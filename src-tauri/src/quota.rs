use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::PathBuf};

use crate::settings;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub used_percentage: f64,
    pub resets_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Quotas {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
}

impl Quotas {
    pub fn is_empty(&self) -> bool {
        self.five_hour.is_none() && self.seven_day.is_none()
    }

    // A window whose reset time has passed describes usage that no longer counts.
    fn without_expired(mut self, now: i64) -> Self {
        let live = |w: Option<Window>| w.filter(|w| w.resets_at > now);
        self.five_hour = live(self.five_hour);
        self.seven_day = live(self.seven_day);
        self
    }
}

fn window(rate_limits: &Value, key: &str) -> Option<Window> {
    let entry = rate_limits.get(key)?;
    Some(Window {
        used_percentage: entry.get("used_percentage")?.as_f64()?,
        resets_at: entry.get("resets_at")?.as_i64()?,
    })
}

// Parses the JSON that Claude Code pipes to a status line command.
pub fn parse_status(input: &str) -> Option<Quotas> {
    let value: Value = serde_json::from_str(input).ok()?;
    let limits = value.get("rate_limits")?;
    let quotas = Quotas {
        five_hour: window(limits, "five_hour"),
        seven_day: window(limits, "seven_day"),
    };
    (!quotas.is_empty()).then_some(quotas)
}

fn usage_file() -> PathBuf {
    settings::data_dir().join("usage.json")
}

pub fn save(quotas: &Quotas) {
    if let Ok(text) = serde_json::to_string(quotas) {
        settings::write_atomically(&usage_file(), &text);
    }
}

pub fn load(now: i64) -> Quotas {
    fs::read_to_string(usage_file())
        .ok()
        .and_then(|text| serde_json::from_str::<Quotas>(&text).ok())
        .unwrap_or_default()
        .without_expired(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rate_limits_from_status_input() {
        let input = r#"{"model":"x","rate_limits":{"five_hour":{"used_percentage":23.5,"resets_at":1738425600},"seven_day":{"used_percentage":41.2,"resets_at":1738857600}}}"#;
        let quotas = parse_status(input).unwrap();
        assert_eq!(quotas.five_hour.unwrap().used_percentage, 23.5);
        assert_eq!(quotas.seven_day.unwrap().resets_at, 1738857600);
    }

    #[test]
    fn input_without_rate_limits_yields_nothing() {
        assert!(parse_status(r#"{"model":"x"}"#).is_none());
        assert!(parse_status(r#"{"rate_limits":{}}"#).is_none());
        assert!(parse_status("not json").is_none());
    }

    #[test]
    fn a_single_window_is_enough() {
        let input = r#"{"rate_limits":{"seven_day":{"used_percentage":5,"resets_at":10}}}"#;
        let quotas = parse_status(input).unwrap();
        assert!(quotas.five_hour.is_none());
        assert!(quotas.seven_day.is_some());
    }

    #[test]
    fn expired_windows_are_dropped() {
        let quotas = Quotas {
            five_hour: Some(Window {
                used_percentage: 99.0,
                resets_at: 100,
            }),
            seven_day: Some(Window {
                used_percentage: 10.0,
                resets_at: 500,
            }),
        };
        let live = quotas.without_expired(200);
        assert!(live.five_hour.is_none());
        assert!(live.seven_day.is_some());
    }
}
