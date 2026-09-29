//! Eingebautes Fenster „Claude Code wartet“ (Issue #10).
//!
//! Claude Code schickt Hook-Ereignisse per HTTP an die App (`"type": "http"`
//! in `~/.claude/settings.json`). Dieses Modul wertet sie aus, merkt sich,
//! welche Session auf den Nutzer wartet, und baut daraus eine Szene. Die
//! Firmware zeigt sie wie ein Plugin-Fenster unter `plugin:builtin.claude-code`
//! an, braucht also keine Änderung.
//!
//! Die Mac-App bildet Zustand und Szene in Swift nach
//! (`companion/Sources/ClaudeCodeWindow.swift`); das Eintragen der Hooks
//! läuft auf beiden Plattformen über [`install_hooks`] und [`remove_hooks`].

use crate::plugin::SceneLayout;
use crate::protocol::{Language, Theme};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Plugin-ID des Fensters; der Fensterschlüssel ist `plugin:` + ID.
pub const VIEW_ID: &str = "builtin.claude-code";
/// Präfix, das installierte Plugins nicht verwenden dürfen.
pub const RESERVED_PREFIX: &str = "builtin.";
pub const PORT: u16 = 47651;
pub const PATH: &str = "/claude-code";
pub const TOKEN_HEADER: &str = "X-AIMonitor-Token";
/// Ereignisse, für die die App einen Hook einträgt.
pub const HOOK_EVENTS: [&str; 9] = [
    "Notification",
    "PermissionRequest",
    "Stop",
    "StopFailure",
    "UserPromptSubmit",
    "PostToolUse",
    "PostToolUseFailure",
    "SessionStart",
    "SessionEnd",
];
/// Wartet eine Session so lange ohne neues Ereignis, gilt sie als verlassen.
pub const SESSION_TTL_SECONDS: i64 = 12 * 3600;
pub const MAX_BODY_BYTES: usize = 256 * 1024;
const MAX_SESSIONS: usize = 32;
const MAX_ROWS: usize = 4;
const MAX_PROJECT_CHARS: usize = 28;

pub fn hook_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}{PATH}")
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Waiting {
    Permission,
    Input,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub project: String,
    pub waiting: Option<Waiting>,
    /// Seit wann die Session im aktuellen Wartezustand ist (Unix-Sekunden).
    pub since: i64,
    pub last_event: i64,
}

#[derive(Debug, Default)]
pub struct Sessions {
    sessions: HashMap<String, Session>,
}

