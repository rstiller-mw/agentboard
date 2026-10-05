use crate::agent::{Agent, Kind, Status, format_age, format_cost};
use crate::theme::{Palette, Rgb};

const CARD_ROWS: usize = 4;
const GAP_ROWS: usize = 1;
const HEADER_ROWS: usize = 2;
const FOOTER_ROWS: usize = 2;
const MARGIN: usize = 2;
const MIN_CARD_WIDTH: usize = 24;
/// Cells between the left border and the text: " ███  ●  ".
const GUTTER: usize = 9;
/// Width of the coloured agent indicator, the same for every kind.
const BAR_CELLS: usize = 3;
/// Width of "100%".
const PERCENT_CELLS: usize = 4;
const SEPARATOR: &str = "  ·  ";

pub struct View<'a> {
    pub agents: &'a [&'a Agent],
    pub selected: usize,
    pub top: usize,
    pub palette: &'a Palette,
    pub width: usize,
    pub height: usize,
    pub now_ms: u64,
    pub message: &'a str,
    pub hiding_finished: bool,
    /// Finished agents in total, hidden or not, for the footer hint.
    pub finished: usize,
}

/// Cards grow by a progress row as soon as any listed agent reports progress, so they stay the same height.
pub fn card_rows(agents: &[&Agent]) -> usize {
    CARD_ROWS + usize::from(agents.iter().any(|a| a.shown_progress().is_some()))
}

/// How many cards of `card_rows` rows fit between header and footer.
pub fn capacity(height: usize, card_rows: usize) -> usize {
    let available = height.saturating_sub(HEADER_ROWS + FOOTER_ROWS);
    ((available + GAP_ROWS) / (card_rows + GAP_ROWS)).max(1)
}

pub fn render(v: &View) -> String {
    let p = v.palette;
    let card_width = v.width.saturating_sub(2 * MARGIN).max(MIN_CARD_WIDTH);
    let mut lines = vec![header(v, card_width), String::new()];

    if v.agents.is_empty() {
        lines.push(paint(p.soft, "No agents found (Claude, Codex, Gemini, opencode)."));
    }
    let rows = card_rows(v.agents);
    let visible = v.agents.iter().enumerate().skip(v.top).take(capacity(v.height, rows));
    for (n, (index, agent)) in visible.enumerate() {
        if n > 0 {
            lines.extend(std::iter::repeat_n(String::new(), GAP_ROWS));
        }
        lines.extend(card(agent, index == v.selected, card_width, p, v.now_ms, rows > CARD_ROWS));
    }

    lines.resize(v.height.saturating_sub(FOOTER_ROWS), String::new());
    lines.push(paint(p.yellow, &sanitize(v.message)));
    lines.push(paint(p.dim, &hints(v.hiding_finished, v.finished)));
    lines.truncate(v.height);

    let margin = " ".repeat(MARGIN);
    let body: Vec<String> = lines.iter().map(|l| format!("{margin}{l}\x1b[K")).collect();
    format!("\x1b[H{}\x1b[J", body.join("\r\n"))
}

fn hints(hiding_finished: bool, finished: usize) -> String {
    let toggle = match (hiding_finished, finished) {
        (_, 0) => None,
        (true, n) => Some(format!("h show {n} finished")),
        (false, _) => Some("h hide finished".to_owned()),
    };
    let mut hints = vec!["j/k move".to_owned(), "enter/space jump".to_owned(), "x remove".to_owned()];
    hints.extend(toggle);
    hints.push("q quit".to_owned());
    hints.join(SEPARATOR)
}

fn header(v: &View, width: usize) -> String {
    let p = v.palette;
    let counts: Vec<(String, Status)> = [Status::Working, Status::Waiting, Status::Idle, Status::Done, Status::Failed, Status::Unknown]
        .into_iter()
        .filter_map(|s| {
            let n = v.agents.iter().filter(|a| a.status == s).count();
            (n > 0).then(|| (format!("{n} {}", s.label()), s))
        })
        .collect();
    let summary_plain = counts.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(SEPARATOR);
    let summary = counts
        .iter()
        .map(|(t, s)| paint(status_style(*s, p).1, t))
        .collect::<Vec<_>>()
        .join(&paint(p.dim, SEPARATOR));
    let position = if v.agents.is_empty() { String::new() } else { format!("  {}/{}", v.selected + 1, v.agents.len()) };
    let title = "agentboard";
    let pad = width.saturating_sub(title.len() + summary_plain.chars().count() + position.chars().count());
    format!("{}{}{}{}", bold(p.accent, title), " ".repeat(pad), summary, paint(p.dim, &position))
}

