//! Atomic response-local content closure for retained explanation topology.

use mant_ir::{ContentProjection, ContentProjectionBuilder, Document};
use mant_protocol::{
    EvidenceBasis, ExplanationContent, ExplanationEvidence, ExplanationSupport,
    ScopedExplanationEvidence,
};

use super::materialize::Budget;

/// Page-local projection admission. One instance per document shares selected
/// roots across supports, bodies and forms; scoped documents still debit the
/// same request budget.
pub(super) struct ProjectionAdmission<'a> {
    builder: Option<ContentProjectionBuilder<'a>>,
    reserved: usize,
}

impl<'a> ProjectionAdmission<'a> {
    pub(super) fn new(document: Option<&'a Document>) -> Self {
        Self {
            builder: document.map(|document| {
                ContentProjectionBuilder::new(
                    &document
                        .flow()
                        .expect("Fixed rejected by explanation preflight")
                        .content_store,
                )
            }),
            reserved: 0,
        }
    }

    /// Preflight a detached value and its projected store closure before the
    /// source body is cloned. Failed trials refund response bytes and selected
    /// objects, but retain already consumed projection work.
    pub(super) fn reserve(
        &mut self,
        budget: &mut Budget,
        value: &impl serde::Serialize,
        include: impl FnOnce(
            &mut ContentProjectionBuilder<'a>,
        ) -> Result<(), mant_ir::ContentProjectionError>,
    ) -> bool {
        let Some(builder) = &mut self.builder else {
            return false;
        };
        let Some(value_size) = Budget::size(value, budget.0) else {
            return false;
        };
        let Ok(checkpoint) = builder.checkpoint() else {
            return false;
        };
        let projection_size = (|| {
            include(builder).ok()?;
            if builder.can_reuse_snapshot() {
                Some(self.reserved)
            } else {
                let (projection, _) = builder.snapshot().ok()?;
                Budget::size(&projection, usize::MAX)
            }
        })();
        let Some(projection_size) = projection_size else {
            builder.rollback(checkpoint);
            return false;
        };
        let growth = projection_size.saturating_sub(self.reserved);
        let Some(charge) = value_size.checked_add(growth) else {
            builder.rollback(checkpoint);
            return false;
        };
        if !budget.charge(charge) {
            builder.rollback(checkpoint);
            return false;
        }
        builder.commit(checkpoint);
        self.reserved = self.reserved.max(projection_size);
        true
    }

    pub(super) fn reserved(&self) -> usize {
        self.reserved
    }
}

pub(super) fn attach(
    document: Option<&Document>,
    supports: &mut Vec<ExplanationSupport>,
    evidence: &mut [ExplanationEvidence],
    budget: &mut Budget,
    reserved: usize,
) -> Option<ContentProjection> {
    attach_filtered(document, supports, evidence.iter_mut(), budget, reserved)
}

pub(super) fn attach_scoped(
    document: Option<&Document>,
    supports: &mut Vec<ExplanationSupport>,
    evidence: &mut [ScopedExplanationEvidence],
    document_index: usize,
    budget: &mut Budget,
    reserved: usize,
) -> Option<ContentProjection> {
    attach_filtered(
        document,
        supports,
        evidence
            .iter_mut()
            .filter(|record| record.document_index == document_index)
            .map(|record| &mut record.evidence),
        budget,
        reserved,
    )
}

fn attach_filtered<'a>(
    document: Option<&Document>,
    supports: &mut Vec<ExplanationSupport>,
    evidence: impl Iterator<Item = &'a mut ExplanationEvidence>,
    budget: &mut Budget,
    reserved: usize,
) -> Option<ContentProjection> {
    let mut evidence = evidence.collect::<Vec<_>>();
    let has_topology = !supports.is_empty()
        || evidence
            .iter()
            .any(|record| record.entry.is_some() || record.content.is_some());
    if !has_topology {
        // No retained carrier can authorize a store-backed position.
        if evidence.iter().any(|record| {
            record.bases.iter().any(|basis| match basis {
                EvidenceBasis::Name { matches } => {
                    matches.iter().any(|item| !item.occurrences.is_empty())
                }
                EvidenceBasis::Form { matches } => {
                    matches.iter().any(|item| !item.occurrences.is_empty())
                }
                _ => false,
            }) || record
                .previews
                .iter()
                .any(|preview| !preview.content_ranges.is_empty())
        }) {
            omit_topology(supports, &mut evidence);
        }
        budget.refund(reserved);
        return None;
    }
    let Some(document) = document else {
        omit_topology(supports, &mut evidence);
        budget.refund(reserved);
        return None;
    };
    let projected = project(document, supports, &evidence);
    let Ok((projection, remap)) = projected else {
        omit_topology(supports, &mut evidence);
        budget.refund(reserved);
        return None;
    };
    let Some(size) = Budget::size(&projection, usize::MAX) else {
        omit_topology(supports, &mut evidence);
        budget.refund(reserved);
        return None;
    };
    if !budget.charge(size.saturating_sub(reserved)) {
        omit_topology(supports, &mut evidence);
        budget.refund(reserved);
        return None;
    }
    budget.refund(reserved.saturating_sub(size));
    if remap_topology(&remap, supports, &mut evidence).is_err() {
        // The response is still atomic: no partially remapped topology escapes.
        omit_topology(supports, &mut evidence);
        budget.refund(size);
        return None;
    }
    Some(projection)
}