impl Sessions {
    /// Ein Hook-Ereignis übernehmen. `true`, wenn sich die Anzeige ändern kann.
    pub fn apply(&mut self, event: &Value, now: i64) -> bool {
        let Some(id) = event
            .get("session_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 128)
        else {
            return false;
        };
        let name = event.get("hook_event_name").and_then(Value::as_str).unwrap_or("");
        if name == "SessionEnd" {
            return self.sessions.remove(id).is_some_and(|s| s.waiting.is_some());
        }
        let current = self.sessions.get(id).and_then(|s| s.waiting);
        let next = match name {
            "PermissionRequest" => Some(Waiting::Permission),
            "Stop" => Some(Waiting::Done),
            "StopFailure" => Some(Waiting::Input),
            "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" | "SessionStart" => None,
            "Notification" => match event.get("notification_type").and_then(Value::as_str) {
                Some("permission_prompt") => Some(Waiting::Permission),
                // Kommt eine Minute nach `Stop`; „fertig“ bleibt dann stehen.
                Some("idle_prompt") if current == Some(Waiting::Done) => current,
                Some("idle_prompt" | "agent_needs_input" | "elicitation_dialog" | "elicitation_url_dialog") => {
                    Some(Waiting::Input)
                }
                Some("agent_completed") => Some(Waiting::Done),
                Some("elicitation_complete" | "elicitation_response") => None,
                _ => return false,
            },
            _ => return false,
        };
        let project = event
            .get("cwd")
            .and_then(Value::as_str)
            .map(project_name)
            .filter(|p| !p.is_empty());
        let session = self.sessions.entry(id.to_owned()).or_insert_with(|| Session {
            id: id.to_owned(),
            project: String::new(),
            waiting: None,
            since: now,
            last_event: now,
        });
        if let Some(project) = project {
            session.project = project;
        }
        if session.waiting != next {
            session.since = now;
        }
        let changed = session.waiting != next || next.is_some();
        session.waiting = next;
        session.last_event = now;
        self.prune(now);
        changed
    }

    /// Verlassene Sessions entfernen und die Anzahl begrenzen.
    pub fn prune(&mut self, now: i64) {
        self.sessions
            .retain(|_, s| now - s.last_event < SESSION_TTL_SECONDS);
        while self.sessions.len() > MAX_SESSIONS {
            let oldest = self
                .sessions
                .values()
                .min_by_key(|s| s.last_event)
                .map(|s| s.id.clone());
            match oldest {
                Some(id) => self.sessions.remove(&id),
                None => break,
            };
        }
    }

    /// Wartende Sessions: Freigabe vor Eingabe vor fertig, darin die älteste zuerst.
    pub fn waiting(&self, now: i64) -> Vec<&Session> {
        let mut list: Vec<&Session> = self
            .sessions
            .values()
            .filter(|s| s.waiting.is_some() && now - s.last_event < SESSION_TTL_SECONDS)
            .collect();
        list.sort_by(|a, b| (a.waiting, a.since, &a.id).cmp(&(b.waiting, b.since, &b.id)));
        list
    }
}

/// Letztes Pfadelement von `cwd` als druckbares ASCII.
pub fn project_name(cwd: &str) -> String {
    let base = cwd
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("");
    ascii(base, MAX_PROJECT_CHARS)
}

/// Umlaute umschreiben, anderes Nicht-ASCII durch `?` ersetzen, kürzen.
pub fn ascii(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let part = match c {
            'ä' => "ae",
            'ö' => "oe",
            'ü' => "ue",
            'Ä' => "Ae",
            'Ö' => "Oe",
            'Ü' => "Ue",
            'ß' => "ss",
            c if (' '..='~').contains(&c) => {
                out.push(c);
                continue;
            }
            _ => "?",
        };
        out.push_str(part);
    }
    if out.chars().count() > max_chars {
        out = out.chars().take(max_chars - 3).collect::<String>() + "...";
    }
    out
}

// ---------------------------------------------------------------------------
// Szene
// ---------------------------------------------------------------------------

/// Hinweis statt Sessionliste, solange die Einrichtung nicht stimmt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setup {
    Ready,
    HooksMissing,
    ListenerFailed,
}

struct Palette {
    background: u32,
    primary: u32,
    secondary: u32,
    divider: u32,
    permission: u32,
    input: u32,
    done: u32,
}

fn palette(theme: Theme) -> Palette {
    match theme {
        Theme::Dark => Palette {
            background: 1580575,
            primary: 0xFFFFFF,
            secondary: 0xABABAB,
            divider: 3717119,
            permission: 0xFF9F0A,
            input: 0xFFD60A,
            done: 0x30D158,
        },
        Theme::Light => Palette {
            background: 0xF5F7FA,
            primary: 0x17212F,
            secondary: 0x45566A,
            divider: 0xD4DDE7,
            permission: 0xC25E00,
            input: 0x8A6D00,
            done: 0x1E7F3A,
        },
    }
}

