//! Escape omissions belong to the input walk, independent of IR ownership.

use std::{cell::Cell, rc::Rc};

/// A bounded scanner reports completion independently of its readable events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum EscapeScanStatus {
    Complete,
    BudgetExceeded,
}

impl From<bool> for EscapeScanStatus {
    fn from(budget_exhausted: bool) -> Self {
        if budget_exhausted {
            Self::BudgetExceeded
        } else {
            Self::Complete
        }
    }
}

/// Live words and detached HEADs share one omission fact. Speculative table
/// candidates fork it and publish only after their content has been accepted.
#[derive(Clone, Debug, Default)]
pub(in crate::mandoc) struct EscapeCoverage(Rc<Cell<bool>>);

impl EscapeCoverage {
    pub(in crate::mandoc) fn record(&self, status: EscapeScanStatus) {
        if status == EscapeScanStatus::BudgetExceeded {
            self.0.set(true);
        }
    }

    pub(in crate::mandoc) fn truncated(&self) -> bool {
        self.0.get()
    }

    pub(in crate::mandoc) fn fork(&self) -> Self {
        Self(Rc::new(Cell::new(self.truncated())))
    }

    pub(in crate::mandoc) fn accept(&self, candidate: &Self) {
        if candidate.truncated() {
            self.0.set(true);
        }
    }
}
