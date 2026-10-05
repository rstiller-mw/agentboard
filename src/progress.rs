use crate::dismissed::state_dir;
use std::fs;

/// Self-reported progress of an agent: the file `<state>/agentboard/progress/<session id>` holds the percent on the
/// first line and an optional step name on the second.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub percent: u8,
    pub step: String,
}

pub fn of(session_id: &str) -> Option<Progress> {
    if session_id.is_empty() {
        return None;
    }
    parse(&fs::read_to_string(state_dir().join("progress").join(session_id)).ok()?)
}

fn parse(text: &str) -> Option<Progress> {
    let mut lines = text.lines();
    let percent = lines.next()?.trim().parse::<u32>().ok()?.min(100) as u8;
    let step = lines.next().unwrap_or_default().trim().to_owned();
    Some(Progress { percent, step })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_percent_and_optional_step() {
        assert_eq!(parse("40\nslice 2/3: tests\n"), Some(Progress { percent: 40, step: "slice 2/3: tests".into() }));
        assert_eq!(parse("7"), Some(Progress { percent: 7, step: String::new() }));
    }

    #[test]
    fn clamps_to_100_and_rejects_garbage() {
        assert_eq!(parse("250").map(|p| p.percent), Some(100));
        assert_eq!(parse("soon"), None);
        assert_eq!(parse(""), None);
    }
}