pub fn text(key: &str, language: Language) -> &'static str {
    match (language, key) {
        (Language::De, "title") => "Claude Code",
        (Language::En, "title") => "Claude Code",
        (Language::De, "quiet") => "Alles ruhig",
        (Language::En, "quiet") => "All quiet",
        (Language::De, "quiet.hint") => "Keine Session wartet auf dich",
        (Language::En, "quiet.hint") => "No session is waiting for you",
        (Language::De, "setup") => "Hooks fehlen",
        (Language::En, "setup") => "Hooks missing",
        (Language::De, "setup.hint") => "In der App unter Plugins einrichten",
        (Language::En, "setup.hint") => "Set them up under Plugins in the app",
        (Language::De, "listener") => "Empfang gestoert",
        (Language::En, "listener") => "Listener failed",
        (Language::De, "listener.hint") => "Port belegt? Details in der App",
        (Language::En, "listener.hint") => "Port in use? Details in the app",
        (Language::De, "permission") => "Wartet auf Freigabe",
        (Language::En, "permission") => "Needs approval",
        (Language::De, "input") => "Wartet auf Eingabe",
        (Language::En, "input") => "Needs input",
        (Language::De, "done") => "Fertig",
        (Language::En, "done") => "Done",
        (Language::De, "now") => "gerade eben",
        (Language::En, "now") => "just now",
        _ => "",
    }
}

fn heading(count: usize, language: Language) -> String {
    match (language, count) {
        (Language::De, 1) => "1 Session wartet".into(),
        (Language::De, n) => format!("{n} Sessions warten"),
        (Language::En, 1) => "1 session waiting".into(),
        (Language::En, n) => format!("{n} sessions waiting"),
    }
}

fn more(count: usize, language: Language) -> String {
    match language {
        Language::De => format!("+{count} weitere"),
        Language::En => format!("+{count} more"),
    }
}

pub fn age(seconds: i64, language: Language) -> String {
    let seconds = seconds.max(0);
    if seconds < 60 {
        text("now", language).into()
    } else if seconds < 3600 {
        format!("{} min", seconds / 60)
    } else if seconds < 86400 {
        format!("{} h", seconds / 3600)
    } else {
        format!("{} d", seconds / 86400)
    }
}

/// Szene für das Fenster. Passt immer in die Grenzen der Firmware
/// (24 Knoten, 1536 Bytes, druckbares ASCII).
pub fn scene(
    sessions: &[&Session],
    setup: Setup,
    now: i64,
    language: Language,
    theme: Theme,
    layout: SceneLayout,
) -> Value {
    let p = palette(theme);
    let (title_font, name_font, status_font) = match layout {
        SceneLayout::Portrait => (20, 16, 14),
        SceneLayout::Landscape | SceneLayout::Square => (24, 20, 14),
    };
    let mut nodes = Vec::new();
    let empty_title = |message: &str, hint: &str, color: u32| {
        vec![
            json!({"type":"text","x":50,"y":60,"w":900,"h":120,"color":p.primary,"font":title_font,"text":text("title", language)}),
            json!({"type":"rect","x":50,"y":200,"w":900,"h":4,"color":p.divider}),
            json!({"type":"text","x":50,"y":380,"w":900,"h":140,"color":color,"font":title_font,"align":"center","text":message}),
            json!({"type":"text","x":50,"y":560,"w":900,"h":140,"color":p.secondary,"font":status_font,"align":"center","text":hint}),
        ]
    };
    match setup {
        Setup::HooksMissing => {
            nodes = empty_title(text("setup", language), text("setup.hint", language), p.permission);
        }
        Setup::ListenerFailed => {
            nodes = empty_title(text("listener", language), text("listener.hint", language), p.permission);
        }
        Setup::Ready if sessions.is_empty() => {
            nodes = empty_title(text("quiet", language), text("quiet.hint", language), p.done);
        }
        Setup::Ready => {
            nodes.push(json!({"type":"text","x":50,"y":40,"w":900,"h":130,"color":p.primary,"font":title_font,"text":heading(sessions.len(), language)}));
            nodes.push(json!({"type":"rect","x":50,"y":185,"w":900,"h":4,"color":p.divider}));
            for (row, session) in sessions.iter().take(MAX_ROWS).enumerate() {
                let y = 215 + row as i64 * 170;
                let (color, key) = match session.waiting {
                    Some(Waiting::Permission) => (p.permission, "permission"),
                    Some(Waiting::Input) => (p.input, "input"),
                    _ => (p.done, "done"),
                };
                let name = if session.project.is_empty() { "Claude Code" } else { &session.project };
                let status = format!("{}, {}", text(key, language), age(now - session.since, language));
                nodes.push(json!({"type":"circle","x":50,"y":y + 25,"w":40,"h":40,"color":color}));
                nodes.push(json!({"type":"text","x":120,"y":y,"w":830,"h":90,"color":p.primary,"font":name_font,"text":name}));
                nodes.push(json!({"type":"text","x":120,"y":y + 90,"w":830,"h":70,"color":color,"font":status_font,"text":status}));
            }
            if sessions.len() > MAX_ROWS {
                nodes.push(json!({"type":"text","x":50,"y":900,"w":900,"h":80,"color":p.secondary,"font":status_font,"text":more(sessions.len() - MAX_ROWS, language)}));
            }
        }
    }
    json!({"background": p.background, "nodes": nodes})
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    /// Es fehlen noch Bytes.
    Incomplete,
    Invalid,
    Complete {
        method: String,
        path: String,
        token: Option<String>,
        body: Vec<u8>,
    },
}