fn project(
    document: &Document,
    supports: &[ExplanationSupport],
    evidence: &[&mut ExplanationEvidence],
) -> Result<(ContentProjection, mant_ir::ContentKeyRemap), mant_ir::ContentProjectionError> {
    let mut builder = ContentProjectionBuilder::new(
        &document
            .flow()
            .expect("Fixed rejected by explanation preflight")
            .content_store,
    );
    for support in supports {
        match support {
            ExplanationSupport::OwnedEntry { block }
            | ExplanationSupport::DeclarationGroup { block, .. } => {
                builder.include_blocks(std::slice::from_ref(block))?;
            }
            ExplanationSupport::ContainedDeclarationGroup { .. } => {}
        }
    }
    for record in evidence {
        if let Some(entry) = &record.entry {
            for form in &entry.forms {
                builder.include_inlines(form)?;
            }
        }
        if let Some(content) = &record.content {
            match content {
                ExplanationContent::Entry { block } | ExplanationContent::Block { block } => {
                    builder.include_blocks(std::slice::from_ref(block))?;
                }
                ExplanationContent::FixedOwner { .. }
                | ExplanationContent::SharedEntry { .. }
                | ExplanationContent::DeclarationMember { .. } => {}
            }
        }
    }
    builder.finish()
}

fn remap_topology(
    remap: &mant_ir::ContentKeyRemap,
    supports: &mut [ExplanationSupport],
    evidence: &mut [&mut ExplanationEvidence],
) -> Result<(), mant_ir::ContentProjectionError> {
    for support in supports {
        match support {
            ExplanationSupport::OwnedEntry { block }
            | ExplanationSupport::DeclarationGroup { block, .. } => {
                remap.remap_blocks(std::slice::from_mut(block))?;
            }
            ExplanationSupport::ContainedDeclarationGroup { .. } => {}
        }
    }
    for record in evidence {
        if let Some(entry) = &mut record.entry {
            for form in &mut entry.forms {
                remap.remap_inlines(form)?;
            }
        }
        if let Some(content) = &mut record.content {
            match content {
                ExplanationContent::Entry { block } | ExplanationContent::Block { block } => {
                    remap.remap_blocks(std::slice::from_mut(block))?;
                }
                ExplanationContent::FixedOwner { .. }
                | ExplanationContent::SharedEntry { .. }
                | ExplanationContent::DeclarationMember { .. } => {}
            }
        }
    }
    Ok(())
}

fn omit_topology(
    supports: &mut Vec<ExplanationSupport>,
    evidence: &mut [&mut ExplanationEvidence],
) {
    supports.clear();
    for record in evidence {
        // A response-local store is the only authority for retained inline
        // forms and body ranges.  Dropping that store must also drop every
        // position that used those targets, while preserving the textual
        // reason for the evidence itself.
        for basis in &mut record.bases {
            let matches = match basis {
                EvidenceBasis::Name { matches } => {
                    if matches.iter().any(|record| !record.occurrences.is_empty()) {
                        record.match_details_omitted = true;
                    }
                    for matched in matches {
                        matched.occurrences.clear();
                    }
                    continue;
                }
                EvidenceBasis::Form { matches } => matches,
                _ => continue,
            };
            if matches
                .iter()
                .any(|matched| !matched.occurrences.is_empty())
            {
                record.match_details_omitted = true;
            }
            for matched in matches {
                matched.occurrences.clear();
            }
        }
        for preview in &mut record.previews {
            if !preview.content_ranges.is_empty() {
                preview.content_ranges.clear();
                record.match_details_omitted = true;
            }
        }
        if record.support.take().is_some() {
            record.support_omitted = true;
        }
        if record.entry.take().is_some() {
            record.details_omitted = true;
        }
        if record.content.take().is_some() {
            record.content_omitted = true;
        }
    }
}
