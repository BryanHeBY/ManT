//! Optional native link membership, separate from the document wire.
use crate::{ContentLocation, Inline, LinkTarget, ResolvedContent};
use std::{ops::Range, sync::Arc};

/// Accepted native membership for one exact decoded document snapshot.
/// This value is separate from all existing document and reference JSON.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkLayout {
    /// Document-order receipts for eligible legacy link fragments.
    pub receipts: Vec<LinkLayoutReceipt>,
}

/// Proven ownership inside one legacy link wrapper.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkLayoutReceipt {
    /// Final structural location of the original legacy wrapper.
    pub origin: ContentLocation,
    /// Authored occurrence identity local to the paired document.
    pub occurrence: u32,
    /// Exact legacy target; equal targets do not imply equal authors.
    pub target: LinkTarget,
    /// Exact typed child snapshot, including styles and zero-width nodes.
    pub expected_children: Vec<Inline>,
    /// Ordered disjoint half-open scalar ranges in the label. A hard line
    /// counts as one scalar, and zero-width anchors count as none.
    pub members: Vec<Range<usize>>,
}

/// An immutable source resolution and its optional native link facts.
///
/// The low-level codec produces a raw [`LinkLayout`] for the loader to bind
/// once. A sealed result does not expose that producer carrier for rebinding:
///
/// ```compile_fail
/// # use mant_ir::SourceBoundLinkLayout;
/// fn splice(a: &SourceBoundLinkLayout, b: &SourceBoundLinkLayout) {
///     let _ = SourceBoundLinkLayout::from_producer(a.content_arc(), b.clone());
/// }
/// ```
#[derive(Clone, Debug)]
pub struct SourceBoundLinkLayout {
    content: Arc<ResolvedContent>,
    layout: LinkLayout,
}

impl SourceBoundLinkLayout {
    /// Producer handoff only: bind the unsealed codec output to its actual
    /// immutable resolved allocation. Do not pass a layout from another decode.
    #[doc(hidden)]
    #[must_use]
    pub const fn from_producer(content: Arc<ResolvedContent>, layout: LinkLayout) -> Self {
        Self { content, layout }
    }

    /// The exact content from which these facts were derived.
    #[must_use]
    pub fn content(&self) -> &ResolvedContent {
        &self.content
    }

    /// Share the exact resolution without rebuilding its document.
    #[must_use]
    pub fn content_arc(&self) -> Arc<ResolvedContent> {
        Arc::clone(&self.content)
    }

    /// Presentation facts are valid only for the exact decoded allocation.
    #[must_use]
    pub fn receipts_for(&self, content: &Arc<ResolvedContent>) -> Option<&[LinkLayoutReceipt]> {
        Arc::ptr_eq(&self.content, content).then_some(self.layout.receipts.as_slice())
    }

    /// Borrow source facts for this sealed resolution.
    #[must_use]
    pub fn receipts(&self) -> &[LinkLayoutReceipt] {
        &self.layout.receipts
    }
}
