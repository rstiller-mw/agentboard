use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

/// Long enough that a dismissed job stays gone in practice, short enough that the file never grows.
const TTL_MS: u64 = 30 * 24 * 3600 * 1000;

/// Where agentboard keeps its own state.
pub fn state_dir() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    state.join("agentboard")
}

/// Agents the user removed from the list, remembered on disk as `<epoch ms>\t<id>` lines until they expire.
pub struct Dismissed {
    path: PathBuf,
    entries: HashMap<String, u64>,
}

impl Dismissed {
    pub fn load(now_ms: u64) -> Dismissed {
        Dismissed::at(state_dir().join("dismissed"), now_ms)
    }

    fn at(path: PathBuf, now_ms: u64) -> Dismissed {
        let text = fs::read_to_string(&path).unwrap_or_default();
        Dismissed { path, entries: parse(&text, now_ms) }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    /// Remembers `id` and rewrites the file, which also drops every expired entry.
    pub fn add(&mut self, id: &str, now_ms: u64) -> io::Result<()> {
        self.entries.retain(|_, at| now_ms.saturating_sub(*at) < TTL_MS);
        self.entries.insert(id.to_owned(), now_ms);
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut lines: Vec<_> = self.entries.iter().map(|(id, at)| format!("{at}\t{id}\n")).collect();
        lines.sort();
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, lines.concat())?;
        fs::rename(&tmp, &self.path)
    }
}

fn parse(text: &str, now_ms: u64) -> HashMap<String, u64> {
    text.lines()
        .filter_map(|line| {
            let (at, id) = line.split_once('\t')?;
            Some((id.to_owned(), at.parse::<u64>().ok()?))
        })
        .filter(|(_, at)| now_ms.saturating_sub(*at) < TTL_MS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agentboard-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("nested/dismissed")
    }

    #[test]
    fn a_dismissed_id_is_remembered_across_loads() {
        let path = temp_file("remember");
        let mut d = Dismissed::at(path.clone(), 1_000);
        d.add("job-1", 1_000).unwrap();
        let again = Dismissed::at(path, 2_000);
        assert!(again.contains("job-1"));
        assert!(!again.contains("job-2"));
    }

    #[test]
    fn expired_entries_are_ignored_on_load_and_dropped_on_the_next_write() {
        let path = temp_file("expire");
        let mut d = Dismissed::at(path.clone(), 0);
        d.add("old", 0).unwrap();
        let later = TTL_MS + 1;
        let mut d = Dismissed::at(path.clone(), later);
        assert!(!d.contains("old"));
        d.add("new", later).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), format!("{later}\tnew\n"));
    }

    #[test]
    fn garbage_lines_are_skipped() {
        let entries = parse("no tab here\nabc\tbad-time\n5\tgood\n", 10);
        assert_eq!(entries.keys().collect::<Vec<_>>(), ["good"]);
    }
}
