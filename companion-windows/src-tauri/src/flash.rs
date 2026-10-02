//! Firmware-Flash aus der App (Spec 6.4): Port der aktiven Verbindung
//! nehmen, Serial-Service anhalten, Image mit `aimonitor_flash` schreiben,
//! Service fortsetzen. Fortschritt geht als Event `flash-progress` ans
//! Frontend; Fehler tragen Zusammenfassung und Detail als i18n-Schlüssel
//! (flash.err.*), dazu die Rohmeldung.
//!
//! Blockierend; der Command ruft [`run`] in `spawn_blocking` auf.

use crate::registry;
use crate::serial_service::{self, Job};
use crate::state::AppState;
use crate::updates;
use aimonitor_core::protocol::{DisplayVariant, PANEL_NO_REPLY};
use aimonitor_core::DeviceInfo;
use aimonitor_flash::{flash_image, validate_merged_image, FlashError, FlashEvent, TargetChip, FLASH_BAUD};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const FLASH_EVENT: &str = "flash-progress";
/// Die App flasht die passende Variante nach, weil das Panel nicht passt.
pub const PANEL_CORRECTION_EVENT: &str = "panel-correction";
/// So lange nach einem Release-Flash darf die App ohne Rückfrage nachflashen.
const CORRECTION_WINDOW: Duration = Duration::from_secs(300);
/// Wartezeit zwischen Port-Freigabe und Flash-Start (main.swift:1412).
const SETTLE_DELAY: Duration = Duration::from_millis(500);
/// Bestätigung des Serial-Threads für die Pause.
const PAUSE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashProgress {
    /// downloading, connecting, connected, erasing, writing, verifying, rebooting, done, failed
    pub phase: &'static str,
    pub variant: DisplayVariant,
    pub percent: Option<u32>,
    /// Freitext, z. B. Chipname oder Rohfehler.
    pub message: Option<String>,
    /// Nur bei `failed`: i18n-Schlüssel flash.err.*.
    pub summary: Option<&'static str>,
    pub detail: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashOutcome {
    pub variant: DisplayVariant,
    pub version: String,
    pub tag: String,
    pub port: String,
    pub seconds: f64,
}

/// Fehlerzuordnung wie `classifyFlashError` der Mac-App (main.swift:1570).
fn classify(error: &FlashError) -> (&'static str, &'static str) {
    match error {
        FlashError::Open(_) => ("flash.err.busy", "flash.err.busy.fix"),
        FlashError::Connect(_) => ("flash.err.nobootmode", "flash.err.bootmode.detail"),
        FlashError::Write(_) => ("flash.err.aborted", "flash.err.aborted.detail"),
        FlashError::EmptyImage => ("flash.err.nofile.title", "flash.err.nofile.detail"),
    }
}

/// Auf die Konsole und ins Ereignisprotokoll der App. Unter Windows sieht
/// niemand die Konsole, Berichte vom Gerät brauchen aber den Ablauf.
fn note(app: &AppHandle, text: String) {
    println!("[flash] {text}");
    serial_service::log(app, format!("Flash: {text}"));
}

fn emit(app: &AppHandle, progress: FlashProgress) {
    if let Err(e) = app.emit(FLASH_EVENT, &progress) {
        eprintln!("[flash] Event nicht gesendet: {e}");
    }
}

fn emit_phase(app: &AppHandle, variant: DisplayVariant, phase: &'static str, percent: Option<u32>, message: Option<String>) {
    emit(app, FlashProgress { phase, variant, percent, message, summary: None, detail: None });
}

fn emit_failed(app: &AppHandle, variant: DisplayVariant, summary: &'static str, detail: &'static str, message: String) {
    note(app, format!("Fehlgeschlagen: {summary} ({message})"));
    emit(app, FlashProgress { phase: "failed", variant, percent: None, message: (!message.is_empty()).then_some(message), summary: Some(summary), detail: Some(detail) });
}

/// Woher ein Flash kam. Entscheidet, ob die App ihn nach dem Neustart anhand
/// des gemeldeten Panels korrigieren darf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashOrigin {
    /// Variante vom Nutzer gewählt oder vom Gerät gemeldet.
    User,
    /// Die App hat die Standard-Variante gewählt, weil sie nur den Chip kannte.
    AutoPicked,
    /// Automatisches Nachflashen nach einem Panel-Abgleich.
    PanelCorrection,
}