/// Eine HTTP/1.1-Anfrage aus dem bisher gelesenen Puffer lesen. Ohne
/// `Content-Length` gilt der Body als leer; `Transfer-Encoding` schickt
/// Claude Code nicht.
pub fn parse_request(buffer: &[u8]) -> Request {
    let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if buffer.len() > 16 * 1024 { Request::Invalid } else { Request::Incomplete };
    };
    let Ok(head) = std::str::from_utf8(&buffer[..end]) else {
        return Request::Invalid;
    };
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (Some(method), Some(target)) = (first.next(), first.next()) else {
        return Request::Invalid;
    };
    let mut length = 0usize;
    let mut token = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Request::Invalid;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            match value.parse::<usize>() {
                Ok(n) if n <= MAX_BODY_BYTES => length = n,
                _ => return Request::Invalid,
            }
        } else if name.eq_ignore_ascii_case(TOKEN_HEADER) {
            token = Some(value.to_owned());
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Request::Invalid;
        }
    }
    let start = end + 4;
    if buffer.len() < start + length {
        return Request::Incomplete;
    }
    Request::Complete {
        method: method.to_owned(),
        path: target.split('?').next().unwrap_or("").to_owned(),
        token,
        body: buffer[start..start + length].to_vec(),
    }
}

/// HTTP-Status für eine vollständige Anfrage; bei 200 ist das Ereignis gültig.
pub fn check_request(method: &str, path: &str, token: Option<&str>, expected: &str) -> u16 {
    if path != PATH {
        return 404;
    }
    if method != "POST" {
        return 405;
    }
    match token {
        Some(token) if !expected.is_empty() && constant_time_eq(token.as_bytes(), expected.as_bytes()) => 200,
        _ => 401,
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Minimale Antwort. Ein leerer Body heißt für Claude Code: keine Entscheidung,
/// also normaler Ablauf (auch bei `PermissionRequest` und `Stop`).
pub fn response(status: u16) -> String {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
}

// ---------------------------------------------------------------------------
// Hooks in ~/.claude/settings.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookStatus {
    Missing,
    Installed,
    /// Einträge vorhanden, aber Token, Port oder Ereignisse passen nicht.
    Outdated,
}

impl HookStatus {
    pub fn wire(self) -> &'static str {
        match self {
            HookStatus::Missing => "missing",
            HookStatus::Installed => "installed",
            HookStatus::Outdated => "outdated",
        }
    }
}

/// Ein Hook-Eintrag gehört der App, wenn er per HTTP an `127.0.0.1` auf
/// den Pfad [`PATH`] geht, gleich welcher Port.
fn is_ours(hook: &Value) -> bool {
    hook.get("type").and_then(Value::as_str) == Some("http")
        && hook
            .get("url")
            .and_then(Value::as_str)
            .and_then(|url| url.strip_prefix("http://127.0.0.1:"))
            .and_then(|rest| rest.split_once('/'))
            .is_some_and(|(port, path)| port.parse::<u16>().is_ok() && format!("/{path}") == PATH)
}

