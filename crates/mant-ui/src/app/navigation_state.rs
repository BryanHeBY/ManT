//! Document navigation ledger. Plans borrow/clone locations; only a verified
//! page transition commits history and tab state.

use std::sync::Arc;

use mant_ir::ResolvedContent;
use mant_protocol::DocumentAddress;

use super::HISTORY_LIMIT;

/// A reference occurrence is private to a scanned document view. Unlike a
/// fragment, it is not a public content selector. Reloading still requires
/// the existing candidate-view validation, not a claim of revision identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) enum LocalTarget {
    #[default]
    Default,
    Fragment(String),
    ReferenceOccurrence(String),
}

impl LocalTarget {
    pub(super) fn from_fragment(fragment: Option<String>) -> Self {
        fragment.map_or(Self::Default, Self::Fragment)
    }

    pub(super) fn id(&self) -> Option<&str> {
        match self {
            Self::Default => None,
            Self::Fragment(id) | Self::ReferenceOccurrence(id) => Some(id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HistoryDirection {
    New,
    Back,
    Forward,
}

#[derive(Debug, Clone)]
pub(super) struct HistoryLocation {
    address: Option<DocumentAddress>,
    fallback: Option<Arc<ResolvedContent>>,
    target: LocalTarget,
}

impl HistoryLocation {
    pub(super) fn address(&self) -> Option<&DocumentAddress> {
        self.address.as_ref()
    }

    pub(super) fn fallback(&self) -> Option<&Arc<ResolvedContent>> {
        self.fallback.as_ref()
    }

    pub(super) fn target(&self) -> &LocalTarget {
        &self.target
    }
}

#[derive(Debug, Clone)]
pub(super) struct DocumentTab {
    location: HistoryLocation,
    label: String,
}

impl DocumentTab {
    pub(super) fn label(&self) -> &str {
        &self.label
    }

    #[cfg(test)]
    pub(super) fn address(&self) -> Option<&DocumentAddress> {
        self.location.address()
    }
}

pub(super) struct NavigationState {
    address: Option<DocumentAddress>,
    fallback: Option<Arc<ResolvedContent>>,
    back: Vec<HistoryLocation>,
    forward: Vec<HistoryLocation>,
    tabs: Vec<DocumentTab>,
    active_tab: usize,
    tab_scroll: usize,
    tab_visibility_target: Option<usize>,
    tab_view_width: u16,
}

impl NavigationState {
    pub(super) fn new(
        address: Option<DocumentAddress>,
        fallback: Option<Arc<ResolvedContent>>,
    ) -> Self {
        Self {
            address,
            fallback,
            back: Vec::new(),
            forward: Vec::new(),
            tabs: Vec::new(),
            active_tab: 0,
            tab_scroll: 0,
            tab_visibility_target: Some(0),
            tab_view_width: 0,
        }
    }

    pub(super) fn address(&self) -> Option<&DocumentAddress> {
        self.address.as_ref()
    }

    pub(super) fn location(&self, target: LocalTarget) -> HistoryLocation {
        HistoryLocation {
            address: self.address.clone(),
            fallback: self.fallback.clone(),
            target,
        }
    }

    pub(super) fn plan_history(&self, back: bool) -> Option<(HistoryLocation, HistoryDirection)> {
        let (history, direction) = if back {
            (&self.back, HistoryDirection::Back)
        } else {
            (&self.forward, HistoryDirection::Forward)
        };
        history
            .last()
            .cloned()
            .map(|location| (location, direction))
    }

    pub(super) fn plan_tab(&self, index: usize) -> Option<HistoryLocation> {
        (index != self.active_tab)
            .then(|| self.tabs.get(index).map(|tab| tab.location.clone()))
            .flatten()
    }

    /// Called only after the App has validated the candidate and its local
    /// destination. Planning a host request never consumes a history entry.
    pub(super) fn commit(&mut self, direction: HistoryDirection, current: HistoryLocation) {
        match direction {
            HistoryDirection::New => {
                push_history(&mut self.back, current);
                self.forward.clear();
            }
            HistoryDirection::Back => {
                self.back.pop();
                push_history(&mut self.forward, current);
            }
            HistoryDirection::Forward => {
                self.forward.pop();
                push_history(&mut self.back, current);
            }
        }
    }

    pub(super) fn replace_current(
        &mut self,
        address: Option<DocumentAddress>,
        fallback: Option<Arc<ResolvedContent>>,
    ) {
        self.address = address;
        self.fallback = fallback;
    }

    pub(super) fn sync_tab(&mut self, label: String, fallback: Option<Arc<ResolvedContent>>) {
        let existing = self
            .tabs
            .iter()
            .position(|tab| tab.location.address == self.address);
        let index = if let Some(index) = existing {
            let tab = &mut self.tabs[index];
            tab.label = label;
            tab.location.fallback = fallback;
            index
        } else {
            if self.tabs.len() == HISTORY_LIMIT {
                self.tabs.remove(0);
                self.active_tab = self.active_tab.saturating_sub(1);
                self.tab_scroll = self.tab_scroll.saturating_sub(1);
            }
            self.tabs.push(DocumentTab {
                location: HistoryLocation {
                    address: self.address.clone(),
                    fallback,
                    target: LocalTarget::Default,
                },
                label,
            });
            self.tabs.len() - 1
        };
        self.active_tab = index;
        self.tab_visibility_target = Some(index);
    }

    pub(super) fn remember_tab(&mut self, target: LocalTarget) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.location.target = target;
        }
    }

    pub(super) fn tabs(&self) -> &[DocumentTab] {
        &self.tabs
    }

    pub(super) const fn active_tab(&self) -> usize {
        self.active_tab
    }

    pub(super) fn scroll_tabs(&mut self, direction: isize) -> bool {
        let next = self
            .tab_scroll
            .saturating_add_signed(direction)
            .min(self.tabs.len().saturating_sub(1));
        if next == self.tab_scroll {
            return false;
        }
        self.tab_scroll = next;
        self.tab_visibility_target = None;
        true
    }

    pub(super) fn prepare_tab_view(&mut self, width: u16) -> (usize, Option<usize>) {
        if self.tab_view_width != width {
            self.tab_view_width = width;
            self.tab_visibility_target = Some(self.active_tab);
        }
        (self.tab_scroll, self.tab_visibility_target.take())
    }

    pub(super) fn commit_tab_scroll(&mut self, start: usize) {
        self.tab_scroll = start;
    }

    #[cfg(test)]
    pub(super) fn history_lengths(&self) -> (usize, usize) {
        (self.back.len(), self.forward.len())
    }
}

fn push_history(history: &mut Vec<HistoryLocation>, location: HistoryLocation) {
    if history.len() == HISTORY_LIMIT {
        history.remove(0);
    }
    history.push(location);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(name: &str) -> DocumentAddress {
        DocumentAddress::Manual {
            name: name.into(),
            manual_section: "1".into(),
        }
    }

    #[test]
    fn planning_history_and_tabs_retains_each_local_target_until_commit() {
        for target in [
            LocalTarget::Default,
            LocalTarget::Fragment("Mixed.Target".into()),
            LocalTarget::ReferenceOccurrence("private-occurrence-2".into()),
        ] {
            let mut state = NavigationState::new(Some(address("first")), None);
            state.sync_tab("First".into(), None);
            state.remember_tab(target.clone());
            state.commit(HistoryDirection::New, state.location(target.clone()));
            state.replace_current(Some(address("second")), None);
            state.sync_tab("Second".into(), None);

            // Repeated plans, including abandoned host requests, are read-only.
            for _ in 0..2 {
                let (location, direction) = state.plan_history(true).unwrap();
                assert_eq!(direction, HistoryDirection::Back);
                assert_eq!(location.address(), Some(&address("first")));
                assert_eq!(location.target(), &target);
                assert_eq!(state.plan_tab(0).unwrap().target(), &target);
                assert_eq!(state.history_lengths(), (1, 0));
                assert_eq!(state.active_tab(), 1);
                assert_eq!(state.address(), Some(&address("second")));
            }
            assert!(state.plan_tab(1).is_none());
            assert!(state.plan_tab(99).is_none());
            state.commit(HistoryDirection::Back, state.location(LocalTarget::Default));
            assert_eq!(state.history_lengths(), (0, 1));
            let (forward, direction) = state.plan_history(false).unwrap();
            assert_eq!(direction, HistoryDirection::Forward);
            assert_eq!(forward.target(), &LocalTarget::Default);
            assert_eq!(forward.address(), Some(&address("second")));
        }
    }
}
