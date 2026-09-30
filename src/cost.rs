use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Dollars per million tokens.
#[derive(Clone, Copy)]
struct Price {
    input: f64,
    output: f64,
    cache_write_5m: f64,
    cache_write_1h: f64,
    cache_read: f64,
}

const fn price(input: f64, output: f64, cache_write_5m: f64, cache_write_1h: f64, cache_read: f64) -> Price {
    Price { input, output, cache_write_5m, cache_write_1h, cache_read }
}

/// Claude Code's own price catalog; the first matching prefix wins. Sonnet 5.5 is not in the catalog:
/// the Opus 5.5 tier reproduces a recorded Sonnet 5.5 session to within 0.2%.
const PRICES: [(&[&str], Price); 9] = [
    (&["claude-3-5-haiku"], price(0.8, 4.0, 1.0, 1.6, 0.08)),
    (&["claude-haiku-4-5"], price(1.0, 5.0, 1.25, 2.0, 0.1)),
    (&["claude-opus-5-5", "claude-sonnet-5-5"], price(4.0, 20.0, 5.0, 8.0, 0.2)),
    (&["claude-sonnet-5"], price(2.0, 10.0, 2.5, 4.0, 0.2)),
    (&["claude-opus-4-0", "claude-opus-4-1"], price(15.0, 75.0, 18.75, 30.0, 1.5)),
    (&["claude-fable-5-1", "claude-mythos-5-1"], price(10.0, 50.0, 12.5, 20.0, 0.25)),
    (&["claude-fable-", "claude-mythos-"], price(10.0, 50.0, 12.5, 20.0, 1.0)),
    (&["claude-sonnet-", "claude-3-5-sonnet", "claude-3-7-sonnet"], price(3.0, 15.0, 3.75, 6.0, 0.3)),
    (&["claude-opus-"], price(5.0, 25.0, 6.25, 10.0, 0.5)),
];
const UNKNOWN_MODEL: Price = price(5.0, 25.0, 6.25, 10.0, 0.5);
const WEB_SEARCH: f64 = 0.01;
const US_ONLY_INFERENCE: f64 = 1.1;

fn price_of(model: &str) -> Price {
    PRICES.iter().find(|(prefixes, _)| prefixes.iter().any(|p| model.starts_with(p))).map_or(UNKNOWN_MODEL, |(_, p)| *p)
}

fn message_cost(model: &str, usage: &Value) -> f64 {
    let p = price_of(model);
    let n = |v: &Value| v.as_f64().unwrap_or(0.0);
    let written = n(&usage["cache_creation_input_tokens"]);
    let hour = n(&usage["cache_creation"]["ephemeral_1h_input_tokens"]).min(written);
    let millionths = n(&usage["input_tokens"]) * p.input
        + n(&usage["output_tokens"]) * p.output
        + n(&usage["cache_read_input_tokens"]) * p.cache_read
        + hour * p.cache_write_1h
        + (written - hour) * p.cache_write_5m;
    let geo = if usage["inference_geo"] == "us" { US_ONLY_INFERENCE } else { 1.0 };
    millionths / 1e6 * geo + n(&usage["server_tool_use"]["web_search_requests"]) * WEB_SEARCH
}

/// What one transcript file has cost so far, and how far into it we have read.
#[derive(Default)]
struct Tally {
    offset: u64,
    /// One entry per API message: a message is written once per content block, so repeats overwrite.
    messages: HashMap<String, f64>,
}

impl Tally {
    /// Reads only what was appended since the last call.
    fn update(&mut self, path: &Path) -> f64 {
        let Ok(mut file) = fs::File::open(path) else { return self.messages.values().sum() };
        let len = file.metadata().map_or(0, |m| m.len());
        if len < self.offset {
            *self = Tally::default();
        }
        let mut fresh = Vec::new();
        if len > self.offset && file.seek(SeekFrom::Start(self.offset)).is_ok() {
            let _ = file.take(len - self.offset).read_to_end(&mut fresh);
        }
        // A line still being written has no newline yet; it is picked up on the next call.
        let complete = fresh.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        for line in String::from_utf8_lossy(&fresh[..complete]).lines() {
            self.add(line);
        }
        self.offset += complete as u64;
        self.messages.values().sum()
    }

