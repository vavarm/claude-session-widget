use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{limit, settings};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub used_percentage: f64,
    pub resets_at: i64,
}

// as_of is when the figures were read, in milliseconds; it is absent when they are known to be current.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Quotas {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    pub as_of: Option<i64>,
}

// Figures older than this are shown with the time they were read.
pub const STALE_MS: i64 = 15 * 60 * 1000;

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
        as_of: None,
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

fn millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn from_status_line() -> Option<Quotas> {
    let file = usage_file();
    let mut quotas: Quotas = serde_json::from_str(&fs::read_to_string(&file).ok()?).ok()?;
    quotas.as_of = Some(millis(fs::metadata(&file).ok()?.modified().ok()?));
    Some(quotas)
}

// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// Seconds since the epoch of an ISO 8601 timestamp such as 2026-11-08T16:00:00.058612+00:00.
fn parse_iso(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once('T')?;
    let mut date = date.split('-').map(|n| n.parse::<i64>().ok());
    let (year, month, day) = (date.next()??, date.next()??, date.next()??);

    let zone_at = rest.find(['+', '-', 'Z']).unwrap_or(rest.len());
    let (time, zone) = rest.split_at(zone_at);
    let mut time = time.split(':');
    let hours: i64 = time.next()?.parse().ok()?;
    let minutes: i64 = time.next()?.parse().ok()?;
    let seconds = time.next().unwrap_or("0").parse::<f64>().ok()? as i64;

    let offset = match zone.as_bytes().first() {
        Some(b'+') | Some(b'-') => {
            let (h, m) = zone[1..].split_once(':')?;
            let magnitude = h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60;
            if zone.starts_with('-') {
                -magnitude
            } else {
                magnitude
            }
        }
        _ => 0,
    };
    Some(
        days_from_civil(year, month, day) * 86_400 + hours * 3600 + minutes * 60 + seconds - offset,
    )
}

fn cached_window(utilization: &Value, key: &str) -> Option<Window> {
    let entry = utilization.get(key)?;
    Some(Window {
        used_percentage: entry.get("utilization")?.as_f64()?,
        resets_at: parse_iso(entry.get("resets_at")?.as_str()?)?,
    })
}

// Claude Code keeps the last usage it fetched in ~/.claude.json. It is refreshed only now and then.
fn from_claude_json_text(text: &str) -> Option<Quotas> {
    let root: Value = serde_json::from_str(text).ok()?;
    let cache = root.get("cachedUsageUtilization")?;
    let utilization = cache.get("utilization")?;
    let quotas = Quotas {
        five_hour: cached_window(utilization, "five_hour"),
        seven_day: cached_window(utilization, "seven_day"),
        as_of: cache.get("fetchedAtMs").and_then(Value::as_i64),
    };
    (!quotas.is_empty()).then_some(quotas)
}

fn claude_json_file() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir).join(".claude.json"),
        None => dirs::home_dir().unwrap_or_default().join(".claude.json"),
    }
}

// The file is large and read every second, so it is only parsed again when it changes.
fn from_claude_json() -> Option<Quotas> {
    static CACHE: Mutex<Option<(PathBuf, SystemTime, Option<Quotas>)>> = Mutex::new(None);
    let file = claude_json_file();
    let modified = fs::metadata(&file).ok()?.modified().ok()?;
    let mut cache = CACHE.lock().unwrap();
    if let Some((path, at, quotas)) = cache.as_ref() {
        if *path == file && *at == modified {
            return quotas.clone();
        }
    }
    let quotas = fs::read_to_string(&file)
        .ok()
        .and_then(|t| from_claude_json_text(&t));
    *cache = Some((file, modified, quotas.clone()));
    quotas
}

// For each window, the newest source that still describes a running window wins. A limit that Claude
// Code reported as exceeded counts as fully used until it resets.
fn merge(sources: Vec<Quotas>, rejections: &[limit::Rejection], now: i64) -> Quotas {
    let sources: Vec<Quotas> = sources
        .into_iter()
        .map(|q| q.without_expired(now))
        .collect();
    let newest = |pick: fn(&Quotas) -> &Option<Window>| {
        sources
            .iter()
            .filter(|q| pick(q).is_some())
            .max_by_key(|q| q.as_of.unwrap_or(i64::MAX))
            .map(|q| (pick(q).clone(), q.as_of))
    };
    let rejected = |name: &str| {
        rejections
            .iter()
            .find(|r| r.window == name)
            .map(|r| Window {
                used_percentage: 100.0,
                resets_at: r.resets_at,
            })
    };

    let mut as_of: Vec<i64> = Vec::new();
    let mut resolve = |picked: Option<(Option<Window>, Option<i64>)>, name: &str| {
        if let Some(window) = rejected(name) {
            return Some(window);
        }
        let (window, taken) = picked?;
        as_of.extend(taken);
        window
    };
    let five_hour = resolve(newest(|q| &q.five_hour), "five_hour");
    let seven_day = resolve(newest(|q| &q.seven_day), "seven_day");
    Quotas {
        five_hour,
        seven_day,
        as_of: as_of.into_iter().min(),
    }
}