fn status_style(status: Status, p: &Palette) -> (&'static str, Rgb) {
    match status {
        Status::Working => ("●", p.green),
        Status::Waiting => ("◆", p.yellow),
        Status::Idle => ("○", p.blue),
        Status::Done => ("✓", p.cyan),
        Status::Failed => ("✗", p.red),
        Status::Unknown => ("?", p.dim),
    }
}

fn card(a: &Agent, selected: bool, width: usize, p: &Palette, now_ms: u64, with_progress_row: bool) -> Vec<String> {
    let border = if selected { p.accent } else { p.dim };
    let kind_color = a.provider.brand();
    let bar_glyph = match a.kind {
        Kind::Interactive => "█",
        Kind::Background => "▒",
    };
    let (icon, status_color) = status_style(a.status, p);
    let text_width = width - 2 - GUTTER - 1;
    let side = paint(border, "│");
    let bar = paint(kind_color, &bar_glyph.repeat(BAR_CELLS));

    let name = fit(&a.name, text_width);
    let name = bold(if a.status.is_finished() && !selected { p.soft } else { p.text }, &name);

    let mut rows = vec![
        paint(border, &format!("╭{}╮", "─".repeat(width - 2))),
        format!("{side} {bar}  {}  {name} {side}", paint(status_color, icon)),
        format!("{side} {bar}     {} {side}", info(a, text_width, p, now_ms)),
    ];
    if with_progress_row {
        rows.push(progress_row(a, text_width, p, &side, &bar));
    }
    rows.push(paint(border, &format!("╰{}╯", "─".repeat(width - 2))));
    rows
}

/// Step name, a thin bar filling the rest of the row, and the percentage; blank for an agent without progress.
fn progress_row(a: &Agent, text_width: usize, p: &Palette, side: &str, bar: &str) -> String {
    let Some(progress) = a.shown_progress() else {
        return format!("{side} {bar}     {} {side}", " ".repeat(text_width));
    };
    let step = match progress.step.chars().count() {
        0 => String::new(),
        n => format!("{} ", fit(&progress.step, n.min(text_width / 3))),
    };
    let bar_cells = text_width.saturating_sub(step.chars().count() + PERCENT_CELLS + 1);
    let filled = bar_cells * usize::from(progress.percent) / 100;
    format!(
        "{side} {bar}     {}{}{} {} {side}",
        paint(p.soft, &step),
        paint(p.accent, &"━".repeat(filled)),
        paint(p.dim, &"─".repeat(bar_cells - filled)),
        paint(p.text, &format!("{:>3}%", progress.percent)),
    )
}

fn info(a: &Agent, width: usize, p: &Palette, now_ms: u64) -> String {
    let label = a.status.label();
    let tail = if !a.detail.is_empty() { a.detail.clone() } else { shorten_home(&a.cwd) };
    let age = (a.created_ms > 0).then(|| format_age(now_ms.saturating_sub(a.created_ms) / 1000));
    let cost = a.cost.map(format_cost);
    let source = match a.kind {
        Kind::Interactive => a.provider.label().to_owned(),
        Kind::Background => format!("{} job", a.provider.label()),
    };
    let parts: Vec<&str> = [Some(label), Some(source.as_str()), age.as_deref(), cost.as_deref(), Some(tail.as_str())]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect();
    let plain = fit(&parts.join(SEPARATOR), width);
    let split = plain.char_indices().nth(label.chars().count()).map_or(plain.len(), |(i, _)| i);
    let (label_part, rest) = plain.split_at(split);
    format!("{}{}", paint(status_style(a.status, p).1, label_part), paint(p.soft, rest))
}