fn our_hook(port: u16, token: &str) -> Value {
    json!({
        "type": "http",
        "url": hook_url(port),
        "headers": {TOKEN_HEADER: token},
        "timeout": 3
    })
}

fn parse_settings(existing: Option<&str>) -> Result<Map<String, Value>, String> {
    match existing.map(str::trim) {
        None | Some("") => Ok(Map::new()),
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => Err("settings.json ist kein JSON-Objekt".into()),
            Err(e) => Err(format!("settings.json ist kein gültiges JSON: {e}")),
        },
    }
}

/// Eigene Einträge entfernen; fremde Hooks und leere Gruppen anderer bleiben.
fn strip_ours(settings: &mut Map<String, Value>) -> Result<(), String> {
    let Some(hooks) = settings.get_mut("hooks") else {
        return Ok(());
    };
    let Some(events) = hooks.as_object_mut() else {
        return Err("\"hooks\" in settings.json ist kein Objekt".into());
    };
    for groups in events.values_mut() {
        let Some(list) = groups.as_array_mut() else { continue };
        list.retain_mut(|group| {
            let Some(inner) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let before = inner.len();
            inner.retain(|hook| !is_ours(hook));
            before == inner.len() || !inner.is_empty()
        });
    }
    events.retain(|_, groups| groups.as_array().map_or(true, |list| !list.is_empty()));
    if events.is_empty() {
        settings.remove("hooks");
    }
    Ok(())
}

fn render(settings: Map<String, Value>) -> String {
    let mut text = serde_json::to_string_pretty(&Value::Object(settings)).unwrap_or_default();
    text.push('\n');
    text
}

/// Neuer Inhalt für `settings.json` mit einem Hook je Ereignis aus [`HOOK_EVENTS`].
pub fn install_hooks(existing: Option<&str>, port: u16, token: &str) -> Result<String, String> {
    let mut settings = parse_settings(existing)?;
    strip_ours(&mut settings)?;
    let hooks = settings.entry("hooks").or_insert_with(|| json!({}));
    let events = hooks.as_object_mut().ok_or("\"hooks\" in settings.json ist kein Objekt")?;
    for event in HOOK_EVENTS {
        let groups = events.entry(event).or_insert_with(|| json!([]));
        let list = groups
            .as_array_mut()
            .ok_or_else(|| format!("\"hooks.{event}\" in settings.json ist keine Liste"))?;
        list.push(json!({"hooks": [our_hook(port, token)]}));
    }
    Ok(render(settings))
}

pub fn remove_hooks(existing: Option<&str>) -> Result<String, String> {
    let mut settings = parse_settings(existing)?;
    strip_ours(&mut settings)?;
    Ok(render(settings))
}

pub fn hook_status(existing: Option<&str>, port: u16, token: &str) -> HookStatus {
    let Ok(settings) = parse_settings(existing) else {
        return HookStatus::Missing;
    };
    let events = settings.get("hooks").and_then(Value::as_object);
    let mut found = 0;
    let mut matching = 0;
    for event in HOOK_EVENTS {
        let ours: Vec<&Value> = events
            .and_then(|e| e.get(event))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|group| group.get("hooks").and_then(Value::as_array))
            .flatten()
            .filter(|hook| is_ours(hook))
            .collect();
        if !ours.is_empty() {
            found += 1;
        }
        if ours.len() == 1 && *ours[0] == our_hook(port, token) {
            matching += 1;
        }
    }
    let any_ours = events.is_some_and(|e| {
        e.values()
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(|group| group.get("hooks").and_then(Value::as_array))
            .flatten()
            .any(is_ours)
    });
    if matching == HOOK_EVENTS.len() && found == HOOK_EVENTS.len() {
        HookStatus::Installed
    } else if any_ours {
        HookStatus::Outdated
    } else {
        HookStatus::Missing
    }
}

