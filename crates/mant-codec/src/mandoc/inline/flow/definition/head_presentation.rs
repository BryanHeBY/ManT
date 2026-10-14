//! Route actual accepted row receipts back to a detached run-in HEAD owner.

use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use mant_ir::{Inline, RowLayoutHint};

use super::super::NativeWordAnchor;

type RowReceipt = (String, usize, RowLayoutHint);

struct PendingRows {
    owners: BTreeSet<String>,
    receipts: Vec<RowReceipt>,
}

/// One definition's HEAD/BODY ownership seam, never execution history.
/// The formatter still owns the sole buffer and decides at the actual flush;
/// this receiver records only accepted layout for the already detached HEAD.
pub(in crate::mandoc) struct DeferredHeadLayout(Rc<RefCell<PendingRows>>);

impl Clone for DeferredHeadLayout {
    fn clone(&self) -> Self {
        // Formatter clones are speculative forks. Their receipts must not
        // mutate the live traversal's detached HEAD owner.
        let source = self.0.borrow();
        Self(Rc::new(RefCell::new(PendingRows {
            owners: source.owners.clone(),
            receipts: source.receipts.clone(),
        })))
    }
}

impl DeferredHeadLayout {
    pub(in crate::mandoc::inline::flow) fn new(anchors: &[NativeWordAnchor]) -> Self {
        Self(Rc::new(RefCell::new(PendingRows {
            owners: anchors.iter().map(|anchor| anchor.owner.clone()).collect(),
            receipts: Vec::new(),
        })))
    }

    pub(in crate::mandoc) fn share(&self) -> Self {
        Self(Rc::clone(&self.0))
    }

    pub(in crate::mandoc::inline::flow) fn accept(&self, receipts: &[RowReceipt]) {
        let mut pending = self.0.borrow_mut();
        for receipt in receipts {
            if pending.owners.contains(&receipt.0) {
                pending.receipts.push(receipt.clone());
            }
        }
    }

    pub(in crate::mandoc) fn apply(&self, terms: &mut [Vec<Inline>]) {
        let receipts = std::mem::take(&mut self.0.borrow_mut().receipts);
        for term in terms {
            super::super::output::row_origins::project_row_presentations(term, &receipts, 0);
        }
    }
}

impl super::super::PreservedInlineState {
    pub(in crate::mandoc) fn deferred_head_layout(&self) -> Option<DeferredHeadLayout> {
        self.definition_field
            .as_ref()?
            .state
            .deferred_head_layout
            .as_ref()
            .map(DeferredHeadLayout::share)
    }
}
