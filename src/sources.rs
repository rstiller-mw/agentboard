use crate::agent::Agent;
use crate::{claude, scan};

/// Every agent from every source, oldest first so new ones append at the bottom.
pub fn load() -> Vec<Agent> {
    let mut agents = claude::load();
    agents.extend(scan::load());
    agents.sort_by(|a, b| a.created_ms.cmp(&b.created_ms).then_with(|| a.id.cmp(&b.id)));
    agents
}
