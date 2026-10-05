use crate::agent::{Agent, Kind, Provider, Status};
use crate::cost;
use crate::proc;
use crate::progress;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

fn config_dir() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".claude"),
    }
}

/// Directories whose changes mean the agent list changed.
pub fn watch_roots() -> Vec<PathBuf> {
    let dir = config_dir();
    vec![dir.join("sessions"), dir.join("jobs")]
}

/// Everything Claude knows about right now: live interactive sessions plus background jobs.
pub fn load() -> Vec<Agent> {
    let dir = config_dir();
    let jobs = jobs(&dir.join("jobs"), &dir.join("projects"));
    let job_ids: HashSet<&str> = jobs.iter().map(|j| j.id.as_str()).collect();
    // A live interactive session wins over its job entry (it is jumpable); a daemon-hosted one loses to it.
    let sessions: Vec<Agent> = sessions(&dir.join("sessions"), &dir.join("projects"))
        .into_iter()
        .filter(|s| s.kind == Kind::Interactive || !job_ids.contains(s.id.as_str()))
        .collect();
    let live: HashSet<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    let extra: Vec<Agent> = jobs.iter().filter(|j| !live.contains(j.id.as_str())).cloned().collect();
    let mut agents = sessions;
    agents.extend(extra);
    agents
}

/// Files are rewritten in place, so a read can catch one half-written; one retry is enough.
fn read_json(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok().or_else(|| {
        std::thread::sleep(std::time::Duration::from_millis(5));
        serde_json::from_slice(&fs::read(path).ok()?).ok()
    })
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect()
}

fn str_field(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_owned()
}

/// A timestamp as Claude has written it across versions: epoch millis or seconds (number or string), or ISO 8601.
fn time_field(v: &Value) -> Option<u64> {
    let epoch = |n: u64| if n < 100_000_000_000 { n * 1000 } else { n };
    match v {
        Value::Number(n) => n.as_u64().map(epoch),
        Value::String(s) => s.parse::<u64>().ok().map(epoch).or_else(|| parse_iso_ms(s)),
        _ => None,
    }
}

fn pid_field(v: &Value) -> Option<u32> {
    match &v["pid"] {
        Value::Number(n) => u32::try_from(n.as_u64()?).ok(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path)
}

fn sessions(dir: &Path, projects: &Path) -> Vec<Agent> {
    json_files(dir)
        .iter()
        .filter_map(|p| read_json(p))
        .filter_map(|v| {
            let mut agent = session(&v)?;
            let transcript = transcript(projects, &agent.id);
            // A name Claude derived (like "billing-api-93") is worse than the title it wrote in the transcript.
            if v["nameSource"].as_str().is_none_or(|s| matches!(s, "derived" | "auto")) {
                agent.name = transcript.as_deref().and_then(|t| ai_title(t, &agent.id)).unwrap_or(agent.name);
            }
            agent.cost = transcript.as_deref().and_then(cost::of);
            agent.progress = progress::of(&agent.id);
            Some(agent)
        })
        .collect()
}

/// The transcript grows to megabytes, but the title is rewritten often enough to always sit near the end.
const TRANSCRIPT_TAIL: u64 = 256 * 1024;

/// Last title found per session, so a tail that momentarily holds none doesn't flip the name back.
static TITLES: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

/// The session's transcript lives in a per-project folder whose name we don't need to derive.
fn transcript(projects: &Path, session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty() {
        return None;
    }
    let file = format!("{session_id}.jsonl");
    fs::read_dir(projects).into_iter().flatten().flatten().map(|d| d.path().join(&file)).find(|p| p.is_file())
}

fn ai_title(transcript: &Path, session_id: &str) -> Option<String> {
    let found = tail(transcript).and_then(|text| last_ai_title(&text));
    let mut cache = TITLES.lock().ok()?;
    match found {
        Some(title) => {
            cache.insert(session_id.to_owned(), title.clone());
            Some(title)
        }
        None => cache.get(session_id).cloned(),
    }
}

fn tail(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(file.metadata().ok()?.len().saturating_sub(TRANSCRIPT_TAIL))).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Lines are JSON objects; the first line of a tail may be cut in half, which simply fails to parse.
fn last_ai_title(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .filter(|l| l.contains("\"ai-title\""))
        .find_map(|l| serde_json::from_str::<Value>(l).ok()?["aiTitle"].as_str().filter(|t| !t.is_empty()).map(str::to_owned))
}

fn session(v: &Value) -> Option<Agent> {
    let pid = pid_field(v)?;
    if !is_same_process(pid, v["procStart"].as_str()) {
        return None;
    }
    let id = str_field(v, "sessionId");
    let cwd = str_field(v, "cwd");
    let name = match str_field(v, "name") {
        n if !n.is_empty() => n,
        _ if !cwd.is_empty() => basename(&cwd).to_owned(),
        _ => id.chars().take(8).collect(),
    };
    let interactive = v["kind"].as_str().is_none_or(|k| k == "interactive");
    Some(Agent {
        provider: Provider::Claude,
        kind: if interactive { Kind::Interactive } else { Kind::Background },
        status: Status::parse(v["status"].as_str().unwrap_or_default()),
        detail: String::new(),
        created_ms: time_field(&v["startedAt"]).unwrap_or(0),
        cost: None,
        progress: None,
        pid: interactive.then_some(pid),
        open: None,
        id,
        name,
        cwd,
    })
}

/// Session files outlive their process, so a file only counts while its PID still has the recorded start time.
fn is_same_process(pid: u32, recorded_start: Option<&str>) -> bool {
    match (proc::start_ticks(pid), recorded_start) {
        (Some(actual), Some(recorded)) => actual == recorded,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

fn jobs(dir: &Path, projects: &Path) -> Vec<Agent> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path().join("state.json");
            let v = read_json(&path)?;
            let modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
            let mut agent = job(&v, &e.file_name().to_string_lossy(), modified);
            agent.cost = transcript(projects, &agent.id).as_deref().and_then(cost::of);
            agent.progress = progress::of(&agent.id);
            Some(agent)
        })
        .collect()
}