/// Letzter Flash-Versuch, für den Panel-Abgleich nach dem Neustart.
#[derive(Debug, Clone, Copy)]
pub struct FlashRecord {
    pub at: Instant,
    pub variant: DisplayVariant,
    pub local: bool,
    pub origin: FlashOrigin,
}

/// Was nach einem Connect aus Panel und laufender Variante folgt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelAction {
    None,
    /// Direkt nachflashen.
    Correct(DisplayVariant),
    /// Nur Hinweis; die Oberfläche bietet den Flash an.
    Notify(DisplayVariant),
}

/// Reine Entscheidung, siehe Mac-App `handlePanelReport`. Nachgeflasht wird
/// nur kurz nach einem eigenen Release-Flash und nie zweimal hintereinander.
/// `noreply` (keine Rückleitung, Verdacht auf ST7789) korrigiert allein die
/// Standard-Wahl der App, nie eine Wahl des Nutzers, und bleibt sonst still.
pub fn panel_action(panel: Option<&str>, firmware: Option<DisplayVariant>, last: Option<&FlashRecord>, now: Instant) -> PanelAction {
    let (Some(panel), Some(firmware)) = (panel, firmware) else {
        return PanelAction::None;
    };
    if firmware.is_esp32s3() {
        return PanelAction::None;
    }
    let recent = last.filter(|r| {
        !r.local
            && r.variant == firmware
            && r.origin != FlashOrigin::PanelCorrection
            && now.saturating_duration_since(r.at) < CORRECTION_WINDOW
    });
    if panel == PANEL_NO_REPLY {
        return match recent {
            Some(r) if r.origin == FlashOrigin::AutoPicked && firmware == DisplayVariant::Ili9341 => PanelAction::Correct(DisplayVariant::St7789),
            _ => PanelAction::None,
        };
    }
    let Some(detected) = DisplayVariant::parse(panel).filter(|v| !v.is_esp32s3()) else {
        return PanelAction::None;
    };
    if detected == firmware {
        PanelAction::None
    } else if recent.is_some() {
        PanelAction::Correct(detected)
    } else {
        PanelAction::Notify(detected)
    }
}

/// Nach jedem Connect vom Serial-Thread aufgerufen. Startet bei Bedarf das
/// Nachflashen in einem eigenen Thread; der Flash pausiert dann den
/// Serial-Service wie jeder andere.
pub fn after_connect(app: &AppHandle, info: &DeviceInfo) {
    let state = app.state::<AppState>();
    let reported = info.panel.is_some() && info.display.is_some();
    if state.flashing.load(Ordering::SeqCst) {
        if reported {
            note(app, "Panel-Abgleich übersprungen, ein Flash läuft noch".into());
        }
        return;
    }
    let last = *state.last_flash.lock().unwrap();
    let now = Instant::now();
    let action = panel_action(info.panel.as_deref(), info.display, last.as_ref(), now);
    if reported {
        note(app, format!("Panel-Abgleich: {}; letzter Flash {} -> {}", info.panel.as_deref().unwrap_or("-"), describe_flash(last.as_ref(), now), describe_action(action)));
    }
    let PanelAction::Correct(variant) = action else {
        return;
    };
    if let Err(e) = app.emit(PANEL_CORRECTION_EVENT, variant) {
        eprintln!("[flash] Event nicht gesendet: {e}");
    }
    let fix_app = app.clone();
    let spawned = std::thread::Builder::new().name("aimonitor-panel-fix".into()).spawn(move || {
        // Erst die Verbindung fertig einrichten lassen.
        std::thread::sleep(Duration::from_secs(1));
        if let Err(e) = run_origin(&fix_app, variant, FlashOrigin::PanelCorrection) {
            note(&fix_app, format!("Nachflashen fehlgeschlagen: {e}"));
        }
    });
    if let Err(e) = spawned {
        note(app, format!("Nachflash-Thread nicht gestartet: {e}"));
    }
}

