//! Optional native link membership, separate from the document wire.
use crate::{ContentLocation, Inline, LinkTarget};
use std::ops::Range;

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
