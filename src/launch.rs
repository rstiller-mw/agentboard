use crate::agent::Agent;
use crate::jump;
use std::process::{Command, Stdio};

/// Brings the agent's terminal to the front; without a live terminal, opens a new one running its reopen command.
pub fn activate(agent: &Agent) -> Result<(), String> {
    match (agent.pid, &agent.open) {
        (Some(pid), _) => jump::focus_terminal_of(pid),
        (None, Some(argv)) => open_terminal(&agent.cwd, argv),
        (None, None) => Err("no terminal to jump to and no way to reopen it".into()),
    }
}

/// Same route as Omarchy's own TUI launcher: uwsm-app around xdg-terminal-exec, detached from us.
fn open_terminal(cwd: &str, argv: &[String]) -> Result<(), String> {
    let cmd = terminal_command(cwd, argv);
    Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", cmd[0]))
}

fn terminal_command(cwd: &str, argv: &[String]) -> Vec<String> {
    let mut cmd: Vec<String> = ["setsid", "-f", "uwsm-app", "--", "xdg-terminal-exec"].map(String::from).into();
    if !cwd.is_empty() && std::path::Path::new(cwd).is_dir() {
        cmd.push(format!("--dir={cwd}"));
    }
    cmd.push("-e".into());
    cmd.extend(argv.iter().cloned());
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_the_command_in_the_agents_directory() {
        let cmd = terminal_command("/tmp", &["claude".into(), "--resume".into(), "abc".into()]);
        assert_eq!(cmd.join(" "), "setsid -f uwsm-app -- xdg-terminal-exec --dir=/tmp -e claude --resume abc");
    }

    #[test]
    fn leaves_out_a_directory_that_no_longer_exists() {
        let cmd = terminal_command("/no/such/dir/anywhere", &["claude".into()]);
        assert_eq!(cmd.join(" "), "setsid -f uwsm-app -- xdg-terminal-exec -e claude");
    }
}