fn describe_flash(last: Option<&FlashRecord>, now: Instant) -> String {
    match last {
        Some(r) => format!(
            "{} vor {} s ({:?}, {})",
            r.variant.wire(),
            now.saturating_duration_since(r.at).as_secs(),
            r.origin,
            if r.local { "Datei" } else { "Release" }
        ),
        None => "keiner seit App-Start".into(),
    }
}

fn describe_action(action: PanelAction) -> String {
    match action {
        PanelAction::None => "nichts zu tun".into(),
        PanelAction::Correct(v) => format!("flashe {} nach", v.wire()),
        PanelAction::Notify(v) => format!("Hinweis, {} passt", v.wire()),
    }
}

/// Serial-Service anhalten und auf die Freigabe des Ports warten.
fn pause_serial(app: &AppHandle) -> bool {
    let (tx, rx) = mpsc::channel();
    serial_service::send(app, Job::Pause(tx));
    if rx.recv_timeout(PAUSE_TIMEOUT).is_err() {
        serial_service::send(app, Job::Resume { diagnostic_after_connect: false });
        return false;
    }
    std::thread::sleep(SETTLE_DELAY);
    true
}

/// Chip am Port auslesen (`esp32` oder `esp32s3`), danach startet das Board
/// neu. `None`, wenn nichts antwortet oder ein anderer Chip dranhängt.
pub fn detect_chip(app: &AppHandle) -> Result<Option<&'static str>, String> {
    let state = app.state::<AppState>();
    if state.flashing.swap(true, Ordering::SeqCst) {
        return Err("flash.err.running".into());
    }
    let _guard = FlashGuard(&state);
    let port = state.connection.lock().unwrap().port.clone().ok_or_else(|| "esp32.none.info".to_string())?;
    if !pause_serial(app) {
        return Err("flash.err.busy".into());
    }
    let result = aimonitor_flash::detect_chip(&port);
    serial_service::send(app, Job::Resume { diagnostic_after_connect: false });
    let chip = match result {
        Ok(Some(TargetChip::Esp32)) => Some("esp32"),
        Ok(Some(TargetChip::Esp32S3)) => Some("esp32s3"),
        Ok(None) => None,
        Err(e) => {
            note(app, format!("Chip-Erkennung auf {port} fehlgeschlagen: {e}"));
            None
        }
    };
    note(app, format!("Chip-Erkennung auf {port}: {}", chip.unwrap_or("unbekannt")));
    Ok(chip)
}

/// Setzt das Flash-Flag beim Verlassen zurück, auch bei frühem `return`.
struct FlashGuard<'a>(&'a AppState);

impl Drop for FlashGuard<'_> {
    fn drop(&mut self) {
        self.0.flashing.store(false, Ordering::SeqCst);
    }
}

/// Kompletter Ablauf: Download (falls nötig), Pause, Flash, Resume.
/// Fehler kommen als i18n-Schlüssel zurück; das Detail steht im Event.
pub fn run_origin(app: &AppHandle, variant: DisplayVariant, origin: FlashOrigin) -> Result<FlashOutcome, String> {
    run_with_image(app, variant, None, origin)
}

