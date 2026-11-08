use crate::sessions::{Kind, Session};
use std::collections::HashMap;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

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
}
