use crate::agent::{Agent, Kind, Provider, Status};
use crate::proc;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Clock ticks per second (100 = one full CPU) above which a process counts as working; an idle TUI uses none.
const BUSY_TICKS_PER_SECOND: f64 = 3.0;
/// Samples closer together than this are too noisy to compare, so the previous verdict stands.
const MIN_SAMPLE_GAP: Duration = Duration::from_millis(500);

struct Sample {
    ticks: u64,
    at: Instant,
    busy: bool,
}

static SAMPLES: Mutex<Option<HashMap<u32, Sample>>> = Mutex::new(None);

/// Codex, Gemini and opencode keep no registry of running instances, so they are found by scanning /proc.
pub fn load() -> Vec<Agent> {
    let found: Vec<(u32, Provider)> = proc::all_pids()
        .into_iter()
        .filter_map(|pid| Some((pid, identify(&proc::comm(pid)?, &proc::args(pid))?)))
        .collect();
    let with_provider: HashMap<u32, Provider> = found.iter().copied().collect();
    let roots: Vec<(u32, Provider)> = found
        .into_iter()
        .filter(|(pid, provider)| proc::parent(*pid).and_then(|p| with_provider.get(&p)) != Some(provider))
        .collect();

    let mut samples = SAMPLES.lock().unwrap_or_else(|e| e.into_inner());
    let mut previous = samples.take().unwrap_or_default();
    let now = Instant::now();
    let mut current = HashMap::new();
    let agents = roots
        .into_iter()
        .filter_map(|(pid, provider)| {
            let ticks = proc::cpu_ticks(pid)?;
            let sample = match previous.remove(&pid) {
                Some(before) if now.duration_since(before.at) < MIN_SAMPLE_GAP => before,
                Some(before) => {
                    let rate = ticks.saturating_sub(before.ticks) as f64 / now.duration_since(before.at).as_secs_f64();
                    Sample { ticks, at: now, busy: rate >= BUSY_TICKS_PER_SECOND }
                }
                None => Sample { ticks, at: now, busy: false },
            };
            let busy = sample.busy;
            current.insert(pid, sample);
            Some(agent(pid, provider, busy))
        })
        .collect();
    *samples = Some(current);
    agents
}

fn agent(pid: u32, provider: Provider, busy: bool) -> Agent {
    let interactive = proc::has_tty(pid);
    let cwd = proc::cwd(pid).unwrap_or_default();
    let name = cwd.rsplit('/').find(|s| !s.is_empty()).unwrap_or(provider.label()).to_owned();
    Agent {
        id: format!("{}:{pid}", provider.label()),
        provider,
        kind: if interactive { Kind::Interactive } else { Kind::Background },
        status: if busy { Status::Working } else { Status::Idle },
        name,
        cwd,
        detail: String::new(),
        created_ms: proc::start_ms(pid).unwrap_or(0),
        cost: None,
        progress: None,
        pid: interactive.then_some(pid),
        open: None,
    }
}

/// Which tool a process is, from its command name and (for interpreters) its script.
fn identify(comm: &str, args: &[String]) -> Option<Provider> {
    let by_name = |name: &str| match name.trim_end_matches(".js").trim_end_matches(".mjs") {
        "codex" => Some(Provider::Codex),
        "gemini" => Some(Provider::Gemini),
        "opencode" => Some(Provider::Opencode),
        _ => None,
    };
    match comm {
        "node" | "bun" | "deno" => by_name(args.get(1)?.rsplit('/').next()?),
        other => by_name(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn recognises_native_binaries_by_command_name() {
        assert_eq!(identify("codex", &args(&["codex"])), Some(Provider::Codex));
        assert_eq!(identify("opencode", &args(&["opencode", "--auto"])), Some(Provider::Opencode));
    }

    #[test]
    fn recognises_node_scripts_by_script_name() {
        let gemini = args(&["node", "/home/u/.local/share/mise/installs/npm-google-gemini-cli/0.61/lib/node_modules/@google/gemini-cli/bundle/gemini.js"]);
        assert_eq!(identify("node", &gemini), Some(Provider::Gemini));
    }

    #[test]
    fn ignores_other_processes_that_merely_mention_an_agent() {
        assert_eq!(identify("node", &args(&["node", "/srv/app.js", "codex"])), None);
        assert_eq!(identify("bash", &args(&["bash", "-c", "opencode run"])), None);
        assert_eq!(identify("claude", &args(&["claude"])), None);
    }

    #[test]
    fn own_process_is_not_an_agent() {
        let me = std::process::id();
        assert_eq!(identify(&proc::comm(me).unwrap(), &proc::args(me)), None);
    }
}
