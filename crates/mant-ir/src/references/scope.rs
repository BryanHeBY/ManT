//! Public root selection and scan entry points over one authoritative walker.
use super::{
    LinkOccurrenceRef, NavigationEvent, NavigationScanOptions, ReferenceScanLimits,
    ReferenceScanReport, ReferenceWorkBudget,
};
use crate::{ContentBlockStep as Step, Document, EntryOwnerLocationRef};
use std::ops::ControlFlow;

/// Scan all authoritative roots in document order without retaining an index.
/// A callback can stop before the next node or path is inspected/materialized.
pub fn scan_references<'ir>(
    document: &'ir Document,
    limits: ReferenceScanLimits,
    mut visit: impl for<'path> FnMut(LinkOccurrenceRef<'ir, 'path>) -> ControlFlow<()>,
) -> ReferenceScanReport {
    scan_reference_scope(
        document,
        ReferenceScope::Document,
        limits,
        |occurrence, _| visit(occurrence),
    )
}

/// Select an existing source range without looking up semantic names.
#[derive(Debug, Clone, Copy)]
pub enum ReferenceScope<'a> {
    /// Heading, root content and every section.
    Document,
    /// Document heading and root blocks, excluding all sections.
    Overview,
    /// One section and its descendants.
    Section(&'a [u32]),
    /// One block and its descendants.
    Block {
        /// Section path; empty selects document-root content.
        sections: &'a [u32],
        /// Typed block path.
        blocks: &'a [Step],
    },
    /// One list or definition item, with its original terms and body.
    Owner(EntryOwnerLocationRef<'a>),
}

/// Scan an explicit source range with one shared traversal/association budget.
/// The callback may run bounded form association using the supplied account.
pub fn scan_reference_scope<'ir>(
    document: &'ir Document,
    scope: ReferenceScope<'_>,
    limits: ReferenceScanLimits,
    mut visit: impl for<'path> FnMut(
        LinkOccurrenceRef<'ir, 'path>,
        &mut ReferenceWorkBudget,
    ) -> ControlFlow<()>,
) -> ReferenceScanReport {
    scan_navigation_scope(
        document,
        scope,
        limits,
        NavigationScanOptions::default(),
        |event, budget| {
            if let NavigationEvent::Link(occurrence) = event {
                visit(occurrence, budget)
            } else {
                ControlFlow::Continue(())
            }
        },
    )
}

/// Scan requested navigation facts with a single shared work account and DFS.
/// Unrequested target aliases and semantic relations are never inspected.
pub fn scan_navigation_scope<'ir>(
    document: &'ir Document,
    scope: ReferenceScope<'_>,
    limits: ReferenceScanLimits,
    options: NavigationScanOptions,
    visit: impl for<'path> FnMut(
        NavigationEvent<'ir, 'path>,
        &mut ReferenceWorkBudget,
    ) -> ControlFlow<()>,
) -> ReferenceScanReport {
    let mut budget = ReferenceWorkBudget::new(limits);
    scan_navigation_scope_with_budget(document, scope, &mut budget, options, visit)
}

/// Continue an operation's existing budget, for example a single bounded local
/// target check after collecting a reference page. Coverage steps/bytes are
/// cumulative; occurrences belong only to this scan.
pub fn scan_navigation_scope_with_budget<'ir>(
    document: &'ir Document,
    scope: ReferenceScope<'_>,
    budget: &mut ReferenceWorkBudget,
    options: NavigationScanOptions,
    visit: impl for<'path> FnMut(
        NavigationEvent<'ir, 'path>,
        &mut ReferenceWorkBudget,
    ) -> ControlFlow<()>,
) -> ReferenceScanReport {
    super::walker::scan(document, scope, budget, options, visit)
}

/// Scan one explicit section subtree, independent of entry filters.
/// The initial section path is charged and checked before copying it.
pub fn scan_section_references<'ir>(
    document: &'ir Document,
    sections: &[u32],
    limits: ReferenceScanLimits,
    mut visit: impl for<'path> FnMut(LinkOccurrenceRef<'ir, 'path>) -> ControlFlow<()>,
) -> ReferenceScanReport {
    scan_reference_scope(
        document,
        ReferenceScope::Section(sections),
        limits,
        |occurrence, _| visit(occurrence),
    )
}

/// Scan one original block subtree, preserving its ancestor item ownership.
/// Root lookup consumes the same work/depth budget as ordinary traversal.
pub fn scan_block_references<'ir>(
    document: &'ir Document,
    sections: &[u32],
    blocks: &[Step],
    limits: ReferenceScanLimits,
    mut visit: impl for<'path> FnMut(LinkOccurrenceRef<'ir, 'path>) -> ControlFlow<()>,
) -> ReferenceScanReport {
    scan_reference_scope(
        document,
        ReferenceScope::Block { sections, blocks },
        limits,
        |occurrence, _| visit(occurrence),
    )
}

/// Scan a precisely addressed item's terms and body, without sibling items.
/// Semantic filters and aliases do not participate in locating this root.
pub fn scan_owner_references<'ir>(
    document: &'ir Document,
    owner: EntryOwnerLocationRef<'_>,
    limits: ReferenceScanLimits,
    mut visit: impl for<'path> FnMut(LinkOccurrenceRef<'ir, 'path>) -> ControlFlow<()>,
) -> ReferenceScanReport {
    scan_reference_scope(
        document,
        ReferenceScope::Owner(owner),
        limits,
        |occurrence, _| visit(occurrence),
    )
}
