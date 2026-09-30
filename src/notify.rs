use crate::agent::{Agent, Provider, Status};
use crate::{claude, jump, launch, sources, watcher::Watcher};
use std::collections::HashMap;
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reason {
    NeedsInput,
    Finished,
    Failed,
}

impl Reason {
    fn text(self) -> &'static str {
        match self {
            Reason::NeedsInput => "needs your input",
            Reason::Finished => "finished",
            Reason::Failed => "failed",
        }
    }
}

/// Agents whose status changed since `previous` in a way that wants attention. Agents seen for the first time are silent,
/// so starting the watcher does not announce everything that already exists.
pub fn events<'a>(previous: &HashMap<String, Status>, agents: &'a [Agent]) -> Vec<(&'a Agent, Reason)> {
    agents
        .iter()
        .filter_map(|a| {
            let before = *previous.get(&a.id)?;
            let reason = match (before, a.status) {
                (b, s) if b == s => return None,
                (_, Status::Waiting) => Reason::NeedsInput,
                (_, Status::Done) => Reason::Finished,
                (_, Status::Failed) => Reason::Failed,
                // Only Claude reports idle/busy itself; for the others it is a CPU guess that flickers.
                (Status::Working, Status::Idle) if a.provider == Provider::Claude => Reason::Finished,
                _ => return None,
            };
            Some((a, reason))
        })
        .collect()
}

/// Runs until killed: sends a desktop notification per event, and clicking it jumps to the agent.
pub fn run() {
    let watcher = Watcher::new(claude::watch_roots());
    let mut previous: Option<HashMap<String, Status>> = None;
    loop {
        let agents = sources::load();
        if let Some(before) = &previous {
            for (agent, reason) in events(before, &agents) {
                notify(agent.clone(), reason);
            }
        }
        previous = Some(agents.iter().map(|a| (a.id.clone(), a.status)).collect());
        wait(watcher.as_ref());
    }
}

fn wait(watcher: Option<&Watcher>) {
    let mut fd = libc::pollfd { fd: watcher.map_or(-1, Watcher::fd), events: libc::POLLIN, revents: 0 };
    // SAFETY: one valid pollfd (a negative fd is ignored by poll).
    unsafe { libc::poll(&mut fd, 1, 1000) };
    if let Some(w) = watcher {
        w.drain();
        w.refresh();
    }
}

/// `notify-send --wait` blocks until the notification is closed, so each one gets its own thread.
fn notify(agent: Agent, reason: Reason) {
    std::thread::spawn(move || {
        if is_focused(&agent) {
            return;
        }
        let body = if agent.detail.is_empty() { agent.cwd.clone() } else { agent.detail.clone() };
        let output = Command::new("notify-send")
            .args(["--app-name=agentboard", "--action=default=Jump to terminal", "--wait"])
            .arg(format!("{} {}", agent.name, reason.text()))
            .arg(format!("{}  ·  {body}", agent.provider.label()))
            .output();
        if output.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "default") {
            let _ = launch::activate(&agent);
        }
    });
}

/// No point interrupting someone who is looking at the agent already.
fn is_focused(agent: &Agent) -> bool {
    let Some(pid) = agent.pid else { return false };
    matches!((jump::window_of(pid), jump::active_address()), (Ok(target), Some(active)) if target == active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Kind;

    fn agent(id: &str, provider: Provider, status: Status) -> Agent {
        Agent {
            id: id.into(),
            provider,
            kind: Kind::Interactive,
            status,
            name: id.into(),
            cwd: String::new(),
            detail: String::new(),
            created_ms: 0,
            cost: None,
            pid: None,
            open: None,
        }
    }

    fn before(entries: &[(&str, Status)]) -> HashMap<String, Status> {
        entries.iter().map(|(id, s)| (id.to_string(), *s)).collect()
    }

    fn reasons(previous: &[(&str, Status)], now: &[Agent]) -> Vec<Reason> {
        events(&before(previous), now).into_iter().map(|(_, r)| r).collect()
    }

    #[test]
    fn announces_agents_that_start_waiting_finish_or_fail() {
        let now = [agent("a", Provider::Claude, Status::Waiting), agent("b", Provider::Claude, Status::Done), agent("c", Provider::Claude, Status::Failed)];
        let previous = [("a", Status::Working), ("b", Status::Working), ("c", Status::Working)];
        assert_eq!(reasons(&previous, &now), [Reason::NeedsInput, Reason::Finished, Reason::Failed]);
    }

    #[test]
    fn a_claude_session_going_from_busy_to_idle_has_finished_its_turn() {
        let now = [agent("a", Provider::Claude, Status::Idle)];
        assert_eq!(reasons(&[("a", Status::Working)], &now), [Reason::Finished]);
    }

    #[test]
    fn cpu_based_idle_guesses_of_other_agents_stay_quiet() {
        let now = [agent("a", Provider::Codex, Status::Idle)];
        assert!(reasons(&[("a", Status::Working)], &now).is_empty());
    }

    #[test]
    fn unchanged_new_and_resumed_agents_are_silent() {
        let now = [agent("same", Provider::Claude, Status::Waiting), agent("new", Provider::Claude, Status::Done), agent("resumed", Provider::Claude, Status::Working)];
        assert!(reasons(&[("same", Status::Waiting), ("resumed", Status::Idle)], &now).is_empty());
    }
}
