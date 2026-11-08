use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;

use crate::settings;

// What Claude Code reports about one session through its status line.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Info {
    pub model: String,
    pub context_percent: Option<f64>,
    pub cost_usd: Option<f64>,
}

fn dir() -> std::path::PathBuf {
    settings::data_dir().join("info")
}

pub fn parse_status(input: &str) -> Option<(String, Info)> {
    let value: Value = serde_json::from_str(input).ok()?;
    let session_id = value.get("session_id")?.as_str()?.to_string();
    let info = Info {
        model: value
            .pointer("/model/display_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        context_percent: value
            .pointer("/context_window/used_percentage")
            .and_then(Value::as_f64),
        cost_usd: value
            .pointer("/cost/total_cost_usd")
            .and_then(Value::as_f64),
    };
    (info != Info::default()).then_some((session_id, info))
}

pub fn save(session_id: &str, info: &Info) {
    if let (Some(file), Ok(text)) = (
        settings::session_file(&dir(), session_id),
        serde_json::to_string(info),
    ) {
        settings::write_atomically(&file, &text);
    }
}

pub fn load(session_id: &str) -> Option<Info> {
    let file = settings::session_file(&dir(), session_id)?;
    serde_json::from_str(&fs::read_to_string(file).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_model_context_and_cost() {
        let input = r#"{"session_id":"abc","model":{"display_name":"Opus"},"context_window":{"used_percentage":42.5},"cost":{"total_cost_usd":1.25}}"#;
        let (id, info) = parse_status(input).unwrap();
        assert_eq!(id, "abc");
        assert_eq!(info.model, "Opus");
        assert_eq!(info.context_percent, Some(42.5));
        assert_eq!(info.cost_usd, Some(1.25));
    }

    #[test]
    fn null_context_before_the_first_response_is_tolerated() {
        let input = r#"{"session_id":"abc","model":{"display_name":"Opus"},"context_window":{"used_percentage":null}}"#;
        let (_, info) = parse_status(input).unwrap();
        assert_eq!(info.context_percent, None);
    }

    #[test]
    fn nothing_useful_or_no_session_yields_nothing() {
        assert!(parse_status(r#"{"session_id":"abc"}"#).is_none());
        assert!(parse_status(r#"{"model":{"display_name":"Opus"}}"#).is_none());
        assert!(parse_status("nope").is_none());
    }
}
