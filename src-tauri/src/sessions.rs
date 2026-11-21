use crate::transcript::{Activity, Info, Task};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
use sysinfo::{Pid, ProcessesToUpdate, System};

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Attention,
    Busy,
    // Claude Code reports the turn over, but tasks or subagents it started are still running.
    Background,
    // The turn ended on a usage limit: nothing moves until it resets.
    Limited,
    Idle,
    Unknown,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub pid: u32,
    #[serde(skip)]
    pub session_id: String,
    pub name: String,
    pub cwd: String,
    pub status: String,
    pub kind: Kind,
    pub detail: String,
    pub since_ms: i64,
    #[serde(skip)]
    pub started_ms: i64,
    pub agents: Vec<Task>,
    pub info: Option<Info>,
}

pub fn dir() -> PathBuf {
    match std::env::var_os("CLAUDE_WIDGET_SESSIONS_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir()
            .unwrap_or_default()
            .join(".claude")
            .join("sessions"),
    }
}

pub fn classify(status: &str) -> Kind {
    let s = status.to_lowercase();
    match s.as_str() {
        "busy" => Kind::Busy,
        "idle" => Kind::Idle,
        _ if ["wait", "input", "permission", "approval"]
            .iter()
            .any(|k| s.contains(k)) =>
        {
            Kind::Attention
        }
        _ => Kind::Unknown,
    }
}

// Claude Code says "idle" once a turn is over, whether or not work goes on behind it.
pub fn refine(session: &mut Session, activity: &Activity, agents: usize) {
    if session.kind != Kind::Idle {
        return;
    }
    let plural = |n: usize, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
    if let Some(reset) = &activity.limit {
        session.kind = Kind::Limited;
        session.detail = reset.clone();
        return;
    }
    let tasks = activity.background_since(session.started_ms);
    let parts: Vec<String> = [(tasks, "task"), (agents, "agent")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| plural(n, what))
        .collect();
    if !parts.is_empty() {
        session.kind = Kind::Background;
        session.detail = parts.join(", ");
    }
}

pub fn rank(kind: Kind) -> u8 {
    match kind {
        Kind::Attention => 0,
        Kind::Busy => 1,
        Kind::Background => 2,
        _ => 3,
    }
}

pub fn sort(sessions: &mut [Session]) {
    sessions.sort_by_cached_key(|s| (rank(s.kind), s.name.to_lowercase()));
}

pub fn read(dir: &Path) -> Vec<Session> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let Ok(mut system) = System::new() else {
        return Vec::new();
    };
    let mut sessions: Vec<Session> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| parse(&p, &mut system))
        .collect();
    sort(&mut sessions);
    sessions
}

