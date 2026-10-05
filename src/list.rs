use crate::agent::Agent;

/// The agents on screen, which one is selected and how far the list is scrolled.
pub struct List {
    agents: Vec<Agent>,
    pub hiding_finished: bool,
    selected: usize,
    selected_id: Option<String>,
    top: usize,
}

impl List {
    pub fn new(agents: Vec<Agent>) -> List {
        let mut list = List { agents, hiding_finished: false, selected: 0, selected_id: None, top: 0 };
        list.follow_selection();
        list
    }

    /// Replaces the agents; the selection stays on the same agent even when it moves or others come and go.
    pub fn replace(&mut self, agents: Vec<Agent>) {
        self.agents = agents;
        self.follow_selection();
    }

    pub fn visible(&self) -> Vec<&Agent> {
        self.agents.iter().filter(|a| !(self.hiding_finished && a.status.is_finished())).collect()
    }

    pub fn finished(&self) -> usize {
        self.agents.iter().filter(|a| a.status.is_finished()).count()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn current(&self) -> Option<&Agent> {
        self.visible().get(self.selected).copied()
    }

    pub fn move_by(&mut self, delta: isize) {
        self.selected = self.selected.saturating_add_signed(delta);
        self.remember_selection();
    }

    pub fn first(&mut self) {
        self.selected = 0;
        self.remember_selection();
    }

    pub fn last(&mut self) {
        self.selected = usize::MAX;
        self.remember_selection();
    }

    pub fn toggle_finished(&mut self) {
        self.hiding_finished = !self.hiding_finished;
        self.follow_selection();
    }

    /// Scroll offset that keeps the selection inside a window of `capacity` cards.
    pub fn top_for(&mut self, capacity: usize) -> usize {
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + capacity {
            self.top = self.selected + 1 - capacity;
        }
        self.top = self.top.min(self.visible().len().saturating_sub(capacity));
        self.top
    }

    fn follow_selection(&mut self) {
        let position = {
            let visible = self.visible();
            self.selected_id.as_ref().and_then(|id| visible.iter().position(|a| &a.id == id))
        };
        if let Some(i) = position {
            self.selected = i;
        }
        self.remember_selection();
    }

    fn remember_selection(&mut self) {
        let visible = self.visible();
        let selected = self.selected.min(visible.len().saturating_sub(1));
        let id = visible.get(selected).map(|a| a.id.clone());
        self.selected = selected;
        self.selected_id = id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Kind, Provider, Status};

    fn agent(id: &str, status: Status) -> Agent {
        Agent {
            id: id.into(),
            provider: Provider::Claude,
            kind: Kind::Interactive,
            status,
            name: id.into(),
            cwd: String::new(),
            detail: String::new(),
            created_ms: 0,
            cost: None,
            progress: None,
            pid: None,
            open: None,
        }
    }

    fn list(ids: &[&str]) -> List {
        List::new(ids.iter().map(|id| agent(id, Status::Idle)).collect())
    }

    fn selected_id(list: &List) -> String {
        list.current().unwrap().id.clone()
    }

    #[test]
    fn selection_stays_on_the_same_agent_when_one_is_inserted_above() {
        let mut l = list(&["a", "b", "c"]);
        l.move_by(1);
        l.replace(["a", "new", "b", "c"].iter().map(|id| agent(id, Status::Idle)).collect());
        assert_eq!((selected_id(&l), l.selected()), ("b".into(), 2));
    }

    #[test]
    fn selection_moves_to_the_nearest_position_when_its_agent_disappears() {
        let mut l = list(&["a", "b", "c"]);
        l.last();
        l.replace(vec![agent("a", Status::Idle), agent("b", Status::Idle)]);
        assert_eq!(selected_id(&l), "b");
    }

    #[test]
    fn an_empty_list_has_no_current_agent() {
        let mut l = list(&["a"]);
        l.replace(vec![]);
        assert!(l.current().is_none());
        l.move_by(1);
        l.last();
        assert_eq!(l.selected(), 0);
    }

    #[test]
    fn moving_is_clamped_to_both_ends() {
        let mut l = list(&["a", "b"]);
        l.move_by(-1);
        assert_eq!(selected_id(&l), "a");
        l.move_by(5);
        assert_eq!(selected_id(&l), "b");
        l.first();
        assert_eq!(selected_id(&l), "a");
    }

    #[test]
    fn hiding_finished_agents_moves_the_selection_off_a_hidden_one() {
        let mut l = List::new(vec![agent("a", Status::Idle), agent("b", Status::Done), agent("c", Status::Idle)]);
        l.move_by(1);
        l.toggle_finished();
        assert_eq!(l.visible().len(), 2);
        assert_eq!(selected_id(&l), "c");
        l.toggle_finished();
        assert_eq!(l.visible().len(), 3);
    }

    #[test]
    fn finished_agents_are_shown_by_default() {
        let l = List::new(vec![agent("a", Status::Done)]);
        assert_eq!((l.visible().len(), l.finished()), (1, 1));
    }

    #[test]
    fn scrolls_only_as_far_as_needed_to_keep_the_selection_visible() {
        let mut l = list(&["a", "b", "c", "d", "e"]);
        assert_eq!(l.top_for(2), 0);
        l.move_by(2);
        assert_eq!(l.top_for(2), 1);
        l.move_by(-1);
        assert_eq!(l.top_for(2), 1);
        l.move_by(-1);
        assert_eq!(l.top_for(2), 0);
        l.last();
        assert_eq!(l.top_for(2), 3);
    }
}
