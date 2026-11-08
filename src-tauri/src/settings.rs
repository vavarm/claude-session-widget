use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "PascalCase", default)]
pub struct Settings {
    pub always_on_top: bool,
    pub compact: bool,
    pub widget_visible: bool,
    pub opacity: f64,
    pub locked: bool,
    pub click_through: bool,
    #[serde(alias = "ShowQuotas")]
    pub show_context: bool,
    pub show_agents: bool,
    pub fold_idle: bool,
    pub auto_collapse: bool,
    pub notify_attention: bool,
    pub notify_finished: bool,
    pub shortcut: String,
    pub x: Option<i32>,
    pub y: Option<i32>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            always_on_top: true,
            compact: false,
            widget_visible: true,
            opacity: 0.9,
            locked: false,
            click_through: false,
            show_context: false,
            show_agents: false,
            fold_idle: false,
            auto_collapse: false,
            notify_attention: false,
            notify_finished: false,
            shortcut: String::new(),
            x: None,
            y: None,
        }
    }
}

pub fn data_dir() -> PathBuf {
    match std::env::var_os("CLAUDE_WIDGET_DATA_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => dirs::config_dir()
            .unwrap_or_default()
            .join("ClaudeSessionWidget"),
    }
}

// Several Claude Code sessions write the same files at once; renaming keeps readers from seeing a half-written one.
pub fn write_atomically(file: &Path, text: &str) {
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let temp = file.with_extension("tmp");
    if fs::write(&temp, text).is_ok() {
        let _ = fs::rename(&temp, file);
    }
}

// Session ids become file names, so anything but a plain identifier is refused.
pub fn session_file(dir: &Path, session_id: &str) -> Option<PathBuf> {
    let plain = !session_id.is_empty()
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    plain.then(|| dir.join(format!("{session_id}.json")))
}

// Per-session files of sessions long gone are not worth keeping.
pub fn prune_older_than(dir: &Path, age: std::time::Duration) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|elapsed| elapsed > age);
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

impl Settings {
    fn file() -> PathBuf {
        data_dir().join("settings.json")
    }

    pub fn load() -> Self {
        fs::read_to_string(Self::file())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = fs::create_dir_all(data_dir());
        if let Ok(text) = serde_json::to_string(self) {
            let _ = fs::write(Self::file(), text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"Compact":true}"#).unwrap();
        assert!(s.compact);
        assert!(s.always_on_top);
        assert_eq!(s.opacity, 0.9);
    }

    #[test]
    fn round_trips() {
        let s = Settings {
            x: Some(3),
            y: Some(4),
            opacity: 0.6,
            ..Settings::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!((back.x, back.y, back.opacity), (Some(3), Some(4), 0.6));
    }
}