fn parse(path: &Path, system: &mut System) -> Option<Session> {
    let json: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    let pid = u32::try_from(json.get("pid")?.as_u64()?).ok()?;
    if !is_alive(system, pid, &json) {
        return None;
    }

    let text = |key: &str| {
        json.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let cwd = text("cwd");
    let mut name = text("name");
    if name.is_empty() {
        name = cwd
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .to_string();
    }
    let status = text("status");
    Some(Session {
        pid,
        session_id: text("sessionId"),
        name,
        cwd,
        kind: classify(&status),
        status,
        detail: String::new(),
        started_ms: json.get("startedAt").and_then(Value::as_i64).unwrap_or(0),
        since_ms: json
            .get("statusUpdatedAt")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        agents: Vec::new(),
        info: None,
    })
}

// Session files outlive crashed processes, so a file only counts if its pid is still the same process.
fn is_alive(system: &mut System, pid: u32, json: &Value) -> bool {
    let pid = Pid::from_u32(pid);
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    let Some(process) = system.process(pid) else {
        return false;
    };

    // procStart is a Windows FILETIME; its encoding on other platforms is not known, so only the pid is checked there.
    #[cfg(windows)]
    {
        let domain = json
            .get("pidDomain")
            .and_then(Value::as_str)
            .unwrap_or("win32");
        let proc_start = json
            .get("procStart")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok());
        if let (true, Some(filetime)) = (domain.starts_with("win32"), proc_start) {
            const UNIX_EPOCH_AS_FILETIME_SECS: u64 = 11_644_473_600;
            let expected = (filetime / 10_000_000).saturating_sub(UNIX_EPOCH_AS_FILETIME_SECS);
            return process.start_time().abs_diff(expected) <= 5;
        }
    }
    let _ = (process, json);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::{Child, Command, Stdio},
        sync::atomic::{AtomicU32, Ordering},
    };

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "sw-test-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&dir).unwrap();
            Fixture { dir }
        }

        fn write(&self, file: &str, content: &str) {
            fs::write(self.dir.join(file), content).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn sleeper() -> Child {
        #[cfg(windows)]
        let mut cmd = Command::new("powershell");
        #[cfg(windows)]
        cmd.args(["-NoProfile", "-Command", "Start-Sleep 60"]);
        #[cfg(not(windows))]
        let mut cmd = Command::new("sleep");
        #[cfg(not(windows))]
        cmd.arg("60");
        cmd.stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    #[cfg(windows)]
    fn proc_start(pid: u32) -> u64 {
        let mut system = System::new().unwrap();
        let p = Pid::from_u32(pid);
        system.refresh_processes(ProcessesToUpdate::Some(&[p]), true);
        (system.process(p).unwrap().start_time() + 11_644_473_600) * 10_000_000
    }

    #[cfg(not(windows))]
    fn proc_start(_pid: u32) -> u64 {
        0
    }

    fn session_json(pid: u32, proc_start: u64, status: &str, name: &str, cwd: &str) -> String {
        serde_json::json!({
            "pid": pid, "procStart": proc_start.to_string(), "cwd": cwd,
            "name": name, "status": status, "statusUpdatedAt": 1794142253018_i64,
        })
        .to_string()
    }

    fn kill(mut child: Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn reads_a_live_session() {
        let fx = Fixture::new();
        let child = sleeper();
        std::thread::sleep(std::time::Duration::from_millis(500));
        fx.write(
            "a.json",
            &session_json(
                child.id(),
                proc_start(child.id()),
                "busy",
                "my-session",
                "/work/proj",
            ),
        );

        let sessions = read(&fx.dir);
        kill(child);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "my-session");
        assert_eq!(sessions[0].kind, Kind::Busy);
        assert_eq!(sessions[0].since_ms, 1794142253018);
    }

    #[test]
    fn skips_a_dead_pid() {
        let fx = Fixture::new();
        fx.write(
            "a.json",
            &session_json(u32::MAX - 1, 1, "idle", "ghost", "/x"),
        );
        assert!(read(&fx.dir).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn skips_a_recycled_pid() {
        let fx = Fixture::new();
        let child = sleeper();
        std::thread::sleep(std::time::Duration::from_millis(500));
        fx.write(
            "a.json",
            &session_json(
                child.id(),
                proc_start(child.id()) - 10_000_000_000,
                "idle",
                "old",
                "/x",
            ),
        );

        let sessions = read(&fx.dir);
        kill(child);
        assert!(sessions.is_empty());
    }

    #[test]
    fn ignores_malformed_files() {
        let fx = Fixture::new();
        fx.write("bad.json", "{not json");
        fx.write("empty.json", "{}");
        fx.write("notes.txt", "ignored");
        let child = sleeper();
        std::thread::sleep(std::time::Duration::from_millis(500));
        fx.write(
            "ok.json",
            &session_json(child.id(), proc_start(child.id()), "idle", "ok", "/x"),
        );

        let sessions = read(&fx.dir);
        kill(child);
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn falls_back_to_the_folder_name() {
        let fx = Fixture::new();
        let child = sleeper();
        std::thread::sleep(std::time::Duration::from_millis(500));
        fx.write(
            "a.json",
            &session_json(
                child.id(),
                proc_start(child.id()),
                "idle",
                "",
                "C:\\work\\my-repo\\",
            ),
        );

        let sessions = read(&fx.dir);
        kill(child);
        assert_eq!(sessions[0].name, "my-repo");
    }

    #[test]
    fn sorts_attention_first_then_busy_then_the_rest() {
        let fx = Fixture::new();
        let children: Vec<Child> = (0..3).map(|_| sleeper()).collect();
        std::thread::sleep(std::time::Duration::from_millis(500));
        for (child, (status, name)) in
            children
                .iter()
                .zip([("idle", "a"), ("busy", "b"), ("waiting_for_input", "c")])
        {
            fx.write(
                &format!("{name}.json"),
                &session_json(child.id(), proc_start(child.id()), status, name, "/x"),
            );
        }

        let names: Vec<String> = read(&fx.dir).into_iter().map(|s| s.name).collect();
        children.into_iter().for_each(kill);
        assert_eq!(names, ["c", "b", "a"]);
    }

    #[test]
    fn classifies_statuses() {
        assert_eq!(classify("busy"), Kind::Busy);
        assert_eq!(classify("IDLE"), Kind::Idle);
        assert_eq!(classify("waiting_for_input"), Kind::Attention);
        assert_eq!(classify("needs_permission"), Kind::Attention);
        assert_eq!(classify("whatever"), Kind::Unknown);
    }

    fn idle(started_ms: i64) -> Session {
        Session {
            pid: 1,
            session_id: String::new(),
            name: "s".into(),
            cwd: String::new(),
            status: "idle".into(),
            kind: Kind::Idle,
            detail: String::new(),
            since_ms: 0,
            started_ms,
            agents: Vec::new(),
            info: None,
        }
    }

    #[test]
    fn an_idle_session_with_work_left_is_in_the_background() {
        let activity = Activity {
            background: vec![("a".into(), 100), ("b".into(), 200), ("old".into(), 10)],
            limit: None,
        };
        let mut session = idle(50);
        refine(&mut session, &activity, 1);
        assert_eq!(
            (session.kind, session.detail.as_str()),
            (Kind::Background, "2 tasks, 1 agent")
        );

        let mut session = idle(50);
        refine(&mut session, &Activity::default(), 0);
        assert_eq!((session.kind, session.detail.as_str()), (Kind::Idle, ""));
    }

    #[test]
    fn a_usage_limit_wins_over_background_work_and_never_overrides_a_live_status() {
        let activity = Activity {
            background: vec![("a".into(), 100)],
            limit: Some("resets 9:30pm (Europe/Paris)".into()),
        };
        let mut session = idle(0);
        refine(&mut session, &activity, 0);
        assert_eq!(
            (session.kind, session.detail.as_str()),
            (Kind::Limited, "resets 9:30pm (Europe/Paris)")
        );

        let mut busy = idle(0);
        busy.kind = Kind::Busy;
        refine(&mut busy, &activity, 2);
        assert_eq!(busy.kind, Kind::Busy);
    }

    #[test]
    fn background_sessions_sort_between_busy_and_idle_ones() {
        let mut sessions: Vec<Session> = [
            ("d", Kind::Idle),
            ("c", Kind::Background),
            ("b", Kind::Busy),
            ("a", Kind::Limited),
        ]
        .into_iter()
        .map(|(name, kind)| Session {
            name: name.into(),
            kind,
            ..idle(0)
        })
        .collect();
        sort(&mut sessions);
        let names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["b", "c", "a", "d"]);
    }

    #[test]
    fn a_missing_directory_yields_nothing() {
        assert!(read(Path::new("/definitely/not/here")).is_empty());
    }
}
