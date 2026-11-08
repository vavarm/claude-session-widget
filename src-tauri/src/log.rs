use crate::{sessions::Session, settings};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

// The widget and the commands Claude Code runs for it all append to the same file, one JSON object per line.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

// Inverse of days_from_civil: the calendar date of a day count since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

pub fn iso(ms: i64) -> String {
    let (days, rest) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1000 % 60,
        rest % 1000
    )
}

fn append(path: &Path, line: &str) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        // One write per line keeps lines from different processes from interleaving.
        let _ = file.write_all(format!("{line}\n").as_bytes());
    }
}

pub fn file() -> std::path::PathBuf {
    settings::data_dir().join("widget.log")
}

pub fn event(kind: &str, data: Value) {
    let ms = now_ms();
    let line =
        json!({ "ts": ms, "t": iso(ms), "pid": std::process::id(), "event": kind, "data": data });
    append(&file(), &line.to_string());
}

#[derive(PartialEq, Clone)]
struct Seen {
    name: String,
    kind: String,
    status: String,
    model: String,
    context: Option<i64>,
    cost_cents: Option<i64>,
    agents: Vec<String>,
}

fn summarize(session: &Session) -> Seen {
    let info = session.info.as_ref();
    Seen {
        name: session.name.clone(),
        kind: serde_json::to_value(session.kind)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default(),
        status: session.status.clone(),
        model: info.map(|i| i.model.clone()).unwrap_or_default(),
        context: info
            .and_then(|i| i.context_percent)
            .map(|p| p.round() as i64),
        cost_cents: info
            .and_then(|i| i.cost_usd)
            .map(|c| (c * 100.0).round() as i64),
        agents: session
            .agents
            .iter()
            .map(|a| format!("{}:{}:{}", a.kind, a.agent_type, a.description))
            .collect(),
    }
}

// Turns successive looks at the sessions into the changes worth recording.
#[derive(Default)]
pub struct Tracker {
    sessions: Option<HashMap<u32, Seen>>,
}

impl Tracker {
    pub fn sessions(&mut self, list: &[Session]) -> Vec<(&'static str, Value)> {
        let current: HashMap<u32, Seen> = list.iter().map(|s| (s.pid, summarize(s))).collect();
        let first = self.sessions.is_none();
        let previous = self.sessions.take().unwrap_or_default();
        let mut events = Vec::new();

        for session in list {
            let now = &current[&session.pid];
            let Some(before) = previous.get(&session.pid) else {
                let kind = if first {
                    "session_seen"
                } else {
                    "session_start"
                };
                events.push((
                    kind,
                    json!({ "pid": session.pid, "sessionId": session.session_id, "name": now.name,
                            "cwd": session.cwd, "kind": now.kind, "status": now.status }),
                ));
                continue;
            };
            if (before.kind.as_str(), before.status.as_str())
                != (now.kind.as_str(), now.status.as_str())
                || before.name != now.name
            {
                events.push((
                    "session_state",
                    json!({ "pid": session.pid, "name": now.name,
                            "from": { "kind": before.kind, "status": before.status },
                            "to": { "kind": now.kind, "status": now.status } }),
                ));
            }
            if (&before.model, before.context, before.cost_cents)
                != (&now.model, now.context, now.cost_cents)
            {
                events.push((
                    "session_info",
                    json!({ "pid": session.pid, "name": now.name, "model": now.model,
                            "contextPercent": now.context, "costUsd": now.cost_cents.map(|c| c as f64 / 100.0) }),
                ));
            }
            if before.agents != now.agents {
                events.push((
                    "session_agents",
                    json!({ "pid": session.pid, "name": now.name, "running": now.agents }),
                ));
            }
        }
        for (pid, before) in &previous {
            if !current.contains_key(pid) {
                events.push((
                    "session_end",
                    json!({ "pid": pid, "name": before.name, "last": before.kind }),
                ));
            }
        }
        self.sessions = Some(current);
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{info::Info, sessions::Kind};

    fn session(pid: u32, kind: Kind, status: &str) -> Session {
        Session {
            pid,
            session_id: format!("s{pid}"),
            name: format!("n{pid}"),
            cwd: "C:/w".into(),
            status: status.into(),
            kind,
            since_ms: 0,
            agents: Vec::new(),
            info: None,
        }
    }

    fn names(events: &[(&'static str, Value)]) -> Vec<&'static str> {
        events.iter().map(|(name, _)| *name).collect()
    }

    #[test]
    fn timestamps_are_iso_utc() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_794_153_600_123), "2026-11-08T16:00:00.123Z");
        assert_eq!(iso(951_782_400_000), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn first_look_reports_sessions_as_seen_then_only_changes() {
        let mut tracker = Tracker::default();
        let list = [session(1, Kind::Busy, "busy")];
        assert_eq!(names(&tracker.sessions(&list)), ["session_seen"]);
        assert!(tracker.sessions(&list).is_empty());

        let changed = [
            session(1, Kind::Idle, "idle"),
            session(2, Kind::Busy, "busy"),
        ];
        assert_eq!(
            names(&tracker.sessions(&changed)),
            ["session_state", "session_start"]
        );
        assert_eq!(
            names(&tracker.sessions(&[])),
            ["session_end", "session_end"]
        );
    }

    #[test]
    fn raw_status_values_are_recorded() {
        let mut tracker = Tracker::default();
        tracker.sessions(&[session(1, Kind::Busy, "busy")]);
        let events = tracker.sessions(&[session(1, Kind::Unknown, "waiting_for_permission")]);
        assert_eq!(events[0].1["to"]["status"], "waiting_for_permission");
    }

    #[test]
    fn info_and_agent_changes_are_recorded() {
        let mut tracker = Tracker::default();
        let mut s = session(1, Kind::Busy, "busy");
        tracker.sessions(&[s.clone()]);
        s.info = Some(Info {
            model: "Opus".into(),
            context_percent: Some(41.6),
            cost_usd: Some(1.234),
        });
        let events = tracker.sessions(&[s.clone()]);
        assert_eq!(names(&events), ["session_info"]);
        assert_eq!(events[0].1["contextPercent"], 42);
        assert!(tracker.sessions(&[s.clone()]).is_empty());

        s.agents.push(crate::agents::Task {
            id: "a".into(),
            name: String::new(),
            kind: "local_agent".into(),
            agent_type: "Explore".into(),
            status: "running".into(),
            description: "Find".into(),
            start_time: 0,
        });
        assert_eq!(names(&tracker.sessions(&[s])), ["session_agents"]);
    }

    #[test]
    fn a_full_file_is_rotated_once() {
        let dir = std::env::temp_dir().join(format!("csw-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("widget.log");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        append(&path, "fresh");
        assert_eq!(fs::read_to_string(&path).unwrap(), "fresh\n");
        assert!(dir.join("widget.log.1").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
