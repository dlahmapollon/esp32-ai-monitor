//! Persistente Einstellungen der App als JSON unter `app_config_dir()/settings.json`.

use aimonitor_core::release::UpdateChannel;
use aimonitor_core::{PercentMode, Provider};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Sprache der Oberfläche. `System` folgt der Betriebssystemsprache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    System,
    De,
    En,
}

/// Inhalt eines Display-Fensters. Eine Quelle darf in mehreren Fenstern liegen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "provider", rename_all = "lowercase")]
pub enum ViewContent {
    Clock,
    Provider(Provider),
    Plugin(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    #[default]
    Manual,
    Automatic,
    Intelligent,
}

pub const MAX_VIEWS: usize = 8;
fn default_views() -> Vec<ViewContent> {
    vec![ViewContent::Provider(Provider::DEFAULT)]
}
fn default_view_interval() -> u16 {
    10
}

impl Language {
    /// Tatsächlich zu verwendende Sprache: "de" oder "en".
    pub fn effective(self) -> &'static str {
        match self {
            Language::De => "de",
            Language::En => "en",
            Language::System => {
                let locale = sys_locale::get_locale()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if locale.starts_with("de") {
                    "de"
                } else {
                    "en"
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Internal revision, changed under the settings lock before publishing assignments.
    /// Never supplied by the frontend or persisted across application launches.
    #[serde(skip)]
    pub(crate) smart_revision: u64,
    #[serde(skip)]
    pub(crate) smart_manual: bool,
    #[serde(default = "default_provider")]
    pub provider: Provider,
    #[serde(default = "default_views")]
    pub views: Vec<ViewContent>,
    #[serde(default)]
    pub view_mode: ViewMode,
    #[serde(default = "default_view_interval")]
    pub view_interval_seconds: u16,
    #[serde(default)]
    pub active_view: usize,
    #[serde(default)]
    pub percent_mode: PercentMode,
    #[serde(default)]
    pub language: Language,
    #[serde(default)]
    pub autostart: bool,
    /// Fest gewählter serieller Port; `None` heißt automatische Wahl.
    #[serde(default)]
    pub manual_port: Option<String>,
    /// `auto` (Systemzeitzone) oder ein IANA-Name wie `Europe/Berlin`.
    #[serde(default = "default_timezone")]
    pub timezone: String,
    /// Release-Kanal für App und Firmware (Phase 3).
    #[serde(default)]
    pub update_channel: UpdateChannel,
    /// Version der zuletzt aus dieser App geflashten Firmware (Tag ohne Präfix).
    #[serde(default)]
    pub installed_firmware_version: Option<String>,
    /// Zeitpunkt der letzten erfolgreichen Release-Abfrage.
    #[serde(default)]
    pub last_update_check: Option<DateTime<Utc>>,
}

fn default_timezone() -> String {
    "auto".into()
}

fn default_provider() -> Provider {
    Provider::DEFAULT
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            smart_revision: 0,
            smart_manual: false,
            provider: Provider::DEFAULT,
            views: default_views(),
            view_mode: ViewMode::Manual,
            view_interval_seconds: default_view_interval(),
            active_view: 0,
            percent_mode: PercentMode::Used,
            language: Language::System,
            autostart: false,
            manual_port: None,
            timezone: default_timezone(),
            update_channel: UpdateChannel::Stable,
            installed_firmware_version: None,
            last_update_check: None,
        }
    }
}

impl Settings {
    pub(crate) fn mark_smart_change(&mut self, manual: bool) {
        self.smart_revision = self.smart_revision.wrapping_add(1);
        self.smart_manual = manual;
    }

    /// Apply only the fields the user edited to the latest backend state.
    /// Internal smart metadata is copied from self and is not accepted as input.
    pub(crate) fn apply_patch(&self, patch: &serde_json::Map<String, Value>) -> Result<Self, String> {
        let mut value = serde_json::to_value(self).map_err(|e| e.to_string())?;
        let fields = value.as_object_mut().unwrap();
        for (key, value) in patch {
            if !fields.contains_key(key) {
                return Err(format!("unknown setting: {key}"));
            }
            fields.insert(key.clone(), value.clone());
        }
        let mut next: Self = serde_json::from_value(value).map_err(|e| e.to_string())?;
        next.normalize_views();
        next.smart_revision = self.smart_revision;
        next.smart_manual = self.smart_manual;
        if (next.view_mode == ViewMode::Intelligent || self.view_mode == ViewMode::Intelligent)
            && (next.views != self.views || next.view_mode != self.view_mode
                || next.active_view != self.active_view)
        {
            let manual = next.view_mode == ViewMode::Intelligent
                && (next.active_view != self.active_view
                    || next.views.get(next.active_view) != self.views.get(self.active_view));
            next.mark_smart_change(manual);
        }
        Ok(next)
    }

    fn path(app: &AppHandle) -> Option<PathBuf> {
        app.path()
            .app_config_dir()
            .ok()
            .map(|d| d.join("settings.json"))
    }

