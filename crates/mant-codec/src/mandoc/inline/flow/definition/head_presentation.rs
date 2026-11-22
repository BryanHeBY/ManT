//! Route consumed row and text receipts back to a detached run-in HEAD owner.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use mant_ir::{Inline, RowLayoutHint};

use super::super::NativeWordAnchor;
use super::super::field_buffer::{DeferredSourceBlank, RewoundGlyph};
use super::super::output::native_passes::OwnerAcceptance;
use super::super::output::tab_projection::{self, TabExpansion};
use super::super::output::{INTERNAL_FIELD_WORD, deferred_blanks};

type RowReceipt = (String, usize, RowLayoutHint);

struct PendingRows {
    owners: BTreeSet<String>,
    receipts: Vec<RowReceipt>,
    hard_rows: BTreeMap<String, Vec<usize>>,
    tabs: Vec<TabExpansion>,
    blanks: Vec<(DeferredSourceBlank, usize)>,
    coverage: BTreeMap<String, OwnerAcceptance>,
    rewinds: Vec<(String, RewoundGlyph)>,
    terms: Option<Rc<Vec<Vec<Inline>>>>,
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
            hard_rows: source.hard_rows.clone(),
            tabs: source.tabs.clone(),
            blanks: source.blanks.clone(),
            coverage: source.coverage.clone(),
            rewinds: source.rewinds.clone(),
            terms: source.terms.clone(),
            operands: source
                .operands
                .as_ref()
                .map(|capture| Rc::new(RefCell::new(capture.borrow().clone()))),
        })))
    }
}

impl DeferredHeadProjection {
    pub(super) fn operands(&self) -> Option<Rc<RefCell<super::super::HeadOperandCapture>>> {
        self.0.borrow().operands.clone()
    }

    pub(in crate::mandoc::inline::flow) fn new(
        anchors: &[NativeWordAnchor],
        operands: Option<Rc<RefCell<super::super::HeadOperandCapture>>>,
    ) -> Self {
        Self(Rc::new(RefCell::new(PendingRows {
            owners: anchors.iter().map(|anchor| anchor.owner.clone()).collect(),
            receipts: Vec::new(),
            hard_rows: BTreeMap::new(),
            tabs: Vec::new(),
            blanks: Vec::new(),
            coverage: BTreeMap::new(),
            rewinds: Vec::new(),
            terms: None,
            operands,
        })))
    }

    pub(in crate::mandoc) fn share(&self) -> Self {
        Self(Rc::clone(&self.0))
    }

    pub(in crate::mandoc::inline::flow) fn owns_word(&self, owner: &str) -> bool {
        self.0.borrow().owners.contains(owner)
    }

    pub(in crate::mandoc) fn suspend_terms(&self, terms: &mut Vec<Vec<Inline>>) {
        self.0.borrow_mut().terms = Some(Rc::new(std::mem::take(terms)));
    }

    pub(in crate::mandoc) fn resume_terms(
        &self,
        terms: &mut Vec<Vec<Inline>>,
    ) -> Option<crate::definitions::CapturedHeadOperands> {
        if let Some(saved) = self.0.borrow_mut().terms.take() {
            *terms = Rc::unwrap_or_clone(saved);
        }
        self.apply(terms)
    }

    pub(in crate::mandoc::inline::flow) fn restore_pending_ink(
        &self,
        owner: u64,
        glyphs: &mut Vec<Inline>,
    ) {
        let mut pending = self.0.borrow_mut();
        if !pending
            .owners
            .contains(&format!("{INTERNAL_FIELD_WORD}{owner}"))
            || glyphs.is_empty()
        {
            return;
        }
        let Some(terms) = pending.terms.as_mut().map(Rc::make_mut) else {
            return;
        };
        // The generated word writes a backspace after the final native
        // HEAD glyph. Keep its projection with the other scalars of that
        // owner until the BODY flush decides if the column still shows it.
        if let Some(term) = terms.last_mut() {
            term.append(glyphs);
        } else {
            terms.push(std::mem::take(glyphs));
        }
    }

    pub(in crate::mandoc::inline::flow) fn relocate_ink(
        &self,
        owner: &str,
        glyph: &RewoundGlyph,
    ) -> Option<Inline> {
        // The original label is suspended while its BODY is running.
        // A proved later device position moves an authored glyph out of
        // that label exactly once, with its original style and link.
        let mut pending = self.0.borrow_mut();
        let terms = Rc::make_mut(pending.terms.as_mut()?);
        terms.iter_mut().rev().find_map(|term| {
            super::super::output::source_blanks::take_rewound_glyph(
                term,
                &mut [],
                owner,
                glyph,
                true,
            )
        })
    }

    pub(in crate::mandoc::inline::flow) fn accept(&self, receipts: &[RowReceipt]) {
        let mut pending = self.0.borrow_mut();
        for receipt in receipts {
            if pending.owners.contains(&receipt.0) {
                pending.receipts.push(receipt.clone());
            }
        }
    }

    pub(in crate::mandoc::inline::flow) fn accept_hard_row(&self, owner: &str, offset: usize) {
        self.0
            .borrow_mut()
            .hard_rows
            .entry(owner.to_owned())
            .or_default()
            .push(offset);
    }

    pub(in crate::mandoc) fn apply(
        &self,
        terms: &mut [Vec<Inline>],
    ) -> Option<crate::definitions::CapturedHeadOperands> {
        let mut pending = self.0.borrow_mut();
        let receipts = std::mem::take(&mut pending.receipts);
        let hard_rows = std::mem::take(&mut pending.hard_rows);
        let tabs = std::mem::take(&mut pending.tabs);
        let blanks = std::mem::take(&mut pending.blanks);
        let coverage = std::mem::take(&mut pending.coverage);
        let rewinds = std::mem::take(&mut pending.rewinds);
        let operands = pending.operands.clone();
        drop(pending);
        if let Some(capture) = &operands {
            let _ = capture.borrow_mut().take();
        }
        for (owner, glyph) in rewinds {
            for term in terms.iter_mut().rev() {
                if super::super::output::source_blanks::take_rewound_glyph(
                    term,
                    &mut [],
                    &owner,
                    &glyph,
                    true,
                )
                .is_some()
                {
                    break;
                }
            }
        }
        for term in terms.iter_mut() {
            if !hard_rows.is_empty() {
                // Breaks use provisional owner positions, just as they
                // do in the live BODY. Apply before removing covered ink.
                *term = super::super::output::native_passes::split_retired_native_field_passes(
                    term, &hard_rows,
                );
            }
            super::super::output::split::remove_covered_native_glyphs(term, &coverage);
            // Detached term vectors own no live builder/execution addresses.
            let _ =
                super::super::output::row_origins::project_row_presentations(term, &receipts, 0);
        }
        tab_projection::apply(terms, &tabs);
        deferred_blanks::apply(terms, &blanks);
        for term in terms {
            super::super::output::reconcile_painted_order(term);
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

    pub(in crate::mandoc::inline::flow) fn accept_coverage(
        &self,
        accepted: &BTreeMap<String, OwnerAcceptance>,
    ) {
        let mut pending = self.0.borrow_mut();
        for (owner, acceptance) in accepted {
            if pending.owners.contains(owner) && acceptance.has_ink_edits() {
                pending.coverage.insert(owner.clone(), acceptance.clone());
            }
        }
    }

    pub(in crate::mandoc::inline::flow) fn accept_rewind(&self, owner: &str, glyph: &RewoundGlyph) {
        self.0
            .borrow_mut()
            .rewinds
            .push((owner.to_owned(), glyph.clone()));
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
