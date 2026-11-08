use serde_json::Value;
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};

// A transcript only needs to be looked at while the window it reports can still be running.
const RECENT: Duration = Duration::from_secs(8 * 3600);
const TAIL_BYTES: u64 = 512 * 1024;
const RESCAN_AFTER: Duration = Duration::from_secs(20);
const MARKER: &str = "\"rate_limit_info\":";

#[derive(Debug, Clone, PartialEq)]
pub struct Rejection {
    pub window: String,
    pub resets_at: i64,
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

fn rejections_in(text: &str, now: i64) -> Vec<Rejection> {
    text.match_indices(MARKER)
        .filter_map(|(at, _)| {
            let rest = &text[at + MARKER.len()..];
            let info: Value = serde_json::Deserializer::from_str(rest)
                .into_iter()
                .next()?
                .ok()?;
            let resets_at = info.get("resetsAt")?.as_i64()?;
            let window = info.get("rateLimitType")?.as_str()?.to_string();
            let rejected = info.get("status")?.as_str()? == "rejected";
            (rejected && resets_at > now).then_some(Rejection { window, resets_at })
        })
        .collect()
}

fn scan(projects: &Path, now: i64) -> Vec<Rejection> {
    let mut found: Vec<Rejection> = Vec::new();
    let Ok(projects) = fs::read_dir(projects) else {
        return found;
    };
    let transcripts = projects
        .flatten()
        .filter_map(|project| fs::read_dir(project.path()).ok())
        .flat_map(|files| files.flatten().map(|f| f.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .filter(|p| {
            fs::metadata(p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .is_some_and(|age| age < RECENT)
        });
    for path in transcripts {
        for rejection in tail(&path)
            .map(|t| rejections_in(&t, now))
            .unwrap_or_default()
        {
            match found.iter_mut().find(|r| r.window == rejection.window) {
                Some(known) => known.resets_at = known.resets_at.max(rejection.resets_at),
                None => found.push(rejection),
            }
        }
    }
    found
}

fn projects_dir() -> PathBuf {
    let base = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".claude"));
    base.join("projects")
}

// Limits Claude Code reported as exceeded and that have not reset yet, found in recent transcripts.
// Scanning is throttled: the answer only changes when a limit is hit or resets.
pub fn active(now: i64) -> Vec<Rejection> {
    static CACHE: Mutex<Option<(Instant, Vec<Rejection>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap();
    if let Some((at, found)) = cache.as_ref() {
        if at.elapsed() < RESCAN_AFTER {
            return found
                .iter()
                .filter(|r| r.resets_at > now)
                .cloned()
                .collect();
        }
    }
    let found = scan(&projects_dir(), now);
    *cache = Some((Instant::now(), found.clone()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"type":"assistant","apiErrorParams":{"rate_limit_info":{"status":"rejected","resetsAt":5000,"rateLimitType":"five_hour","overageStatus":"rejected"}},"quotaLimits":{}}"#;

    #[test]
    fn finds_a_rejection_that_has_not_reset_yet() {
        assert_eq!(
            rejections_in(LINE, 1000),
            vec![Rejection {
                window: "five_hour".into(),
                resets_at: 5000
            }]
        );
    }

    #[test]
    fn ignores_resets_in_the_past_and_allowed_statuses() {
        assert!(rejections_in(LINE, 6000).is_empty());
        let allowed = LINE.replace("rejected\",\"resetsAt", "allowed\",\"resetsAt");
        assert!(rejections_in(&allowed, 1000).is_empty());
    }

    #[test]
    fn scans_recent_transcripts_and_keeps_the_latest_reset_per_window() {
        let root = std::env::temp_dir().join(format!("csw-limit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let project = root.join("some-project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("a.jsonl"), format!("{LINE}\n")).unwrap();
        fs::write(
            project.join("b.jsonl"),
            format!("{}\n", LINE.replace("5000", "7000")),
        )
        .unwrap();
        fs::write(project.join("notes.txt"), LINE).unwrap();

        let found = scan(&root, 1000);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].resets_at, 7000);
        let _ = fs::remove_dir_all(&root);
    }
}