    /// Laden; bei fehlender oder unlesbarer Datei die Defaults.
    pub fn load(app: &AppHandle) -> Settings {
        let Some(path) = Self::path(app) else {
            return Settings::default();
        };
        match std::fs::read(&path) {
            Ok(bytes) => Self::from_disk(&bytes).unwrap_or_else(|e| {
                eprintln!("[aimonitor] settings.json unlesbar ({e}), Defaults");
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    fn from_disk(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        let mut value: Value = serde_json::from_slice(bytes)?;
        let had_views = value.get("views").is_some();
        // Older companions can read the stored manual mode and ignore this
        // extra field. The current app restores the intelligent mode.
        if value.get("intelligentViews").and_then(Value::as_bool) == Some(true) {
            value["viewMode"] = json!("intelligent");
        }
        let mut settings: Settings = serde_json::from_value(value)?;
        if !had_views {
            settings.views = vec![ViewContent::Provider(settings.provider)];
        }
        settings.normalize_views();
        Ok(settings)
    }

    fn to_disk_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut value = serde_json::to_value(self)?;
        if self.view_mode == ViewMode::Intelligent {
            value["viewMode"] = json!("manual");
            value["intelligentViews"] = json!(true);
        }
        serde_json::to_vec_pretty(&value)
    }

    pub fn normalize_views(&mut self) {
        if self.views.is_empty() {
            self.views = default_views();
        }
        self.views.truncate(MAX_VIEWS);
        for view in &mut self.views {
            if let ViewContent::Plugin(id) = view {
                if id.is_empty()
                    || id.len() > 40
                    || !id.bytes().all(|c| {
                        c.is_ascii_lowercase()
                            || c.is_ascii_digit()
                            || matches!(c, b'.' | b'-' | b'_')
                    })
                {
                    *view = ViewContent::Clock;
                }
            }
        }
        self.view_interval_seconds = self.view_interval_seconds.clamp(2, 3600);
        self.active_view = self.active_view.min(self.views.len() - 1);
    }

    pub fn save(&self, app: &AppHandle) {
        let Some(path) = Self::path(app) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match self.to_disk_bytes() {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&path, bytes) {
                    eprintln!("[aimonitor] Einstellungen nicht gespeichert: {e}");
                }
            }
            Err(e) => eprintln!("[aimonitor] Einstellungen nicht serialisierbar: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_patch_preserves_a_selection_made_after_the_frontend_snapshot() {
        let mut current = Settings::default();
        current.view_mode = ViewMode::Intelligent;
        current.views.push(ViewContent::Clock);
        current.active_view = 1;
        current.provider = Provider::Claude;
        current.smart_revision = 4;
        let patch = serde_json::from_value(json!({"viewIntervalSeconds": 30})).unwrap();
        let submitted = current.apply_patch(&patch).unwrap();
        assert_eq!(submitted.active_view, 1);
        assert_eq!(submitted.provider, Provider::Claude);
        assert_eq!(submitted.smart_revision, 4);
        assert_eq!(submitted.view_interval_seconds, 30);
    }

    #[test]
    fn explicit_selection_advances_revision_but_internal_fields_are_not_serialized() {
        let mut current = Settings::default();
        current.view_mode = ViewMode::Intelligent;
        current.views.push(ViewContent::Clock);
        current.smart_revision = 4;
        let patch = serde_json::from_value(json!({"activeView": 1})).unwrap();
        let submitted = current.apply_patch(&patch).unwrap();
        assert_eq!(submitted.active_view, 1);
        assert_eq!(submitted.smart_revision, 5);
        assert!(submitted.smart_manual);
        let json = serde_json::to_value(&submitted).unwrap();
        assert!(json.get("smartRevision").is_none());
        assert!(json.get("smartManual").is_none());
        let forged = serde_json::from_value(json!({"smartRevision": 0})).unwrap();
        assert!(current.apply_patch(&forged).is_err());
    }

    #[test]
    fn intelligent_mode_on_disk_is_readable_by_older_companions() {
        let mut settings = Settings::default();
        settings.provider = Provider::Codex;
        settings.views = vec![ViewContent::Provider(Provider::Codex), ViewContent::Clock];
        settings.view_mode = ViewMode::Intelligent;
        let bytes = settings.to_disk_bytes().unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["viewMode"], "manual");
        assert_eq!(value["intelligentViews"], true);
        assert_eq!(Settings::from_disk(&bytes).unwrap().view_mode, ViewMode::Intelligent);
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct OldSettings {
            provider: Provider,
            views: Vec<ViewContent>,
            view_mode: OldViewMode,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "lowercase")]
        enum OldViewMode { Manual, Automatic }
        let old: OldSettings = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(old.provider, Provider::Codex);
        assert_eq!(old.views.len(), 2);
        assert!(matches!(old.view_mode, OldViewMode::Manual));
    }

    #[test]
    fn view_settings_keep_one_window_and_clamp_limits() {
        let mut settings = Settings::default();
        settings.views.clear();
        settings.active_view = 99;
        settings.view_interval_seconds = 1;
        settings.normalize_views();
        assert_eq!(
            settings.views,
            vec![ViewContent::Provider(Provider::Claude)]
        );
        assert_eq!(settings.active_view, 0);
        assert_eq!(settings.view_interval_seconds, 2);

        settings.views = vec![ViewContent::Clock; MAX_VIEWS + 2];
        settings.active_view = MAX_VIEWS + 1;
        settings.normalize_views();
        assert_eq!(settings.views.len(), MAX_VIEWS);
        assert_eq!(settings.active_view, MAX_VIEWS - 1);
    }

    #[test]
    fn view_content_has_a_stable_wire_shape() {
        assert_eq!(
            serde_json::to_value(ViewContent::Clock).unwrap(),
            serde_json::json!({"kind":"clock"})
        );
        assert_eq!(
            serde_json::to_value(ViewContent::Provider(Provider::Codex)).unwrap(),
            serde_json::json!({"kind":"provider","provider":"codex"})
        );
        assert_eq!(
            serde_json::to_value(ViewContent::Plugin("org.example.status".into())).unwrap(),
            serde_json::json!({"kind":"plugin","provider":"org.example.status"})
        );
    }
}
