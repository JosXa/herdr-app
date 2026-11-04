use super::{AppWindow, remember_pane};
use gpui::{App, Context, NavigationDirection, Window};
use serde_json::json;

#[derive(Clone, Default)]
pub(super) struct FocusHistory {
    past: Vec<String>,
    future: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visits_are_recent_unique_bounded_and_skip_closed_panes() {
        let mut history = FocusHistory::default();
        for pane in ["a", "b", "c", "a"] { history.visit(pane); }
        assert_eq!(history.past, ["b", "c", "a"]);
        assert_eq!(history.destination(NavigationDirection::Back, Some("a"), |pane| pane != "c").as_deref(), Some("b"));
        history.visit("b");
        assert_eq!(history.destination(NavigationDirection::Forward, Some("b"), |_| true).as_deref(), Some("a"));
        assert_eq!(history.destination(NavigationDirection::Back, Some("a"), |_| true).as_deref(), Some("b"));
        history.visit("new");
        assert_eq!(history.destination(NavigationDirection::Forward, Some("new"), |_| true), None);
        for pane in 0..100 { history.visit(&pane.to_string()); }
        assert_eq!(history.past.len(), 64);
    }
}

impl FocusHistory {
    pub(super) fn visit(&mut self, pane: &str) {
        if self.past.last().is_some_and(|last| last == pane) {
            return;
        }
        remember_pane(&mut self.past, pane);
        if self.past.len() > 64 {
            self.past.remove(0);
        }
        self.future.clear();
    }

    fn destination(&mut self, direction: NavigationDirection, current: Option<&str>, live: impl Fn(&str) -> bool) -> Option<String> {
        self.past.retain(|pane| live(pane));
        self.future.retain(|pane| live(pane));
        match direction {
            NavigationDirection::Back => {
                let destination = self.past.iter().rev().find(|pane| Some(pane.as_str()) != current)?.clone();
                self.past.retain(|pane| Some(pane.as_str()) != current);
                if let Some(current) = current.filter(|pane| live(pane)) {
                    self.future.retain(|pane| pane != current);
                    self.future.insert(0, current.to_owned());
                }
                Some(destination)
            }
            NavigationDirection::Forward => {
                self.future.retain(|pane| Some(pane.as_str()) != current);
                if self.future.is_empty() { return None; }
                let destination = self.future.remove(0);
                remember_pane(&mut self.past, &destination);
                Some(destination)
            }
        }
    }
}

impl AppWindow {
    pub(super) fn keyboard_pane(&self, window: &Window, cx: &App) -> Option<String> {
        if !window.is_window_active() { return None; }
        self.terms.iter().find(|(pane, term)| self.model.tab_of(pane).is_some() && term.view.read(cx).owns_keyboard_focus(window, cx)).map(|(pane, _)| pane.clone())
    }

    pub(super) fn navigate_history(&mut self, direction: NavigationDirection, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlays.iter().any(|overlay| overlay.kind.modal_keys()) { return; }
        let current = self.keyboard_pane(window, cx);
        let previous = self.focus_history.clone();
        let model = &self.model;
        let Some(pane) = self.focus_history.destination(direction, current.as_deref(), |pane| model.tab_of(pane).is_some()) else { return; };
        self.run("pane.focus", json!({"pane": pane}), window, cx);
        if self.keyboard_pane(window, cx).as_deref() != Some(&pane) {
            self.focus_history = previous;
        }
    }
}
