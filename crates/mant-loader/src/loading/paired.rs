//! Shared immutable source snapshot and its native link facts.
pub use mant_ir::SourceBoundLinkLayout as ResolvedWithLinkLayout;

pub(super) fn pair(
    content: mant_ir::ResolvedContent,
    layout: mant_ir::LinkLayout,
) -> ResolvedWithLinkLayout {
    ResolvedWithLinkLayout::from_producer(std::sync::Arc::new(content), layout)
}
