mod agent;
mod claude;
mod cost;
mod dismissed;
mod jump;
mod launch;
mod list;
mod notify;
mod proc;
mod scan;
mod sources;
mod term;
mod theme;
mod ui;
mod watcher;

use dismissed::Dismissed;
use list::List;
use term::{Key, Term};
use theme::Palette;
use watcher::Watcher;

const USAGE: &str = "usage: agentboard [--json | watch]

  (no flag)  interactive list; j/k move, enter/space jump to the terminal (or reopen a background job), x/d remove from the list, h hide/show finished, q quit
  --json     print all agents as JSON and exit
  watch      run in the background and send a desktop notification when an agent finishes or needs input; click it to jump
  --version  print the version";

fn main() {
    match std::env::args().nth(1).as_deref() {
        None => run_tui(),
        Some("--json") => print_json(),
        Some("watch") => notify::run(),
        Some("--version" | "-V") => println!("agentboard {}", env!("CARGO_PKG_VERSION")),
        Some("-h" | "--help") => println!("{USAGE}"),
        Some(_) => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn print_json() {
    let agents: Vec<_> = sources::load()
        .iter()
        .map(|a| {
            serde_json::json!({
                "id": a.id, "provider": a.provider.label(), "kind": a.kind.label(), "status": a.status.label(),
                "name": a.name, "cwd": a.cwd, "detail": a.detail, "created_ms": a.created_ms, "cost": a.cost,
                "pid": a.pid, "jumpable": a.pid.is_some(), "reopenable": a.open.is_some(),
            })
        })
        .collect();
    println!("{}", serde_json::Value::Array(agents));
}

fn load_shown(dismissed: &Dismissed) -> Vec<agent::Agent> {
    let mut agents = sources::load();
    agents.retain(|a| !dismissed.contains(&a.id));
    agents
}

fn run_tui() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        term::restore();
        default_hook(info);
    }));
    let _term = Term::enter();

    let watcher = Watcher::new(claude::watch_roots());
    let mut dismissed = Dismissed::load(now_ms());
    let mut list = List::new(load_shown(&dismissed));
    let mut palette = Palette::load();
    let mut message = String::new();

    loop {
        let (width, height) = term::size();
        let top = list.top_for(ui::capacity(height));
        term::write_out(&ui::render(&ui::View {
            agents: &list.visible(),
            selected: list.selected(),
            top,
            palette: &palette,
            width,
            height,
            now_ms: now_ms(),
            message: &message,
            hiding_finished: list.hiding_finished,
            finished: list.finished(),
        }));

        let key = term::read_key(1000, watcher.as_ref().map(Watcher::fd));
        if let Some(w) = &watcher {
            w.drain();
        }
        let Some(key) = key.filter(|k| *k != Key::Refresh) else {
            if let Some(w) = &watcher {
                w.refresh();
            }
            list.replace(load_shown(&dismissed));
            palette = Palette::load();
            continue;
        };
        message.clear();
        match key {
            Key::Quit => break,
            Key::Up => list.move_by(-1),
            Key::Down => list.move_by(1),
            Key::First => list.first(),
            Key::Last => list.last(),
            Key::ToggleFinished => list.toggle_finished(),
            Key::Dismiss => {
                if let Some(id) = list.current().map(|a| a.id.clone()) {
                    message = dismissed.add(&id, now_ms()).err().map(|e| format!("can't save: {e}")).unwrap_or_default();
                    list.replace(load_shown(&dismissed));
                }
            }
            Key::Jump => {
                if let Some(agent) = list.current() {
                    message = launch::activate(agent).err().unwrap_or_default();
                }
            }
            Key::Refresh | Key::Other => {}
        }
    }
}
