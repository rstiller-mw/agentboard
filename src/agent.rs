use crate::progress::Progress;
use crate::theme::Rgb;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provider {
    Claude,
    Codex,
    Gemini,
    Opencode,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::Gemini => "gemini",
            Provider::Opencode => "opencode",
        }
    }

    /// The tool's own colour, independent of the Omarchy theme, so a card is recognisable at a glance.
    pub fn brand(self) -> Rgb {
        match self {
            Provider::Claude => Rgb(0xd9, 0x77, 0x57),
            Provider::Codex => Rgb(0x10, 0xa3, 0x7f),
            Provider::Gemini => Rgb(0x47, 0x96, 0xe3),
            Provider::Opencode => Rgb(0xb7, 0xb1, 0xb1),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Interactive,
    Background,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Interactive => "interactive",
            Kind::Background => "background",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Working,
    Waiting,
    Idle,
    Done,
    Failed,
    /// A state word we don't recognise, shown as such instead of being mistaken for idle.
    Unknown,
}

impl Status {
    pub fn parse(raw: &str) -> Status {
        match raw {
            "busy" | "working" | "running" | "active" => Status::Working,
            "blocked" | "waiting" | "needs_input" | "permission" => Status::Waiting,
            "idle" => Status::Idle,
            "done" | "completed" | "complete" | "success" => Status::Done,
            "failed" | "error" | "errored" => Status::Failed,
            _ => Status::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Working => "working",
            Status::Waiting => "waiting",
            Status::Idle => "idle",
            Status::Done => "done",
            Status::Failed => "failed",
            Status::Unknown => "unknown",
        }
    }

    pub fn is_finished(self) -> bool {
        matches!(self, Status::Done | Status::Failed)
    }
}

#[derive(Clone, Debug)]
pub struct Agent {
    pub id: String,
    pub provider: Provider,
    pub kind: Kind,
    pub status: Status,
    pub name: String,
    pub cwd: String,
    pub detail: String,
    pub created_ms: u64,
    /// Estimated dollars spent, when a transcript is available to price.
    pub cost: Option<f64>,
    /// Self-reported by the agent through a progress file.
    pub progress: Option<Progress>,
    /// Only set for a live process that owns a terminal, which is what makes an agent jumpable.
    pub pid: Option<u32>,
    /// Command that reopens the agent in a new terminal when there is no live terminal to jump to.
    pub open: Option<Vec<String>>,
}

impl Agent {
    /// A finished agent's leftover progress would only mislead.
    pub fn shown_progress(&self) -> Option<&Progress> {
        self.progress.as_ref().filter(|_| !self.status.is_finished())
    }
}

/// Costs are estimates, hence the tilde.
pub fn format_cost(dollars: f64) -> String {
    if dollars < 0.01 { "<$0.01".to_owned() } else { format!("~${dollars:.2}") }
}

pub fn format_age(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m", seconds / 60),
        3600..=86399 => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
        _ => format!("{}d {}h", seconds / 86400, seconds % 86400 / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_claude_state_words_to_statuses() {
        assert_eq!(Status::parse("busy"), Status::Working);
        assert_eq!(Status::parse("blocked"), Status::Waiting);
        assert_eq!(Status::parse("needs_input"), Status::Waiting);
        assert_eq!(Status::parse("done"), Status::Done);
        assert_eq!(Status::parse("idle"), Status::Idle);
    }

    #[test]
    fn unrecognised_state_words_are_not_mistaken_for_idle() {
        assert_eq!(Status::parse("brand-new-state"), Status::Unknown);
        assert_eq!(Status::parse(""), Status::Unknown);
    }

    #[test]
    fn formats_costs_as_estimated_dollars() {
        assert_eq!(format_cost(5.5488), "~$5.55");
        assert_eq!(format_cost(0.004), "<$0.01");
        assert_eq!(format_cost(27.0), "~$27.00");
    }

    #[test]
    fn formats_ages_compactly() {
        assert_eq!(format_age(42), "42s");
        assert_eq!(format_age(600), "10m");
        assert_eq!(format_age(3 * 3600 + 600), "3h 10m");
        assert_eq!(format_age(34 * 3600), "1d 10h");
    }
}
