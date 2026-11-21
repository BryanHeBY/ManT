//! One bounded quote/bracket/escape state for names and argument admission.

use super::{MAX_NESTING, closing, quote_closer};

#[derive(Default)]
pub(super) struct LexicalState {
    quote: Option<char>,
    closers: Vec<char>,
    escaped: bool,
    uncertain: bool,
    #[cfg(feature = "roff")]
    enclosures: Vec<u32>,
}

impl LexicalState {
    pub(super) fn is_top_level(&self) -> bool {
        !self.escaped && !self.within_group() && !self.uncertain
    }

    pub(super) fn within_group(&self) -> bool {
        self.group_depth() != 0
    }

    pub(super) fn group_depth(&self) -> usize {
        self.closers.len() + usize::from(self.quote.is_some()) + self.native_depth()
    }

    #[cfg(not(feature = "roff"))]
    const fn native_depth(&self) -> usize {
        0
    }

    #[cfg(feature = "roff")]
    fn native_depth(&self) -> usize {
        self.enclosures.len()
    }

    #[cfg(feature = "roff")]
    pub(super) fn observe_enclosure(&mut self, role: super::NativeOperandRole) -> Option<bool> {
        use super::NativeOperandRole as Role;
        match role {
            Role::EnclosureStart { body, .. } if self.group_depth() < MAX_NESTING => {
                self.enclosures.push(body);
                Some(false)
            }
            Role::EnclosureEnd(body) if self.enclosures.last() == Some(&body) => {
                self.enclosures.pop();
                Some(self.is_top_level())
            }
            Role::EnclosureStart { .. } | Role::EnclosureEnd(_) => {
                self.uncertain = true;
                Some(false)
            }
            _ => None,
        }
    }

    #[cfg(feature = "roff")]
    pub(super) fn inside_native_enclosure(&self) -> bool {
        !self.enclosures.is_empty()
    }

    /// Return whether this scalar completed one outer quoted/bracketed value.
    pub(super) fn observe(&mut self, character: char) -> bool {
        if self.escaped {
            self.escaped = false;
        } else if character == '\\' {
            self.escaped = true;
        } else if let Some(quote) = self.quote {
            if character == quote {
                self.quote = None;
                return self.closers.is_empty();
            }
        } else if let Some(quote) = quote_closer(character) {
            self.quote = Some(quote);
        } else if let Some(closer) = closing(character) {
            if self.closers.len() == MAX_NESTING {
                self.uncertain = true;
            } else {
                self.closers.push(closer);
            }
        } else if matches!(character, ']' | '}' | ')' | '>' | '⟩') {
            if self.closers.pop() == Some(character) {
                return self.closers.is_empty();
            }
            self.uncertain = true;
        }
        false
    }
}
