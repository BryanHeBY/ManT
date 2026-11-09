//! Route consumed row and text receipts back to a detached run-in HEAD owner.

use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use mant_ir::{Inline, RowLayoutHint};

use super::super::NativeWordAnchor;
use super::super::field_buffer::DeferredSourceBlank;
use super::super::output::tab_projection::{self, TabExpansion};
use super::super::output::{INTERNAL_FIELD_WORD, deferred_blanks};

type RowReceipt = (String, usize, RowLayoutHint);

struct PendingRows {
    owners: BTreeSet<String>,
    receipts: Vec<RowReceipt>,
    tabs: Vec<TabExpansion>,
    blanks: Vec<(DeferredSourceBlank, usize)>,
    operands: Option<Rc<RefCell<super::super::HeadOperandCapture>>>,
}

/// One definition's HEAD/BODY ownership seam, never execution history.
/// The formatter still owns the sole buffer and decides at the actual flush;
/// this receiver records only accepted projection for the already detached HEAD.
pub(in crate::mandoc) struct DeferredHeadProjection(Rc<RefCell<PendingRows>>);

impl Clone for DeferredHeadProjection {
    fn clone(&self) -> Self {
        // Formatter clones are speculative forks. Their receipts must not
        // mutate the live traversal's detached HEAD owner.
        let source = self.0.borrow();
        Self(Rc::new(RefCell::new(PendingRows {
            owners: source.owners.clone(),
            receipts: source.receipts.clone(),
            tabs: source.tabs.clone(),
            blanks: source.blanks.clone(),
            operands: source
                .operands
                .as_ref()
                .map(|capture| Rc::new(RefCell::new(capture.borrow().clone()))),
        })))
    }
}

impl DeferredHeadProjection {
    pub(in crate::mandoc::inline::flow) fn new(
        anchors: &[NativeWordAnchor],
        operands: Option<Rc<RefCell<super::super::HeadOperandCapture>>>,
    ) -> Self {
        Self(Rc::new(RefCell::new(PendingRows {
            owners: anchors.iter().map(|anchor| anchor.owner.clone()).collect(),
            receipts: Vec::new(),
            tabs: Vec::new(),
            blanks: Vec::new(),
            operands,
        })))
    }

    pub(in crate::mandoc) fn share(&self) -> Self {
        Self(Rc::clone(&self.0))
    }

    pub(in crate::mandoc::inline::flow) fn owns_word(&self, owner: &str) -> bool {
        self.0.borrow().owners.contains(owner)
    }

    pub(in crate::mandoc::inline::flow) fn accept(&self, receipts: &[RowReceipt]) {
        let mut pending = self.0.borrow_mut();
        for receipt in receipts {
            if pending.owners.contains(&receipt.0) {
                pending.receipts.push(receipt.clone());
            }
        }
    }

    pub(in crate::mandoc) fn apply(
        &self,
        terms: &mut [Vec<Inline>],
    ) -> Option<crate::definitions::CapturedHeadOperands> {
        let mut pending = self.0.borrow_mut();
        let receipts = std::mem::take(&mut pending.receipts);
        let tabs = std::mem::take(&mut pending.tabs);
        let blanks = std::mem::take(&mut pending.blanks);
        let operands = pending.operands.clone();
        drop(pending);
        if let Some(capture) = &operands {
            let _ = capture.borrow_mut().take();
        }
        for term in terms.iter_mut() {
            // Detached term vectors own no live builder/execution addresses.
            let _ =
                super::super::output::row_origins::project_row_presentations(term, &receipts, 0);
        }
        tab_projection::apply(terms, &tabs);
        deferred_blanks::apply(terms, &blanks);
        for term in terms {
            if let Some(capture) = &operands {
                capture.borrow_mut().observe_accepted_output(term);
            }
        }
        operands.map(|capture| capture.borrow_mut().take())
    }

    pub(in crate::mandoc::inline::flow) fn accept_tabs(&self, tabs: &[TabExpansion]) {
        let mut pending = self.0.borrow_mut();
        for tab in tabs {
            if pending.owners.contains(&tab.owner) {
                pending.tabs.push(tab.clone());
            }
        }
    }

    pub(in crate::mandoc::inline::flow) fn accept_blanks(
        &self,
        blanks: &[(DeferredSourceBlank, usize)],
    ) {
        let mut pending = self.0.borrow_mut();
        for &(blank, count) in blanks {
            if pending
                .owners
                .contains(&format!("{INTERNAL_FIELD_WORD}{}", blank.owner))
            {
                pending.blanks.push((blank, count));
            }
        }
    }
}

impl super::super::PreservedInlineState {
    pub(in crate::mandoc) fn deferred_head_projection(&self) -> Option<DeferredHeadProjection> {
        self.definition_field
            .as_ref()?
            .state
            .deferred_head_projection
            .as_ref()
            .map(DeferredHeadProjection::share)
    }
}