pub fn load(now: i64) -> Quotas {
    let sources = [from_status_line(), from_claude_json()]
        .into_iter()
        .flatten()
        .collect();
    merge(sources, &limit::active(now), now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(percent: f64, resets_at: i64) -> Option<Window> {
        Some(Window {
            used_percentage: percent,
            resets_at,
        })
    }

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
    fn iso_timestamps_become_epoch_seconds() {
        assert_eq!(
            parse_iso("2026-11-08T16:00:00.058612+00:00"),
            Some(1794153600)
        );
        assert_eq!(parse_iso("2026-11-08T16:00:00Z"), Some(1794153600));
        assert_eq!(parse_iso("2026-11-08T17:00:00+01:00"), Some(1794153600));
        assert_eq!(parse_iso("1970-01-01T00:00:00+00:00"), Some(0));
        assert_eq!(parse_iso("nonsense"), None);
    }

    #[test]
    fn reads_the_usage_cache_of_claude_code() {
        let text = r#"{"cachedUsageUtilization":{"fetchedAtMs":1000,"utilization":{
            "five_hour":{"utilization":65,"resets_at":"2026-11-08T16:00:00.058612+00:00"},
            "seven_day":{"utilization":48,"resets_at":"2026-11-14T09:00:00+00:00"},
            "seven_day_opus":null}}}"#;
        let quotas = from_claude_json_text(text).unwrap();
        assert_eq!(quotas.five_hour, win(65.0, 1794153600));
        assert_eq!(quotas.as_of, Some(1000));
        assert!(from_claude_json_text("{}").is_none());
    }

    #[test]
    fn expired_windows_are_dropped() {
        let quotas = Quotas {
            five_hour: win(99.0, 100),
            seven_day: win(10.0, 500),
            as_of: None,
        };
        let live = quotas.without_expired(200);
        assert!(live.five_hour.is_none());
        assert!(live.seven_day.is_some());
    }

    fn source(five: Option<Window>, seven: Option<Window>, as_of: i64) -> Quotas {
        Quotas {
            five_hour: five,
            seven_day: seven,
            as_of: Some(as_of),
        }
    }

    #[test]
    fn the_newest_source_wins_per_window() {
        let status_line = source(win(10.0, 9000), None, 2000);
        let cache = source(win(80.0, 9000), win(48.0, 9000), 1000);
        let merged = merge(vec![status_line, cache], &[], 100);
        assert_eq!(merged.five_hour, win(10.0, 9000));
        assert_eq!(merged.seven_day, win(48.0, 9000));
        assert_eq!(merged.as_of, Some(1000));
    }

    #[test]
    fn an_expired_newer_source_does_not_hide_a_running_older_one() {
        let newer = source(win(10.0, 50), None, 2000);
        let older = source(win(80.0, 9000), None, 1000);
        assert_eq!(
            merge(vec![newer, older], &[], 100).five_hour,
            win(80.0, 9000)
        );
    }

    #[test]
    fn a_reported_rejection_counts_as_fully_used_and_is_current() {
        let cache = source(win(65.0, 9000), win(48.0, 9000), 1000);
        let rejected = [limit::Rejection {
            window: "five_hour".into(),
            resets_at: 7000,
        }];
        let merged = merge(vec![cache], &rejected, 100);
        assert_eq!(merged.five_hour, win(100.0, 7000));
        assert_eq!(merged.seven_day, win(48.0, 9000));
        assert_eq!(merged.as_of, Some(1000));

        let only = merge(vec![], &rejected, 100);
        assert_eq!(only.five_hour, win(100.0, 7000));
        assert_eq!(only.as_of, None);
    }

    #[test]
    fn nothing_known_gives_an_empty_result() {
        assert!(merge(vec![], &[], 100).is_empty());
    }
}