fn shorten_home(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    match path.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// Truncates with an ellipsis and pads with spaces to exactly `width` cells (one cell per char).
fn fit(s: &str, width: usize) -> String {
    let clean = sanitize(s);
    let len = clean.chars().count();
    if len <= width {
        format!("{clean}{}", " ".repeat(width - len))
    } else {
        format!("{}…", clean.chars().take(width.saturating_sub(1)).collect::<String>())
    }
}

fn paint(c: Rgb, s: &str) -> String {
    format!("\x1b[38;2;{};{};{}m{s}\x1b[0m", c.0, c.1, c.2)
}

fn bold(c: Rgb, s: &str) -> String {
    format!("\x1b[1;38;2;{};{};{}m{s}\x1b[0m", c.0, c.1, c.2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Provider;
    use crate::progress::Progress;

    fn agent(name: &str, status: Status) -> Agent {
        Agent {
            id: name.into(),
            provider: Provider::Claude,
            kind: Kind::Interactive,
            status,
            name: name.into(),
            cwd: "/somewhere/very/long/path/that/keeps/going/and/going/and/going/on".into(),
            detail: String::new(),
            created_ms: 1,
            cost: None,
            progress: None,
            pid: None,
            open: None,
        }
    }

    fn visible_width(s: &str) -> usize {
        let mut in_escape = false;
        s.chars()
            .filter(|&c| match (in_escape, c) {
                (_, '\x1b') => { in_escape = true; false }
                (true, 'm') => { in_escape = false; false }
                (true, _) => false,
                _ => true,
            })
            .count()
    }

    #[test]
    fn every_card_row_has_the_same_visible_width() {
        for width in [24, 40, 100] {
            let mut a = agent("a very long name that must be truncated somewhere", Status::Working);
            for (progress, with_row) in [(None, false), (None, true), (Some(Progress { percent: 42, step: "slice 2/3: implementing the thing".into() }), true), (Some(Progress { percent: 100, step: String::new() }), true)] {
                a.progress = progress;
                let rows = card(&a, true, width, &Palette::load(), 10_000, with_row);
                assert!(rows.iter().all(|r| visible_width(r) == width), "width {width}: {:?}", rows.iter().map(|r| visible_width(r)).collect::<Vec<_>>());
            }
        }
    }

    #[test]
    fn progress_row_shows_step_thin_bar_and_percent() {
        let mut a = agent("x", Status::Working);
        a.progress = Some(Progress { percent: 50, step: "tests".into() });
        let row = progress_row(&a, 36, &Palette::load(), "│", "███");
        let plain: String = row.split('\x1b').map(|s| s.split_once('m').map_or(s, |(_, t)| t)).collect();
        assert_eq!(plain, format!("│ ███     tests {}{}  50% │", "━".repeat(12), "─".repeat(13)));
    }

    #[test]
    fn finished_agents_show_no_progress() {
        let mut a = agent("x", Status::Done);
        a.progress = Some(Progress { percent: 50, step: String::new() });
        assert_eq!(card_rows(&[&a]), CARD_ROWS);
        a.status = Status::Working;
        assert_eq!(card_rows(&[&a]), CARD_ROWS + 1);
    }

    #[test]
    fn fit_pads_short_text_and_truncates_long_text_with_an_ellipsis() {
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("a\nb", 3), "a b");
    }

    #[test]
    fn capacity_counts_the_gap_between_cards() {
        assert_eq!(capacity(24, 4), 4);
        assert_eq!(capacity(24, 5), 3);
        assert_eq!(capacity(3, 4), 1);
    }

    #[test]
    fn frame_has_exactly_the_terminal_height() {
        let a = agent("x", Status::Idle);
        let list = [&a];
        let view = View { agents: &list, selected: 0, top: 0, palette: &Palette::load(), width: 80, height: 24, now_ms: 5_000, message: "", hiding_finished: false, finished: 0 };
        assert_eq!(render(&view).matches("\r\n").count(), 23);
    }
}

#[cfg(test)]
mod hint_tests {
    use super::*;

    #[test]
    fn footer_mentions_hidden_finished_agents_only_when_there_are_some() {
        assert!(hints(true, 4).contains("h show 4 finished"));
        assert!(hints(false, 4).contains("h hide finished"));
        assert!(!hints(true, 0).contains("finished"));
    }
}