/// Größte `settings.json`, die die App noch anfasst.
pub const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;

pub fn settings_path(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

fn read_settings_file(path: &Path) -> Result<Option<String>, String> {
    match fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
        Ok(meta) if meta.len() > MAX_SETTINGS_BYTES => {
            return Err(format!("{} ist zu groß", path.display()))
        }
        Ok(_) => {}
    }
    fs::read_to_string(path)
        .map(Some)
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub fn file_hook_status(path: &Path, port: u16, token: &str) -> Result<HookStatus, String> {
    Ok(hook_status(read_settings_file(path)?.as_deref(), port, token))
}

/// Hooks eintragen (`install = true`) oder entfernen. Vorher wird der alte
/// Stand als `settings.json.aimonitor-backup` gesichert; geschrieben wird
/// über eine temporäre Datei im selben Ordner.
pub fn write_hooks_file(path: &Path, install: bool, port: u16, token: &str) -> Result<HookStatus, String> {
    let existing = read_settings_file(path)?;
    let updated = if install {
        install_hooks(existing.as_deref(), port, token)?
    } else {
        remove_hooks(existing.as_deref())?
    };
    if existing.as_deref() != Some(updated.as_str()) {
        if !install && existing.is_none() {
            return Ok(HookStatus::Missing);
        }
        let dir = path.parent().ok_or("settings.json ohne Ordner")?;
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        if let Some(old) = &existing {
            let backup = path.with_extension("json.aimonitor-backup");
            fs::write(&backup, old).map_err(|e| format!("{}: {e}", backup.display()))?;
        }
        let temp = path.with_extension("json.aimonitor-tmp");
        fs::write(&temp, &updated).map_err(|e| format!("{}: {e}", temp.display()))?;
        fs::rename(&temp, path).map_err(|e| {
            let _ = fs::remove_file(&temp);
            format!("{}: {e}", path.display())
        })?;
    }
    Ok(hook_status(Some(&updated), port, token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{validate_scene_for_tests, MAX_SCENE_BYTES};

    fn event(name: &str, session: &str, extra: Value) -> Value {
        let mut value = json!({"hook_event_name": name, "session_id": session, "cwd": "/Users/t/Developer/esp32-ai-monitor"});
        if let (Some(map), Some(more)) = (value.as_object_mut(), extra.as_object()) {
            map.extend(more.clone());
        }
        value
    }

    #[test]
    fn events_move_sessions_between_states() {
        let mut s = Sessions::default();
        assert!(s.apply(&event("PermissionRequest", "a", json!({})), 100));
        assert_eq!(s.waiting(100)[0].waiting, Some(Waiting::Permission));
        assert_eq!(s.waiting(100)[0].project, "esp32-ai-monitor");
        // Nach der Freigabe arbeitet Claude weiter.
        assert!(s.apply(&event("PostToolUse", "a", json!({})), 110));
        assert!(s.waiting(110).is_empty());
        assert!(s.apply(&event("Stop", "a", json!({})), 200));
        assert_eq!(s.waiting(200)[0].waiting, Some(Waiting::Done));
        // idle_prompt nach Stop lässt „fertig“ und die Wartezeit stehen.
        s.apply(&event("Notification", "a", json!({"notification_type": "idle_prompt"})), 260);
        assert_eq!(s.waiting(260)[0].waiting, Some(Waiting::Done));
        assert_eq!(s.waiting(260)[0].since, 200);
        assert!(s.apply(&event("UserPromptSubmit", "a", json!({})), 300));
        assert!(s.waiting(300).is_empty());
        s.apply(&event("Notification", "a", json!({"notification_type": "agent_needs_input"})), 310);
        assert_eq!(s.waiting(310)[0].waiting, Some(Waiting::Input));
        assert!(s.apply(&event("SessionEnd", "a", json!({})), 320));
        assert!(s.waiting(320).is_empty());
    }

    #[test]
    fn unknown_or_invalid_events_are_ignored() {
        let mut s = Sessions::default();
        assert!(!s.apply(&json!({"hook_event_name": "Stop"}), 1));
        assert!(!s.apply(&event("PreCompact", "a", json!({})), 1));
        assert!(!s.apply(&event("Notification", "a", json!({"notification_type": "auth_success"})), 1));
        assert!(s.waiting(1).is_empty());
    }

    #[test]
    fn waiting_order_and_expiry() {
        let mut s = Sessions::default();
        s.apply(&event("Stop", "done", json!({})), 10);
        s.apply(&event("Notification", "input", json!({"notification_type": "idle_prompt"})), 20);
        s.apply(&event("PermissionRequest", "perm", json!({})), 30);
        let order: Vec<&str> = s.waiting(40).iter().map(|x| x.id.as_str()).collect();
        assert_eq!(order, ["perm", "input", "done"]);
        assert!(s.waiting(10 + SESSION_TTL_SECONDS).iter().all(|x| x.id != "done"));
    }

    #[test]
    fn project_names_are_ascii() {
        assert_eq!(project_name("/Users/t/Projekte/Größe/"), "Groesse");
        assert_eq!(project_name("C:\\Users\\t\\café"), "caf?");
        assert_eq!(project_name(&format!("/x/{}", "a".repeat(40))).len(), MAX_PROJECT_CHARS);
    }

    #[test]
    fn scenes_fit_firmware_limits() {
        let mut s = Sessions::default();
        for i in 0..6 {
            s.apply(&event("PermissionRequest", &format!("s{i}"), json!({"cwd": format!("/x/{}{i}", "Ü".repeat(30))})), 0);
        }
        let waiting = s.waiting(40_000);
        let cases = [
            (waiting.clone(), Setup::Ready),
            (vec![], Setup::Ready),
            (vec![], Setup::HooksMissing),
            (vec![], Setup::ListenerFailed),
        ];
        for (list, setup) in cases {
            for language in [Language::De, Language::En] {
                for theme in [Theme::Dark, Theme::Light] {
                    for layout in [SceneLayout::Portrait, SceneLayout::Landscape, SceneLayout::Square] {
                        let scene = scene(&list, setup, 40_000, language, theme, layout);
                        validate_scene_for_tests(&scene).unwrap();
                        let bytes = scene.to_string().len();
                        assert!(bytes <= MAX_SCENE_BYTES, "{bytes} Bytes");
                    }
                }
            }
        }
        let full = scene(&waiting, Setup::Ready, 40_000, Language::De, Theme::Dark, SceneLayout::Square);
        let texts: Vec<&str> = full["nodes"].as_array().unwrap().iter().filter_map(|n| n["text"].as_str()).collect();
        assert!(texts.contains(&"6 Sessions warten"));
        assert!(texts.contains(&"+2 weitere"));
        assert!(texts.contains(&"Wartet auf Freigabe, 11 h"));
    }

    #[test]
    fn ages_are_short() {
        assert_eq!(age(30, Language::De), "gerade eben");
        assert_eq!(age(125, Language::En), "2 min");
        assert_eq!(age(7300, Language::De), "2 h");
        assert_eq!(age(200_000, Language::En), "2 d");
    }

    #[test]
    fn requests_are_parsed_and_checked() {
        let body = br#"{"hook_event_name":"Stop"}"#;
        let raw = format!(
            "POST /claude-code HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nx-aimonitor-token: abc\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let mut full = raw.clone().into_bytes();
        full.extend_from_slice(body);
        assert_eq!(parse_request(&full[..full.len() - 3]), Request::Incomplete);
        let Request::Complete { method, path, token, body: parsed } = parse_request(&full) else {
            panic!("nicht vollständig");
        };
        assert_eq!(parsed, body);
        assert_eq!(check_request(&method, &path, token.as_deref(), "abc"), 200);
        assert_eq!(check_request(&method, &path, token.as_deref(), "abd"), 401);
        assert_eq!(check_request(&method, &path, None, "abc"), 401);
        assert_eq!(check_request("GET", &path, token.as_deref(), "abc"), 405);
        assert_eq!(check_request(&method, "/other", token.as_deref(), "abc"), 404);
        assert_eq!(parse_request(b"POST / HTTP/1.1\r\nContent-Length: 999999999\r\n\r\n"), Request::Invalid);
    }

    #[test]
    fn hooks_are_merged_without_touching_foreign_entries() {
        let existing = r#"{
          "model": "opus",
          "hooks": {
            "Stop": [{"hooks": [{"type": "command", "command": "say fertig"}]}],
            "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "check"}]}]
          }
        }"#;
        assert_eq!(hook_status(Some(existing), PORT, "t1"), HookStatus::Missing);
        let installed = install_hooks(Some(existing), PORT, "t1").unwrap();
        assert_eq!(hook_status(Some(&installed), PORT, "t1"), HookStatus::Installed);
        assert_eq!(hook_status(Some(&installed), PORT, "t2"), HookStatus::Outdated);
        let value: Value = serde_json::from_str(&installed).unwrap();
        assert_eq!(value["model"], "opus");
        assert_eq!(value["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(value["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "check");
        // Zweimal eintragen ergibt keine Dubletten.
        let again = install_hooks(Some(&installed), PORT, "t2").unwrap();
        let value: Value = serde_json::from_str(&again).unwrap();
        assert_eq!(value["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(hook_status(Some(&again), PORT, "t2"), HookStatus::Installed);
        // Entfernen stellt den fremden Stand wieder her.
        let removed: Value = serde_json::from_str(&remove_hooks(Some(&again)).unwrap()).unwrap();
        let original: Value = serde_json::from_str(existing).unwrap();
        assert_eq!(removed, original);
        let empty: Value = serde_json::from_str(&remove_hooks(Some(&install_hooks(None, PORT, "x").unwrap())).unwrap()).unwrap();
        assert_eq!(empty, json!({}));
    }

    #[test]
    fn settings_file_is_backed_up_and_replaced() {
        let dir = std::env::temp_dir().join(format!("aimonitor-cc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = settings_path(&dir);
        assert_eq!(file_hook_status(&path, PORT, "t").unwrap(), HookStatus::Missing);
        // Entfernen ohne Datei legt keine an.
        assert_eq!(write_hooks_file(&path, false, PORT, "t").unwrap(), HookStatus::Missing);
        assert!(!path.exists());
        assert_eq!(write_hooks_file(&path, true, PORT, "t").unwrap(), HookStatus::Installed);
        assert!(!path.with_extension("json.aimonitor-backup").exists());
        fs::write(&path, r#"{"model":"opus"}"#).unwrap();
        assert_eq!(write_hooks_file(&path, true, PORT, "t").unwrap(), HookStatus::Installed);
        assert_eq!(fs::read_to_string(path.with_extension("json.aimonitor-backup")).unwrap(), r#"{"model":"opus"}"#);
        assert_eq!(file_hook_status(&path, PORT, "t").unwrap(), HookStatus::Installed);
        assert_eq!(write_hooks_file(&path, false, PORT, "t").unwrap(), HookStatus::Missing);
        let back: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back, json!({"model": "opus"}));
        fs::write(&path, "{ kaputt").unwrap();
        assert!(write_hooks_file(&path, true, PORT, "t").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ kaputt");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_settings_are_not_overwritten() {
        assert!(install_hooks(Some("{ kaputt"), PORT, "t").is_err());
        assert!(install_hooks(Some("[]"), PORT, "t").is_err());
        assert!(install_hooks(Some(r#"{"hooks": []}"#), PORT, "t").is_err());
        assert!(install_hooks(Some(r#"{"hooks": {"Stop": {}}}"#), PORT, "t").is_err());
    }
}
