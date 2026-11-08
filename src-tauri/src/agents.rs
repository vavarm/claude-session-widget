use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::PathBuf};

use crate::settings;

// Claude Code only reports tasks while it refreshes its agent panel; a file older than this describes nothing live.
const FRESH_MS: i64 = 30_000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub agent_type: String,
    pub status: String,
    pub description: String,
    pub start_time: i64,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Record {
    updated_at: i64,
    tasks: Vec<Task>,
}

fn dir() -> PathBuf {
    settings::data_dir().join("agents")
}

fn text(task: &Value, key: &str) -> String {
    task.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

// Parses the input of a subagent status line command: the session id and its tasks.
pub fn parse_input(input: &str) -> Option<(String, Vec<Task>)> {
    let value: Value = serde_json::from_str(input).ok()?;
    let session_id = value.get("session_id")?.as_str()?.to_string();
    let tasks = value
        .get("tasks")?
        .as_array()?
        .iter()
        .map(|task| Task {
            id: text(task, "id"),
            name: text(task, "name"),
            kind: text(task, "type"),
            agent_type: text(task, "agentType"),
            status: text(task, "status"),
            description: text(task, "description"),
            start_time: task.get("startTime").and_then(Value::as_i64).unwrap_or(0),
        })
        .collect();
    Some((session_id, tasks))
}

// Session ids become file names, so anything but a plain identifier is refused.
fn file_for(session_id: &str) -> Option<PathBuf> {
    let plain = !session_id.is_empty()
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    plain.then(|| dir().join(format!("{session_id}.json")))
}

pub fn save(session_id: &str, tasks: Vec<Task>, now: i64) {
    let Some(file) = file_for(session_id) else {
        return;
    };
    let record = Record {
        updated_at: now,
        tasks,
    };
    if let Ok(text) = serde_json::to_string(&record) {
        settings::write_atomically(&file, &text);
    }
}

// Running tasks of a session, oldest first.
pub fn load(session_id: &str, now: i64) -> Vec<Task> {
    let record: Record = file_for(session_id)
        .and_then(|file| fs::read_to_string(file).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    if now - record.updated_at > FRESH_MS {
        return Vec::new();
    }
    let mut tasks: Vec<Task> = record
        .tasks
        .into_iter()
        .filter(|t| t.status == "running")
        .collect();
    tasks.sort_by_key(|t| t.start_time);
    tasks
}

#[cfg(test)]
mod tests {
    use super::*;

    const INPUT: &str = r#"{"session_id":"abc-123","columns":80,"tasks":[
        {"id":"t1","name":"scout","type":"local_agent","agentType":"Explore","status":"running","description":"Find the auth code","startTime":2000,"tokenCount":10},
        {"id":"t2","type":"local_agent","status":"completed","description":"Old one","startTime":1000},
        {"id":"t3","type":"local_agent","status":"running","description":"First","startTime":1500}]}"#;

    #[test]
    fn parses_session_id_and_tasks_with_missing_fields() {
        let (id, tasks) = parse_input(INPUT).unwrap();
        assert_eq!(id, "abc-123");
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].agent_type, "Explore");
        assert_eq!(tasks[1].name, "");
    }

    #[test]
    fn rejects_input_without_a_session_or_tasks() {
        assert!(parse_input(r#"{"tasks":[]}"#).is_none());
        assert!(parse_input(r#"{"session_id":"a"}"#).is_none());
        assert!(parse_input("nope").is_none());
    }

    #[test]
    fn only_plain_identifiers_become_file_names() {
        assert!(file_for("d12a0cea-646a-4207-8cda-327343d23ed7").is_some());
        assert!(file_for("").is_none());
        assert!(file_for("../escape").is_none());
        assert!(file_for("a\\b").is_none());
    }

    #[test]
    fn load_returns_running_tasks_oldest_first_and_ignores_stale_files() {
        let dir = std::env::temp_dir().join(format!("csw-agents-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        std::env::set_var("CLAUDE_WIDGET_DATA_DIR", &dir);

        let (id, tasks) = parse_input(INPUT).unwrap();
        save(&id, tasks, 100_000);
        let running = load(&id, 105_000);
        assert_eq!(
            running.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["t3", "t1"]
        );
        assert!(load(&id, 100_000 + FRESH_MS + 1).is_empty());
        assert!(load("unknown", 105_000).is_empty());

        std::env::remove_var("CLAUDE_WIDGET_DATA_DIR");
        let _ = fs::remove_dir_all(&dir);
    }
}
