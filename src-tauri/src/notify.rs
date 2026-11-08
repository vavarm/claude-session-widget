use crate::{
    quota::{Quotas, Window, STALE_MS},
    sessions::{Kind, Session},
};
use std::collections::{HashMap, HashSet};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

pub const LIMIT_PERCENT: f64 = 90.0;

#[derive(Debug, PartialEq)]
pub enum Reason {
    NeedsYou,
    Finished,
}

// Remembers what was last seen, so only changes produce events. It keeps watching while
// notifications are off: turning them on later must not replay the current state.
#[derive(Default)]
pub struct Watcher {
    previous: Option<HashMap<u32, Kind>>,
    alerted: HashSet<(&'static str, i64)>,
}

impl Watcher {
    pub fn sessions(&mut self, sessions: &[Session]) -> Vec<(Reason, String)> {
        let current: HashMap<u32, Kind> = sessions.iter().map(|s| (s.pid, s.kind)).collect();
        let mut events = Vec::new();
        if let Some(previous) = &self.previous {
            for session in sessions {
                match (previous.get(&session.pid), session.kind) {
                    (Some(Kind::Attention), _) => {}
                    (_, Kind::Attention) => events.push((Reason::NeedsYou, session.name.clone())),
                    (Some(Kind::Busy), Kind::Idle) => {
                        events.push((Reason::Finished, session.name.clone()))
                    }
                    _ => {}
                }
            }
        }
        self.previous = Some(current);
        events
    }

    // One message per window and per reset period.
    pub fn quotas(&mut self, quotas: &Quotas, now: i64) -> Vec<String> {
        let mut messages = Vec::new();
        if quotas.as_of.is_some_and(|at| now * 1000 - at > STALE_MS) {
            return messages;
        }
        for (name, window) in [("5h", &quotas.five_hour), ("7d", &quotas.seven_day)] {
            let Some(Window {
                used_percentage,
                resets_at,
            }) = window
            else {
                continue;
            };
            if *used_percentage >= LIMIT_PERCENT && self.alerted.insert((name, *resets_at)) {
                messages.push(format!(
                    "{name} usage at {} %, resets in {}",
                    used_percentage.round(),
                    until(*resets_at - now)
                ));
            }
        }
        messages
    }
}

fn until(seconds: i64) -> String {
    let minutes = ((seconds + 59) / 60).max(1);
    match minutes {
        m if m < 60 => format!("{m}m"),
        m if m < 24 * 60 => format!("{}h{:02}", m / 60, m % 60),
        m => format!("{}d{:02}h", m / 1440, m % 1440 / 60),
    }
}

pub fn show(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(pid: u32, name: &str, kind: Kind) -> Session {
        Session {
            pid,
            session_id: String::new(),
            name: name.into(),
            cwd: String::new(),
            status: String::new(),
            kind,
            since_ms: 0,
            agents: Vec::new(),
            info: None,
        }
    }

    #[test]
    fn the_first_look_never_notifies() {
        let mut watcher = Watcher::default();
        assert!(watcher
            .sessions(&[session(1, "a", Kind::Attention)])
            .is_empty());
    }

    #[test]
    fn a_session_that_starts_needing_you_notifies_once() {
        let mut watcher = Watcher::default();
        watcher.sessions(&[session(1, "a", Kind::Busy)]);
        let events = watcher.sessions(&[session(1, "a", Kind::Attention)]);
        assert_eq!(events, vec![(Reason::NeedsYou, "a".to_string())]);
        assert!(watcher
            .sessions(&[session(1, "a", Kind::Attention)])
            .is_empty());
    }

    #[test]
    fn busy_to_idle_means_finished_but_idle_to_idle_does_not() {
        let mut watcher = Watcher::default();
        watcher.sessions(&[session(1, "a", Kind::Busy), session(2, "b", Kind::Idle)]);
        let events = watcher.sessions(&[session(1, "a", Kind::Idle), session(2, "b", Kind::Idle)]);
        assert_eq!(events, vec![(Reason::Finished, "a".to_string())]);
    }

    #[test]
    fn answering_a_prompt_is_not_a_finish() {
        let mut watcher = Watcher::default();
        watcher.sessions(&[session(1, "a", Kind::Attention)]);
        assert!(watcher.sessions(&[session(1, "a", Kind::Idle)]).is_empty());
    }

    #[test]
    fn a_new_session_that_needs_you_notifies() {
        let mut watcher = Watcher::default();
        watcher.sessions(&[]);
        let events = watcher.sessions(&[session(7, "new", Kind::Attention)]);
        assert_eq!(events, vec![(Reason::NeedsYou, "new".to_string())]);
    }

    fn quotas(five: f64, seven: f64, reset: i64) -> Quotas {
        let window = |used_percentage| {
            Some(Window {
                used_percentage,
                resets_at: reset,
            })
        };
        Quotas {
            five_hour: window(five),
            seven_day: window(seven),
            as_of: None,
        }
    }

    #[test]
    fn quota_alert_fires_once_per_reset_period() {
        let mut watcher = Watcher::default();
        assert!(watcher.quotas(&quotas(50.0, 10.0, 5000), 1000).is_empty());
        let first = watcher.quotas(&quotas(92.4, 10.0, 5000), 1000);
        assert_eq!(first, vec!["5h usage at 92 %, resets in 1h07".to_string()]);
        assert!(watcher.quotas(&quotas(95.0, 10.0, 5000), 1100).is_empty());
        assert_eq!(watcher.quotas(&quotas(91.0, 10.0, 9000), 6000).len(), 1);
    }

    #[test]
    fn stale_figures_never_raise_an_alert() {
        let mut watcher = Watcher::default();
        let mut stale = quotas(95.0, 10.0, 5000);
        stale.as_of = Some(0);
        assert!(watcher.quotas(&stale, 1000).is_empty());
        assert_eq!(watcher.quotas(&quotas(95.0, 10.0, 5000), 1000).len(), 1);
    }

    #[test]
    fn until_formats_minutes_hours_and_days() {
        assert_eq!(until(10), "1m");
        assert_eq!(until(25 * 60), "25m");
        assert_eq!(until(2 * 3600 + 5 * 60), "2h05");
        assert_eq!(until(50 * 3600), "2d02h");
    }
}
