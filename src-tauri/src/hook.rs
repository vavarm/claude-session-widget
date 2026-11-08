use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

// A command entry in Claude Code's settings.json that Claude Code feeds with live data.
// The widget registers itself as that command, with a flag selecting what to record.
#[derive(Clone, Copy)]
pub struct Hook {
    pub key: &'static str,
    pub flag: &'static str,
}

pub const STATUS_LINE: Hook = Hook {
    key: "statusLine",
    flag: "--statusline",
};
pub const SUBAGENT_STATUS_LINE: Hook = Hook {
    key: "subagentStatusLine",
    flag: "--subagent-status",
};
pub const ALL: [Hook; 2] = [STATUS_LINE, SUBAGENT_STATUS_LINE];

pub fn claude_settings_file() -> PathBuf {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".claude"));
    dir.join("settings.json")
}

fn read_settings(file: &Path) -> Result<Value, String> {
    match fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{} is not valid JSON: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("cannot read {}: {e}", file.display())),
    }
}

fn write_settings(file: &Path, value: &Value) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    text.push('\n');
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(file, text).map_err(|e| format!("cannot write {}: {e}", file.display()))
}

impl Hook {
    fn command(&self, exe: &Path) -> String {
        // Forward slashes work in cmd, PowerShell and POSIX shells alike.
        format!(
            "\"{}\" {}",
            exe.to_string_lossy().replace('\\', "/"),
            self.flag
        )
    }

    fn is_ours(&self, command: &str) -> bool {
        command.contains(self.flag) && command.contains("ClaudeSessionWidget")
    }

    fn current_command<'a>(&self, root: &'a Value) -> Option<&'a str> {
        root.get(self.key)?.get("command")?.as_str()
    }

    // Refuses to replace an entry the user set up themselves.
    pub fn enable(&self, file: &Path, exe: &Path) -> Result<(), String> {
        let mut root = read_settings(file)?;
        if !root.is_object() {
            return Err(format!("{} is not a JSON object", file.display()));
        }
        if root.get(self.key).is_some()
            && !self.current_command(&root).is_some_and(|c| self.is_ours(c))
        {
            return Err(format!(
                "Claude Code already has a custom {} configured; the widget will not replace it.",
                self.key
            ));
        }
        let wanted = json!({ "type": "command", "command": self.command(exe) });
        if root.get(self.key) == Some(&wanted) {
            return Ok(());
        }
        root[self.key] = wanted;
        write_settings(file, &root)
    }

    pub fn disable(&self, file: &Path) -> Result<(), String> {
        let mut root = read_settings(file)?;
        if self.current_command(&root).is_some_and(|c| self.is_ours(c)) {
            if let Some(object) = root.as_object_mut() {
                object.remove(self.key);
            }
            write_settings(file, &root)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("csw-hook-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    const EXE: &str = "C:\\Apps\\ClaudeSessionWidget.exe";

    #[test]
    fn enabling_keeps_other_settings_and_is_idempotent() {
        let file = temp_file("enable");
        fs::write(&file, r#"{"theme":"dark","hooks":{"a":1}}"#).unwrap();
        STATUS_LINE.enable(&file, Path::new(EXE)).unwrap();
        let first = fs::read_to_string(&file).unwrap();
        STATUS_LINE.enable(&file, Path::new(EXE)).unwrap();
        assert_eq!(first, fs::read_to_string(&file).unwrap());

        let value: Value = serde_json::from_str(&first).unwrap();
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["hooks"]["a"], 1);
        assert_eq!(
            value["statusLine"]["command"],
            "\"C:/Apps/ClaudeSessionWidget.exe\" --statusline"
        );
    }

    #[test]
    fn both_hooks_coexist_and_are_removed_independently() {
        let file = temp_file("both");
        for hook in ALL {
            hook.enable(&file, Path::new(EXE)).unwrap();
        }
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("statusLine") && text.contains("subagentStatusLine"));

        SUBAGENT_STATUS_LINE.disable(&file).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("statusLine") && !text.contains("subagentStatusLine"));
    }

    #[test]
    fn enabling_creates_a_missing_settings_file() {
        let file = temp_file("create");
        STATUS_LINE.enable(&file, Path::new(EXE)).unwrap();
        assert!(fs::read_to_string(&file).unwrap().contains("statusLine"));
    }

    #[test]
    fn enabling_updates_a_moved_executable() {
        let file = temp_file("moved");
        STATUS_LINE
            .enable(&file, Path::new("D:\\Old\\ClaudeSessionWidget.exe"))
            .unwrap();
        STATUS_LINE.enable(&file, Path::new(EXE)).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("C:/Apps/ClaudeSessionWidget.exe"));
        assert!(!text.contains("Old"));
    }

    #[test]
    fn enabling_never_replaces_a_foreign_entry() {
        let file = temp_file("foreign");
        let original = r#"{"statusLine":{"type":"command","command":"my-script.sh"}}"#;
        fs::write(&file, original).unwrap();
        assert!(STATUS_LINE.enable(&file, Path::new(EXE)).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), original);
    }

    #[test]
    fn invalid_settings_are_left_untouched() {
        let file = temp_file("invalid");
        fs::write(&file, "{ broken").unwrap();
        assert!(STATUS_LINE.enable(&file, Path::new(EXE)).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "{ broken");
    }

    #[test]
    fn disabling_removes_only_our_entry() {
        let file = temp_file("disable");
        STATUS_LINE.enable(&file, Path::new(EXE)).unwrap();
        STATUS_LINE.disable(&file).unwrap();
        assert!(!fs::read_to_string(&file).unwrap().contains("statusLine"));

        let foreign = r#"{"statusLine":{"type":"command","command":"my-script.sh"}}"#;
        fs::write(&file, foreign).unwrap();
        STATUS_LINE.disable(&file).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), foreign);
    }
}
