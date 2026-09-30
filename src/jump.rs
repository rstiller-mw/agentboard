use crate::proc;
use serde_json::Value;
use std::io::Write;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub address: String,
    pub pid: u32,
    pub title: String,
}

/// Focuses the Hyprland window whose terminal process is an ancestor of `pid`, switching workspace if needed.
pub fn focus_terminal_of(pid: u32) -> Result<(), String> {
    let address = window_of(pid)?;
    focus(&address)
}

/// The window showing `pid`; when a terminal keeps several windows in one process, its title tells them apart.
pub fn window_of(pid: u32) -> Result<String, String> {
    let windows = windows()?;
    let candidates = candidates(pid, &windows, proc::parent)?;
    match candidates.as_slice() {
        [only] => Ok(only.address.clone()),
        _ => disambiguate(pid, &candidates),
    }
}

/// The windows of the nearest ancestor process that owns any, or an error when none does.
fn candidates(pid: u32, windows: &[Window], parent: impl Fn(u32) -> Option<u32>) -> Result<Vec<Window>, String> {
    let mut current = pid;
    for _ in 0..64 {
        let owned: Vec<Window> = windows.iter().filter(|w| w.pid == current).cloned().collect();
        if !owned.is_empty() {
            return Ok(owned);
        }
        current = parent(current).filter(|&p| p > 1).ok_or("no terminal window found")?;
    }
    Err("no terminal window found".into())
}

fn windows() -> Result<Vec<Window>, String> {
    let out = Command::new("hyprctl").args(["clients", "-j"]).output().map_err(|e| format!("hyprctl: {e}"))?;
    parse_clients(&out.stdout).ok_or_else(|| "hyprctl returned no windows".into())
}

fn parse_clients(json: &[u8]) -> Option<Vec<Window>> {
    let clients: Vec<Value> = serde_json::from_slice(json).ok()?;
    Some(
        clients
            .iter()
            .filter_map(|c| {
                Some(Window {
                    address: c["address"].as_str()?.to_owned(),
                    pid: u32::try_from(c["pid"].as_u64()?).ok()?,
                    title: c["title"].as_str().unwrap_or_default().to_owned(),
                })
            })
            .collect(),
    )
}

/// Which of several windows of one process runs `pid`: name the agent's terminal with a unique title,
/// see which window took it, then put the old title back.
fn disambiguate(pid: u32, candidates: &[Window]) -> Result<String, String> {
    let tty = proc::tty_path(pid).ok_or("several windows share this terminal process and its tty is unknown")?;
    let marker = format!("agentboard-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    set_title(&tty, &marker)?;
    let deadline = Instant::now() + Duration::from_millis(600);
    let mut found = None;
    while found.is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
        found = windows().ok().and_then(|now| now.into_iter().find(|w| w.title == marker));
    }
    let original = found.as_ref().and_then(|w| candidates.iter().find(|c| c.address == w.address)).map(|c| c.title.clone());
    if let Some(title) = original {
        let _ = set_title(&tty, &title);
    }
    found.map(|w| w.address).ok_or_else(|| "could not tell which of the terminal's windows runs this agent".into())
}

fn set_title(tty: &str, title: &str) -> Result<(), String> {
    let mut device = std::fs::OpenOptions::new().write(true).open(tty).map_err(|e| format!("{tty}: {e}"))?;
    device.write_all(format!("\x1b]2;{title}\x1b\\").as_bytes()).map_err(|e| format!("{tty}: {e}"))
}

pub fn active_address() -> Option<String> {
    let out = Command::new("hyprctl").args(["activewindow", "-j"]).output().ok()?;
    serde_json::from_slice::<Value>(&out.stdout).ok()?["address"].as_str().map(str::to_owned)
}

/// Same fallback as Omarchy's own focus script: Lua dispatcher first, classic syntax for older Hyprland.
pub fn focus(address: &str) -> Result<(), String> {
    let hyprctl_ok = |arg: String| {
        Command::new("hyprctl")
            .args(["dispatch", &arg])
            .output()
            .is_ok_and(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).contains("error"))
    };
    let lua = format!("hl.dsp.focus({{ window = \"address:{address}\" }})");
    if hyprctl_ok(lua) || hyprctl_ok(format!("focuswindow address:{address}")) {
        Ok(())
    } else {
        Err("hyprctl could not focus the window".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn window(address: &str, pid: u32) -> Window {
        Window { address: address.into(), pid, title: String::new() }
    }

    fn tree(edges: &[(u32, u32)]) -> impl Fn(u32) -> Option<u32> {
        let parents: HashMap<u32, u32> = edges.iter().copied().collect();
        move |pid| parents.get(&pid).copied()
    }

    #[test]
    fn finds_the_window_of_the_nearest_ancestor() {
        let windows = [window("0xa", 10), window("0xb", 20)];
        let parent = tree(&[(300, 200), (200, 20), (20, 10), (10, 1)]);
        assert_eq!(candidates(300, &windows, parent).unwrap(), vec![window("0xb", 20)]);
    }

    #[test]
    fn the_process_itself_may_own_the_window() {
        let windows = [window("0xa", 10)];
        assert_eq!(candidates(10, &windows, tree(&[])).unwrap(), vec![window("0xa", 10)]);
    }

    #[test]
    fn returns_every_window_of_a_shared_terminal_process() {
        let windows = [window("0xa", 10), window("0xb", 10), window("0xc", 99)];
        let found = candidates(50, &windows, tree(&[(50, 10)])).unwrap();
        assert_eq!(found.iter().map(|w| w.address.as_str()).collect::<Vec<_>>(), ["0xa", "0xb"]);
    }

    #[test]
    fn fails_when_no_ancestor_owns_a_window() {
        let windows = [window("0xa", 10)];
        assert!(candidates(50, &windows, tree(&[(50, 40), (40, 1)])).is_err());
    }

    #[test]
    fn stops_on_a_parent_cycle() {
        let windows = [window("0xa", 10)];
        assert!(candidates(50, &windows, tree(&[(50, 40), (40, 50)])).is_err());
    }

    #[test]
    fn parses_hyprctl_clients_and_skips_incomplete_entries() {
        let json = br#"[{"address":"0x1","pid":7,"title":"vim"},{"pid":8},{"address":"0x2","pid":9}]"#;
        let parsed = parse_clients(json).unwrap();
        assert_eq!(parsed, vec![Window { address: "0x1".into(), pid: 7, title: "vim".into() }, window("0x2", 9)]);
        assert!(parse_clients(b"not json").is_none());
    }
}
