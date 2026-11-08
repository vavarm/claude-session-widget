use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// Claude Code records every session under ~/.claude/projects: the conversation as <id>.jsonl and
// each subagent under <id>/subagents. They are written by the terminal, VS Code and desktop clients alike.
const TAIL_BYTES: u64 = 256 * 1024;
const STANDARD_WINDOW: f64 = 200_000.0;
const LARGE_WINDOW: f64 = 1_000_000.0;
// A running subagent that has written nothing for this long is taken for a crashed one.
const SILENT_AFTER: Duration = Duration::from_secs(15 * 60);

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Info {
    pub model: String,
    pub context_percent: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub kind: String,
    pub agent_type: String,
    pub description: String,
    pub start_time: i64,
}

pub fn projects_dir() -> PathBuf {
    let base = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".claude"));
    base.join("projects")
}

// Session ids become file names, so anything but a plain identifier is refused.
fn plain(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn find(projects: &Path, session_id: &str) -> Option<PathBuf> {
    if !plain(session_id) {
        return None;
    }
    fs::read_dir(projects)
        .ok()?
        .flatten()
        .map(|project| project.path().join(format!("{session_id}.jsonl")))
        .find(|path| path.is_file())
}

fn millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn tail(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

// The conversation grows by one assistant message at a time; the last one says how full the context is.
fn info_from(text: &str) -> Option<Info> {
    let message = text
        .lines()
        .rev()
        .filter(|line| line.contains("\"type\":\"assistant\""))
        .find_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            let message = value.get("message")?.clone();
            message.get("usage")?;
            Some(message)
        })?;
    let usage = message.get("usage")?;
    let tokens: f64 = [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ]
    .iter()
    .filter_map(|key| usage.get(key)?.as_f64())
    .sum();
    let model = message.get("model")?.as_str()?.to_string();
    // The transcript does not name the window: a context already past the standard one must be the large one.
    let window = if model.contains("[1m]") || tokens > STANDARD_WINDOW {
        LARGE_WINDOW
    } else {
        STANDARD_WINDOW
    };
    Some(Info {
        model,
        context_percent: Some((tokens / window * 100.0).min(100.0)),
    })
}

pub fn info(path: &Path) -> Option<Info> {
    type Cached = (SystemTime, u64, Option<Info>);
    static CACHE: Mutex<Option<HashMap<PathBuf, Cached>>> = Mutex::new(None);
    let meta = fs::metadata(path).ok()?;
    let key = (meta.modified().ok()?, meta.len());
    let mut guard = CACHE.lock().unwrap();
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some((modified, len, info)) = cache.get(path) {
        if (*modified, *len) == key {
            return info.clone();
        }
    }
    let info = tail(path).and_then(|text| info_from(&text));
    cache.insert(path.to_path_buf(), (key.0, key.1, info.clone()));
    info
}

fn task_from(meta: &Path, now: SystemTime) -> Option<Task> {
    let value: Value = serde_json::from_str(&fs::read_to_string(meta).ok()?).ok()?;
    if value.get("finished").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let name = meta.file_name()?.to_str()?.strip_suffix(".meta.json")?;
    let transcript = meta.with_file_name(format!("{name}.jsonl"));
    let written = fs::metadata(&transcript).ok()?;
    if now.duration_since(written.modified().ok()?).ok()? > SILENT_AFTER {
        return None;
    }
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    Some(Task {
        id: name.to_string(),
        kind: "agent".to_string(),
        agent_type: text("agentType"),
        description: text("description"),
        start_time: millis(written.created().or_else(|_| written.modified()).ok()?),
    })
}

// Subagents of a session that have not reported back yet, oldest first.
pub fn agents(transcript: &Path) -> Vec<Task> {
    let dir = transcript.with_extension("").join("subagents");
    let now = SystemTime::now();
    let mut tasks: Vec<Task> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".meta.json"))
        .filter_map(|meta| task_from(&meta, now))
        .collect();
    tasks.sort_by_key(|t| t.start_time);
    tasks
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSISTANT: &str = r#"{"type":"assistant","message":{"model":"claude-sonnet-5-5","usage":{"input_tokens":2,"cache_creation_input_tokens":1000,"cache_read_input_tokens":49000,"output_tokens":9}}}"#;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("csw-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn context_is_the_last_assistant_message_against_the_standard_window() {
        let text = format!("{ASSISTANT}\n{{\"type\":\"user\"}}\n");
        let info = info_from(&text).unwrap();
        assert_eq!(info.model, "claude-sonnet-5-5");
        assert_eq!(info.context_percent.unwrap().round(), 25.0);
    }

    #[test]
    fn a_context_past_the_standard_window_is_measured_against_the_large_one() {
        let big = ASSISTANT.replace("49000", "449000");
        assert_eq!(
            info_from(&big).unwrap().context_percent.unwrap().round(),
            45.0
        );
    }

    #[test]
    fn the_newest_assistant_message_wins_and_a_torn_first_line_is_ignored() {
        let newer = ASSISTANT.replace("49000", "99000");
        let text = format!("garbage cut off\n{ASSISTANT}\n{newer}\n");
        assert_eq!(
            info_from(&text).unwrap().context_percent.unwrap().round(),
            50.0
        );
        assert!(info_from("{\"type\":\"user\"}\n").is_none());
    }

    #[test]
    fn finds_the_transcript_of_a_session_and_refuses_odd_ids() {
        let root = scratch("find");
        fs::create_dir_all(root.join("proj")).unwrap();
        fs::write(root.join("proj").join("abc-1.jsonl"), "").unwrap();
        assert!(find(&root, "abc-1").is_some());
        assert!(find(&root, "missing").is_none());
        assert!(find(&root, "../abc-1").is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn lists_only_subagents_that_are_still_running() {
        let root = scratch("agents");
        let transcript = root.join("s1.jsonl");
        let dir = root.join("s1").join("subagents");
        fs::create_dir_all(&dir).unwrap();
        let meta = |name: &str, finished: &str| {
            fs::write(dir.join(format!("{name}.jsonl")), "{}\n").unwrap();
            fs::write(
                dir.join(format!("{name}.meta.json")),
                format!(r#"{{"agentType":"Explore","description":"Look around"{finished}}}"#),
            )
            .unwrap();
        };
        meta("agent-a", "");
        meta("agent-b", r#","finished":true"#);
        meta("agent-c", r#","finished":false"#);

        let tasks = agents(&transcript);
        let mut ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["agent-a", "agent-c"]);
        assert_eq!(tasks[0].agent_type, "Explore");
        assert_eq!(tasks[0].description, "Look around");
        let _ = fs::remove_dir_all(&root);
    }
}