pub fn run_with_image(app: &AppHandle, variant: DisplayVariant, local_path: Option<PathBuf>, origin: FlashOrigin) -> Result<FlashOutcome, String> {
    let state = app.state::<AppState>();
    if state.flashing.swap(true, Ordering::SeqCst) {
        return Err("flash.err.running".into());
    }
    let _guard = FlashGuard(&state);

    let port = state.connection.lock().unwrap().port.clone().ok_or_else(|| "esp32.none.info".to_string())?;
    note(app, format!("Start: Variante {} auf {port} ({origin:?}, {})", variant.wire(), if local_path.is_some() { "Datei" } else { "Release" }));
    *state.last_flash.lock().unwrap() = Some(FlashRecord { at: Instant::now(), variant, local: local_path.is_some(), origin });
    let chip = if variant.is_esp32s3() { TargetChip::Esp32S3 } else { TargetChip::Esp32 };

    // Firmware-Datei sicherstellen; der Download meldet sich über firmware-download.
    let (image, release) = if let Some(path) = local_path {
        let valid_extension = path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("bin"));
        let metadata = std::fs::metadata(&path).map_err(|e| {
            let msg = e.to_string();
            emit_failed(app, variant, "flash.err.nofile.title", "flash.err.nofile.detail", msg.clone());
            msg
        })?;
        if !valid_extension || !metadata.is_file() || metadata.len() > chip.max_image_bytes() {
            emit_failed(app, variant, "flash.err.invalid.title", "flash.local.format.required", String::new());
            return Err("flash.local.format.required".into());
        }
        let image = std::fs::read(&path).map_err(|e| {
            let msg = e.to_string();
            emit_failed(app, variant, "flash.err.nofile.title", "flash.err.nofile.detail", msg.clone());
            msg
        })?;
        (image, None)
    } else {
        emit_phase(app, variant, "downloading", None, None);
        let file = updates::download_firmware(app, variant).map_err(|e| {
            emit_failed(app, variant, "flash.err.nofile.title", "flash.err.nofile.detail", e.clone());
            e
        })?;
        if file.fallback {
            println!("[flash] Hinweis: kein Asset für {}, Standard-Image {} wird verwendet", variant.wire(), file.asset);
        }
        let image = std::fs::read(&file.path).map_err(|e| {
            let msg = format!("{}: {e}", file.path.display());
            emit_failed(app, variant, "flash.err.nofile.title", "flash.err.nofile.detail", msg.clone());
            msg
        })?;
        (image, Some(file))
    };
    if let Err(reason) = validate_merged_image(&image, chip) {
        eprintln!("[flash] Ungültiges Image: {reason:?}");
        let detail = if let Some(file) = &release {
            if let Err(e) = std::fs::remove_file(&file.path) {
                eprintln!("[flash] Ungültige Cache-Datei konnte nicht gelöscht werden: {e}");
            }
            "flash.release.corrupt"
        } else {
            "flash.local.format.required"
        };
        emit_failed(app, variant, "flash.err.invalid.title", detail, String::new());
        return Err(detail.into());
    }

    if !pause_serial(app) {
        let msg = "Serial-Service hat die Pause nicht bestätigt".to_string();
        emit_failed(app, variant, "flash.err.busy", "flash.err.busy.fix", msg.clone());
        return Err(msg);
    }
    println!("[flash] Pause bestätigt, Port {port} frei; {} B ab 0x0 mit {FLASH_BAUD} Baud", image.len());

    let started = Instant::now();
    let app_events = app.clone();
    let mut last_percent: Option<u32> = None;
    let result = flash_image(&port, &image, FLASH_BAUD, chip, &mut |event| {
        let (phase, percent, message) = match &event {
            FlashEvent::Connecting => ("connecting", None, None),
            FlashEvent::Connected { chip } => ("connected", None, Some(chip.clone())),
            FlashEvent::Erasing => ("erasing", None, None),
            FlashEvent::Writing { .. } => ("writing", event.percent(), None),
            FlashEvent::Verifying => ("verifying", None, None),
            FlashEvent::Rebooting => ("rebooting", None, None),
            FlashEvent::Done => ("done", Some(100), None),
        };
        // Schreibfortschritt nur bei geänderter Prozentzahl, Log alle 10 %.
        if phase == "writing" {
            if percent == last_percent {
                return;
            }
            if percent.map(|p| p % 10 == 0).unwrap_or(false) {
                println!("[flash] {:>6.1?} writing {}%", started.elapsed(), percent.unwrap_or(0));
            }
            last_percent = percent;
        } else {
            println!("[flash] {:>6.1?} {phase}{}", started.elapsed(), message.as_deref().map(|m| format!(" ({m})")).unwrap_or_default());
        }
        emit_phase(&app_events, variant, phase, percent, message);
    });

    let outcome = match result {
        Ok(()) => {
            let seconds = started.elapsed().as_secs_f64();
            note(app, format!("Fertig nach {seconds:.1} s: {}", variant.wire()));
            {
                let mut settings = state.settings.lock().unwrap();
                settings.installed_firmware_version = release.as_ref().map(|file| file.version.clone());
                settings.save(app);
            }
            {
                let mut reg = state.registry.lock().unwrap();
                if let Some(profile) = reg.current_profile_mut() {
                    profile.display_variant = Some(variant);
                    registry::save(app, &reg);
                }
            }
            Ok(FlashOutcome { variant, version: release.as_ref().map_or("local".to_string(), |file| file.version.clone()), tag: release.as_ref().map_or("local".to_string(), |file| file.tag.clone()), port: port.clone(), seconds })
        }
        Err(e) => {
            let (summary, detail) = classify(&e);
            emit_failed(app, variant, summary, detail, e.to_string());
            Err(summary.to_string())
        }
    };

    // Service in jedem Fall fortsetzen; Diagnose-Frame nur nach Erfolg.
    serial_service::send(app, Job::Resume { diagnostic_after_connect: outcome.is_ok() });
    updates::emit_status(app);
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(variant: DisplayVariant, origin: FlashOrigin, ago: u64, now: Instant) -> FlashRecord {
        FlashRecord { at: now - Duration::from_secs(ago), variant, local: false, origin }
    }

    #[test]
    fn detected_panel_is_corrected_right_after_own_flash() {
        let now = Instant::now() + Duration::from_secs(3600);
        let fresh = record(DisplayVariant::Ili9341, FlashOrigin::User, 60, now);
        assert_eq!(panel_action(Some("st7789"), Some(DisplayVariant::Ili9341), Some(&fresh), now), PanelAction::Correct(DisplayVariant::St7789));
        // Ohne frischen Flash nur ein Hinweis.
        let old = record(DisplayVariant::Ili9341, FlashOrigin::User, 600, now);
        assert_eq!(panel_action(Some("st7789"), Some(DisplayVariant::Ili9341), Some(&old), now), PanelAction::Notify(DisplayVariant::St7789));
        assert_eq!(panel_action(Some("st7789"), Some(DisplayVariant::Ili9341), None, now), PanelAction::Notify(DisplayVariant::St7789));
        // Nach einer Korrektur nie ein zweites Mal automatisch.
        let fixed = record(DisplayVariant::Ili9341, FlashOrigin::PanelCorrection, 60, now);
        assert_eq!(panel_action(Some("st7789"), Some(DisplayVariant::Ili9341), Some(&fixed), now), PanelAction::Notify(DisplayVariant::St7789));
        // Lokale Datei: keine Korrektur.
        let local = FlashRecord { local: true, ..fresh };
        assert_eq!(panel_action(Some("st7789"), Some(DisplayVariant::Ili9341), Some(&local), now), PanelAction::Notify(DisplayVariant::St7789));
        // Passt: nichts zu tun.
        assert_eq!(panel_action(Some("ili9341"), Some(DisplayVariant::Ili9341), Some(&fresh), now), PanelAction::None);
    }

    #[test]
    fn no_reply_only_corrects_the_apps_own_pick() {
        let now = Instant::now() + Duration::from_secs(3600);
        let picked = record(DisplayVariant::Ili9341, FlashOrigin::AutoPicked, 60, now);
        assert_eq!(panel_action(Some(PANEL_NO_REPLY), Some(DisplayVariant::Ili9341), Some(&picked), now), PanelAction::Correct(DisplayVariant::St7789));
        let chosen = record(DisplayVariant::Ili9341, FlashOrigin::User, 60, now);
        assert_eq!(panel_action(Some(PANEL_NO_REPLY), Some(DisplayVariant::Ili9341), Some(&chosen), now), PanelAction::None);
        assert_eq!(panel_action(Some(PANEL_NO_REPLY), Some(DisplayVariant::St7789), None, now), PanelAction::None);
    }

    #[test]
    fn s3_and_unknown_panels_are_left_alone() {
        let now = Instant::now();
        assert_eq!(panel_action(Some("st7701"), Some(DisplayVariant::St7701), None, now), PanelAction::None);
        assert_eq!(panel_action(Some("ili9342"), Some(DisplayVariant::Ili9341), None, now), PanelAction::None);
        assert_eq!(panel_action(None, Some(DisplayVariant::Ili9341), None, now), PanelAction::None);
    }
}
