//! Stable scan limits, coverage and borrowed occurrence contracts.
use crate::{
    ContentLocationRef, EntryOwner, EntryOwnerLocationRef, Inline, LinkTarget, MAX_CONTENT_DEPTH,
    SourceSpan,
};

/// Default work units across traversal and optional label/form inspection.
pub const DEFAULT_REFERENCE_SCAN_STEPS: usize = 250_000;
/// Hard work ceiling, independent of the source-file size limit.
pub const MAX_REFERENCE_SCAN_STEPS: usize = 1_000_000;
/// Default bytes inspected in link targets and labels.
pub const DEFAULT_REFERENCE_SCAN_BYTES: usize = 8 * 1024 * 1024;
/// Hard ceiling for target/label inspection.
pub const MAX_REFERENCE_SCAN_BYTES: usize = 32 * 1024 * 1024;

/// Limits apply before inspecting or retaining another node's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceScanLimits {
    /// Traversal steps, including containers, skipped links and optional label/form inspection.
    pub steps: usize,
    /// Combined section/block/inline structural depth, clamped to 256.
    pub depth: usize,
    /// Inspected target/label/form bytes; no text is copied by the base scan.
    pub bytes: usize,
}

impl Default for ReferenceScanLimits {
    fn default() -> Self {
        Self {
            steps: DEFAULT_REFERENCE_SCAN_STEPS,
            depth: MAX_CONTENT_DEPTH,
            bytes: DEFAULT_REFERENCE_SCAN_BYTES,
        }
    }
}

/// Why scanning stopped before visiting all requested content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceScanStop {
    /// The consumer explicitly stopped; no subsequent node was inspected.
    Visitor,
    /// Work-unit budget exhausted.
    Steps,
    /// Structural or inline nesting exceeded its limit.
    Depth,
    /// Target/label byte budget exhausted.
    Bytes,
    /// An occurrence position exceeds its encoded-size ceiling.
    Position,
    /// A requested subtree coordinate is invalid for this snapshot.
    InvalidRoot,
}

/// Operation coverage, not a cached whole-document inventory.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceScanReport {
    /// Charged traversal and optional label/form-inspection work units.
    pub steps: usize,
    /// Charged target/label/form bytes; repeated inspections also consume budget.
    pub bytes: usize,
    /// Real occurrences delivered to the consumer; no target deduplication.
    pub occurrences: usize,
    /// Absent only after a complete scan of the selected root.
    pub stopped: Option<ReferenceScanStop>,
}

impl ReferenceScanReport {
    /// Whether the selected source range was fully scanned.
    #[must_use]
    pub const fn complete(self) -> bool {
        self.stopped.is_none()
    }
}

/// An actual content item and its source-neutral address.
#[derive(Debug, Clone, Copy)]
pub struct ReferenceOwnerRef<'ir, 'path> {
    /// Borrowed original item; attached semantic facts are not revalidated here.
    pub owner: EntryOwner<'ir>,
    /// Exact structural item position, not a semantic selector.
    pub location: EntryOwnerLocationRef<'path>,
}

/// One link and ephemeral structural context, borrowing only final IR.
///
/// Form association is intentionally uninspected at this layer. Consumers must
/// validate and budget original form slices separately rather than calling
/// `EntryOwner::forms` or treating the attached facts as validated relationships.
#[derive(Debug, Clone, Copy)]
pub struct LinkOccurrenceRef<'ir, 'path> {
    /// The actual `Inline::Link` node; never a reconstructed form copy.
    pub link: &'ir Inline,
    /// Original typed destination, including any original fragment.
    pub target: &'ir LinkTarget,
    /// Original visible label children, including empty labels and styling.
    pub label: &'ir [Inline],
    /// Precise snapshot-local node position.
    pub location: ContentLocationRef<'path>,
    /// Nearest actual list/definition item, including unannotated containers.
    pub content_owner: Option<ReferenceOwnerRef<'ir, 'path>>,
    /// Nearest attached semantic owner. Invalid fields never become inferred names.
    pub semantic_owner: Option<ReferenceOwnerRef<'ir, 'path>>,
    /// Nearest available content provenance, not a fabricated exact link span.
    pub source: Option<SourceSpan>,
}
