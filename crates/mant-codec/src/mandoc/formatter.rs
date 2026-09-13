//! Explicit persistent state, separate from source services and local joins.
use libmandoc_rs::AuthorMode;

use super::inline::FontState;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::mandoc) enum AuthorFlow {
    #[default]
    Automatic,
    Split,
    NoSplit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FormatterState {
    pub(super) font: FontState,
    pub(super) spacing: bool,
    /// CVS `termp.skipvsp` is formatter-global: structural and presentation
    /// scopes do not clear it, while the next real formatter word does.
    pub(super) vertical_space_debt: u16,
    /// Bare CVS `TERMP_BACKAFTER` state.  A structural newline with no
    /// formatter cell does not clear it; the next real or generated word
    /// consumes it.  Tables are the deliberate exception and clear both
    /// backtracking flags before rendering each native cell.
    pub(super) zero_advance_armed: bool,
    /// CVS `TERMP_SPLIT`/`TERMP_NOSPLIT` persist across intervening inline
    /// nodes and nested block lowerers; they are not `.An` adjacency.
    author_flow: AuthorFlow,
}

impl Default for FormatterState {
    fn default() -> Self {
        Self {
            font: FontState::new(),
            spacing: true,
            vertical_space_debt: 0,
            zero_advance_armed: false,
            author_flow: AuthorFlow::default(),
        }
    }
}

impl FormatterState {
    /// Enter the body of a top-level mdoc AUTHORS section.
    ///
    /// CVS clears both renderer-global author switches at this exact point;
    /// subsections inherit the resulting mode and unrelated sections do not
    /// reset an explicitly selected mode.
    pub(super) fn enter_authors_section(&mut self) {
        self.author_flow = AuthorFlow::Automatic;
    }

    /// Execute one formatter word at a structural boundary.
    ///
    /// The visible spelling belongs to the renderer-neutral IR, but CVS
    /// still routes list markers and similar generated text through
    /// `term_word()`, which clears pending vertical-space debt.
    pub(super) fn execute_word(&mut self) {
        self.vertical_space_debt = 0;
        self.zero_advance_armed = false;
    }

    pub(super) fn clear_zero_advance(&mut self) {
        self.zero_advance_armed = false;
    }

    /// Execute one mdoc `.An` mode or name in source order.
    ///
    /// Returns whether the name starts a new formatter line.  The first
    /// ordinary name in AUTHORS enables splitting unless `-nosplit` is active.
    pub(super) fn execute_author(
        &mut self,
        mode: Option<AuthorMode>,
        authors_section: bool,
    ) -> bool {
        self.author_flow.execute(mode, authors_section)
    }

    pub(super) const fn author_flow(self) -> AuthorFlow {
        self.author_flow
    }

    pub(super) fn set_author_flow(&mut self, flow: AuthorFlow) {
        self.author_flow = flow;
    }
}

impl AuthorFlow {
    pub(in crate::mandoc) fn execute(
        &mut self,
        mode: Option<AuthorMode>,
        authors_section: bool,
    ) -> bool {
        match mode {
            Some(AuthorMode::Split) => {
                *self = Self::Split;
                false
            }
            Some(AuthorMode::NoSplit) => {
                *self = Self::NoSplit;
                false
            }
            None => {
                let breaks = *self == Self::Split;
                if authors_section && *self != Self::NoSplit {
                    *self = Self::Split;
                }
                breaks
            }
        }
    }
}