fn job(v: &Value, dir_name: &str, modified: Option<std::time::SystemTime>) -> Agent {
    let session_id = str_field(v, "sessionId");
    let id = if session_id.is_empty() { dir_name.to_owned() } else { session_id.clone() };
    let name = match str_field(v, "name") {
        n if !n.is_empty() => n,
        _ => match str_field(v, "intent").lines().next() {
            Some(line) if !line.is_empty() => line.to_owned(),
            _ => id.chars().take(8).collect(),
        },
    };
    let status = Status::parse(v["state"].as_str().unwrap_or_default());
    let created_ms = time_field(&v["createdAt"])
        .or_else(|| time_field(&v["startedAt"]))
        .or_else(|| modified?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_millis() as u64))
        .unwrap_or(0);
    let short = str_field(v, "daemonShort");
    let resume = match str_field(v, "resumeSessionId") {
        r if !r.is_empty() => r,
        _ => session_id,
    };
    // A finished job has no daemon session left to attach to, so it is resumed from its transcript.
    let open = match (status.is_finished(), short.is_empty(), resume.is_empty()) {
        (false, false, _) => Some(vec!["claude".into(), "attach".into(), short]),
        (_, _, false) => Some(vec!["claude".into(), "--resume".into(), resume]),
        _ => None,
    };
    Agent {
        provider: Provider::Claude,
        kind: Kind::Background,
        status,
        detail: str_field(v, "detail"),
        created_ms,
        cost: None,
        progress: None,
        pid: None,
        open,
        id,
        name,
        cwd: str_field(v, "cwd"),
    }
}

