//! Paired native output cannot be assembled or rebound by consumers.
use mant_ir::{LinkLayout, ResolvedContent};
use std::sync::Arc;

/// One immutable resolved snapshot and the native link facts derived from it.
#[derive(Clone, Debug)]
pub struct ResolvedWithLinkLayout {
    content: Arc<ResolvedContent>,
    layout: LinkLayout,
}

impl ResolvedWithLinkLayout {
    pub(super) fn new(content: ResolvedContent, layout: LinkLayout) -> Self {
        Self {
            content: Arc::new(content),
            layout,
        }
    }

    /// The exact content from which these facts were derived.
    #[must_use]
    pub fn content(&self) -> &ResolvedContent {
        &self.content
    }

    /// Share the exact content snapshot without reloading or rebuilding it.
    #[must_use]
    pub fn content_arc(&self) -> Arc<ResolvedContent> {
        Arc::clone(&self.content)
    }

    /// Native facts for this snapshot; empty for other sources.
    #[must_use]
    pub const fn layout(&self) -> &LinkLayout {
        &self.layout
    }
}