    fn add(&mut self, line: &str) {
        if !line.contains("\"usage\"") {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        let message = &v["message"];
        let (Some(id), Some(model)) = (message["id"].as_str().or(v["uuid"].as_str()), message["model"].as_str()) else { return };
        if message["usage"].is_object() {
            self.messages.insert(id.to_owned(), message_cost(model, &message["usage"]));
        }
    }
}

static TALLIES: LazyLock<Mutex<HashMap<PathBuf, Tally>>> = LazyLock::new(Default::default);

/// Estimated dollars for a session: its transcript plus the sub-agent transcripts stored next to it.
/// Claude does not persist a running total, so this is rebuilt from token usage and lands a little below
/// Claude's own figure (calls that never reach the transcript, such as title generation, are missed).
pub fn of(transcript: &Path) -> Option<f64> {
    let mut files = vec![transcript.to_owned()];
    files.extend(
        fs::read_dir(transcript.with_extension("").join("subagents"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "jsonl")),
    );
    let mut tallies = TALLIES.lock().ok()?;
    let total: f64 = files.into_iter().map(|f| tallies.entry(f.clone()).or_default().update(&f)).sum();
    (total > 0.0).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, output: u64, written: u64, hour: u64, read: u64) -> Value {
        serde_json::json!({
            "input_tokens": input, "output_tokens": output, "cache_creation_input_tokens": written,
            "cache_read_input_tokens": read, "cache_creation": { "ephemeral_1h_input_tokens": hour }
        })
    }

    #[test]
    fn prices_a_message_from_a_real_session() {
        // Two opus-5 messages from a session Claude recorded at $0.57 (the rest is calls outside the transcript).
        let first = message_cost("claude-opus-5", &usage(2, 312, 47_791, 47_791, 0));
        let second = message_cost("claude-opus-5", &usage(2, 679, 1_022, 1_022, 47_791));
        assert!((first + second - 0.5368).abs() < 0.0001, "{}", first + second);
    }

    #[test]
    fn picks_the_price_tier_by_model_prefix() {
        assert_eq!(price_of("claude-opus-5-5").output, 20.0);
        assert_eq!(price_of("claude-opus-5").output, 25.0);
        assert_eq!(price_of("claude-sonnet-5").output, 10.0);
        assert_eq!(price_of("claude-sonnet-4-6").output, 15.0);
        assert_eq!(price_of("claude-haiku-4-5-20251001").output, 5.0);
        assert_eq!(price_of("something-new").output, UNKNOWN_MODEL.output);
    }

    #[test]
    fn us_only_inference_costs_ten_percent_more() {
        let mut u = usage(0, 1_000_000, 0, 0, 0);
        let global = message_cost("claude-opus-5", &u);
        u["inference_geo"] = "us".into();
        assert!((message_cost("claude-opus-5", &u) / global - 1.1).abs() < 1e-9);
    }

    fn line(id: &str, output: u64) -> String {
        format!("{{\"message\":{{\"id\":\"{id}\",\"model\":\"claude-opus-5\",\"usage\":{{\"output_tokens\":{output}}}}}}}\n")
    }

    #[test]
    fn counts_repeated_message_lines_once_and_reads_only_what_was_appended() {
        let dir = std::env::temp_dir().join(format!("agentboard-cost-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("t.jsonl");
        let mut tally = Tally::default();

        fs::write(&file, format!("{}{}{}", line("a", 1_000_000), line("a", 1_000_000), "{\"type\":\"user\"}\n")).unwrap();
        assert!((tally.update(&file) - 25.0).abs() < 1e-9);

        let half = line("b", 1_000_000);
        let (head, rest) = half.split_at(20);
        fs::write(&file, format!("{}{}{}{head}", line("a", 1_000_000), line("a", 1_000_000), "{\"type\":\"user\"}\n")).unwrap();
        assert!((tally.update(&file) - 25.0).abs() < 1e-9, "a half-written line must wait");

        fs::write(&file, format!("{}{}{}{head}{rest}", line("a", 1_000_000), line("a", 1_000_000), "{\"type\":\"user\"}\n")).unwrap();
        assert!((tally.update(&file) - 50.0).abs() < 1e-9);
        let _ = fs::remove_dir_all(dir);
    }
}