/// Parses `2026-09-28T14:58:53.595Z` (UTC) to epoch milliseconds.
fn parse_iso_ms(s: &str) -> Option<u64> {
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let millis = if s.as_bytes().get(19) == Some(&b'.') { num(20..23).unwrap_or(0) } else { 0 };
    let secs = days_from_civil(year, month, day) * 86400 + hour * 3600 + minute * 60 + second;
    u64::try_from(secs * 1000 + millis).ok()
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso_timestamps_to_epoch_millis() {
        assert_eq!(parse_iso_ms("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_iso_ms("2026-09-28T14:58:53.595Z"), Some(1_790_607_533_595));
        assert_eq!(parse_iso_ms("nonsense"), None);
    }

    #[test]
    fn builds_a_session_agent_from_a_live_process() {
        let me = std::process::id();
        let v = serde_json::json!({
            "pid": me, "procStart": proc::start_ticks(me).unwrap(), "sessionId": "abc", "cwd": "/x/my-repo",
            "kind": "interactive", "status": "busy", "startedAt": 5000
        });
        let a = session(&v).unwrap();
        assert_eq!((a.name.as_str(), a.status, a.kind, a.pid), ("my-repo", Status::Working, Kind::Interactive, Some(me)));
    }

    #[test]
    fn ignores_sessions_whose_pid_was_reused() {
        let v = serde_json::json!({ "pid": std::process::id(), "procStart": "1", "sessionId": "abc" });
        assert!(session(&v).is_none());
    }

    /// Runs a real session file from a Claude release against the current process, so only the process check is faked.
    fn fixture_session(raw: &str) -> Agent {
        let mut v: Value = serde_json::from_str(raw).unwrap();
        let me = std::process::id();
        v["pid"] = me.into();
        v["procStart"] = proc::start_ticks(me).unwrap().into();
        session(&v).unwrap()
    }

    #[test]
    fn reads_the_session_format_of_claude_2_1_233() {
        let a = fixture_session(include_str!("fixtures/session-2.1.233.json"));
        assert_eq!(a.name, "billing-api-93");
        assert_eq!((a.status, a.kind, a.created_ms), (Status::Idle, Kind::Interactive, 1_790_581_917_722));
        assert_eq!(a.cwd, "/home/user/projects/billing-api");
    }

    #[test]
    fn reads_the_session_format_of_claude_2_1_283() {
        let a = fixture_session(include_str!("fixtures/session-2.1.283.json"));
        assert_eq!((a.name.as_str(), a.status, a.created_ms), ("agentboard-2c", Status::Working, 1_790_706_815_394));
    }

    #[test]
    fn reads_a_finished_background_job_of_claude_2_1_283() {
        let v: Value = serde_json::from_str(include_str!("fixtures/job-2.1.283.json")).unwrap();
        let a = job(&v, "11111111", None);
        assert_eq!((a.name.as_str(), a.status, a.kind, a.pid), ("API deployment logs", Status::Done, Kind::Background, None));
        assert_eq!(a.created_ms, parse_iso_ms("2026-09-29T11:14:51.963Z").unwrap());
        assert!(a.detail.starts_with("Deleting all pods"));
        assert_eq!(a.open, Some(vec!["claude".into(), "--resume".into(), "11111111-2222-4333-8444-555555555555".into()]));
    }

    #[test]
    fn a_running_job_is_attached_not_resumed() {
        let mut v: Value = serde_json::from_str(include_str!("fixtures/job-2.1.283.json")).unwrap();
        v["state"] = "working".into();
        assert_eq!(job(&v, "x", None).open, Some(vec!["claude".into(), "attach".into(), "11111111".into()]));
    }

    #[test]
    fn survives_a_job_with_only_a_few_fields() {
        let v: Value = serde_json::from_str(include_str!("fixtures/job-sparse.json")).unwrap();
        let a = job(&v, "dirname", None);
        assert_eq!((a.name.as_str(), a.status, a.id.as_str()), ("first line of intent", Status::Waiting, "dirname"));
        assert_eq!(a.created_ms, parse_iso_ms("2026-09-01T10:00:00Z").unwrap());
        assert_eq!(a.open, Some(vec!["claude".into(), "attach".into(), "abcd1234".into()]));
    }

    #[test]
    fn an_empty_object_still_produces_an_agent() {
        let a = job(&serde_json::json!({}), "dir1234567", None);
        assert_eq!((a.status, a.name.as_str(), a.open), (Status::Unknown, "dir12345", None));
    }

    #[test]
    fn accepts_timestamps_as_millis_seconds_strings_or_iso() {
        assert_eq!(time_field(&serde_json::json!(1_790_581_917_722u64)), Some(1_790_581_917_722));
        assert_eq!(time_field(&serde_json::json!(1_790_581_917u64)), Some(1_790_581_917_000));
        assert_eq!(time_field(&serde_json::json!("1790581917722")), Some(1_790_581_917_722));
        assert_eq!(time_field(&serde_json::json!("1970-01-01T00:00:01Z")), Some(1000));
        assert_eq!(time_field(&serde_json::json!(null)), None);
    }

    #[test]
    fn the_latest_ai_title_in_a_transcript_tail_wins() {
        let tail = concat!(
            "ial cut line\"}\n",
            "{\"type\":\"ai-title\",\"aiTitle\":\"First\"}\n",
            "{\"type\":\"user\",\"text\":\"mentions \\\"ai-title\\\" in passing\"}\n",
            "{\"type\":\"ai-title\",\"aiTitle\":\"Second\"}\n",
            "{\"type\":\"assistant\"}\n",
        );
        assert_eq!(last_ai_title(tail), Some("Second".into()));
        assert_eq!(last_ai_title("{\"type\":\"user\"}\n"), None);
    }

    #[test]
    fn a_derived_session_name_is_replaced_by_the_transcript_title_but_a_chosen_one_is_not() {
        let root = std::env::temp_dir().join(format!("agentboard-claude-{}", std::process::id()));
        let (sessions_dir, project) = (root.join("sessions"), root.join("projects/-x"));
        fs::create_dir_all(&sessions_dir).unwrap();
        fs::create_dir_all(&project).unwrap();
        let me = std::process::id();
        for (id, source) in [("derived-id", "derived"), ("chosen-id", "user")] {
            let v = serde_json::json!({
                "pid": me, "procStart": proc::start_ticks(me).unwrap(), "sessionId": id,
                "name": "generated-93", "nameSource": source, "kind": "interactive"
            });
            fs::write(sessions_dir.join(format!("{id}.json")), v.to_string()).unwrap();
            fs::write(project.join(format!("{id}.jsonl")), "{\"type\":\"ai-title\",\"aiTitle\":\"A real title\"}\n").unwrap();
        }
        let mut names: Vec<_> = sessions(&sessions_dir, &root.join("projects")).into_iter().map(|a| (a.id, a.name)).collect();
        names.sort();
        assert_eq!(names, [("chosen-id".into(), "generated-93".into()), ("derived-id".into(), "A real title".into())]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_a_pid_written_as_a_string() {
        assert_eq!(pid_field(&serde_json::json!({ "pid": "42" })), Some(42));
    }
}
