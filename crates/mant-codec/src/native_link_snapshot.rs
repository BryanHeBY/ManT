//! Opaque decoded link annotations and their presentation handoff.
use mant_ir::{
    ContentLocation, Document, DocumentAddress, LinkLayoutReceipt, LinkTarget, ResolvedContent,
    TldrDocument,
};
use std::sync::Arc;

/// The presentation-only state of a paired native snapshot. The compatible
/// raw `mant_ir::LinkLayout` and the document wire are unchanged.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NativeLinkLayout {
    pub(crate) receipts: Vec<LinkLayoutReceipt>,
    pub(crate) restrictions: Vec<NativeLinkRestriction>,
    pub(crate) restrict_unidentified: bool,
}

impl NativeLinkLayout {
    /// Fully validated memberships in final structural document order.
    #[must_use]
    pub fn receipts(&self) -> &[LinkLayoutReceipt] {
        &self.receipts
    }

    /// Known mixed wrappers without enough proof to activate any glyph.
    #[must_use]
    pub fn restrictions(&self) -> &[NativeLinkRestriction] {
        &self.restrictions
    }

    /// Unidentified external links may be mixed. Proven memberships are
    /// still usable and unrelated non-external navigation is unaffected.
    #[must_use]
    pub const fn restrict_unidentified(&self) -> bool {
        self.restrict_unidentified
    }
}

/// An exact legacy wrapper requiring evidence that was not representable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeLinkRestriction {
    /// Final structural location in the paired document.
    pub origin: ContentLocation,
    /// Expected original target, for structural validation before rendering.
    pub target: LinkTarget,
}

/// The immutable document that produced its native link receipts.
///
/// Only the codec's native decoder can attach nonempty facts. The caller may
/// supply catalog metadata but never a different document for these facts.
#[derive(Debug)]
pub struct DecodedLinkDocument {
    document: Document,
    layout: NativeLinkLayout,
}

impl DecodedLinkDocument {
    #[cfg(feature = "roff")]
    pub(crate) const fn from_decode(document: Document, layout: NativeLinkLayout) -> Self {
        Self { document, layout }
    }

    /// Wrap an ordinary document, with no native link facts.
    #[must_use]
    pub fn from_plain(document: Document) -> Self {
        Self {
            document,
            layout: NativeLinkLayout::default(),
        }
    }

    /// Borrow the exact decoded model.
    #[must_use]
    pub const fn document(&self) -> &Document {
        &self.document
    }

    /// Drop native link facts and keep the document alone.
    #[must_use]
    pub fn into_document(self) -> Document {
        self.document
    }

    /// Supply the selected manual section when the source did not declare one.
    pub fn use_manual_section_if_missing(&mut self, section: String) {
        self.document.meta.manual_section.get_or_insert(section);
    }

    /// Apply alias metadata from bounded manual source selection.
    pub fn set_alias_target(&mut self, target: String) {
        self.document.meta.alias_target = Some(target);
    }

    /// Resolve the exact owned document with caller-supplied catalog metadata.
    #[must_use]
    pub fn into_resolved(
        self,
        label: String,
        address: Option<DocumentAddress>,
        tldr: Option<TldrDocument>,
    ) -> SourceBoundLinkLayout {
        SourceBoundLinkLayout {
            content: Arc::new(ResolvedContent {
                label,
                address,
                document: Some(self.document),
                tldr,
            }),
            layout: self.layout,
        }
    }
}

/// One immutable resolved snapshot and the native link facts derived from it.
///
/// Borrowed presentation facts cannot be transplanted into another snapshot,
/// even if its link target, location and typed children are identical.
///
/// ```compile_fail
/// use mant_codec::SourceBoundLinkLayout;
/// fn rebind(a: &SourceBoundLinkLayout, b: &SourceBoundLinkLayout) {
///     let layout = b.layout_for(&b.content_arc()).unwrap().clone();
///     // Genuine native data still cannot be attached to another allocation.
///     let _ = SourceBoundLinkLayout { content: a.content_arc(), layout };
/// }
/// ```
///
/// ```compile_fail
/// use mant_codec::{DecodedLinkDocument, SourceBoundLinkLayout};
/// fn rebind(a: &SourceBoundLinkLayout, b: &SourceBoundLinkLayout) {
///     let layout = b.layout_for(&b.content_arc()).unwrap().clone();
///     let _ = DecodedLinkDocument::from_decode(a.content().document.clone().unwrap(), layout);
/// }
/// ```
#[derive(Clone, Debug)]
pub struct SourceBoundLinkLayout {
    content: Arc<ResolvedContent>,
    layout: NativeLinkLayout,
}

impl SourceBoundLinkLayout {
    /// Use an ordinary resolution without attaching any native link facts.
    #[must_use]
    pub fn without_native_links(content: ResolvedContent) -> Self {
        Self {
            content: Arc::new(content),
            layout: NativeLinkLayout::default(),
        }
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

    /// All presentation facts, including budget restrictions, are valid only
    /// for the exact decoded allocation.
    #[must_use]
    pub fn layout_for(&self, content: &Arc<ResolvedContent>) -> Option<&NativeLinkLayout> {
        Arc::ptr_eq(&self.content, content).then_some(&self.layout)
    }

    /// Presentation facts are valid only for the exact decoded allocation.
    #[must_use]
    pub fn receipts_for(&self, content: &Arc<ResolvedContent>) -> Option<&[LinkLayoutReceipt]> {
        self.layout_for(content)
            .map(|layout| layout.receipts.as_slice())
    }

    /// Borrow source facts for this resolved snapshot.
    #[must_use]
    pub fn receipts(&self) -> &[LinkLayoutReceipt] {
        &self.layout.receipts
    }
}
