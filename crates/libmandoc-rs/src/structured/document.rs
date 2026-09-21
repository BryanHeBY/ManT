//! Structured document metadata, diagnostics, and table ownership.

use super::{
    ContentAtom, ContentAtomKey, ContentAtomKind, ContentOwner, ContentRef, ContentRoot,
    ContentRootKey, LinkOccurrence, LinkOccurrenceKey, NativeBlock, NativeBlockKey, OwnerKey,
    Provenance, ProvenanceKey, SourceFormat, SourceKey, SourceRecord, SourceSpan, SpanKey,
    StructuredProfile,
};
use std::num::NonZeroU32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuredDiagnosticLevel {
    Style,
    Warning,
    Error,
    Unsupported,
}

/// One diagnostic identity from the pinned native `mandocerr` set.
///
/// Construction is private so values outside the frozen `1..=210` set cannot
/// enter a structured document.  The opaque representation keeps distinct
/// native codes comparable without publishing a passthrough integer ABI.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StructuredDiagnosticCode(NonZeroU32);

impl StructuredDiagnosticCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        "mandoc-native"
    }

    pub(crate) const fn from_native_ordinal(code: u32) -> Option<Self> {
        if code > 210 {
            return None;
        }
        match NonZeroU32::new(code) {
            Some(code) => Some(Self(code)),
            None => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeDiagnostic {
    pub(crate) level: StructuredDiagnosticLevel,
    pub(crate) code: StructuredDiagnosticCode,
    pub(crate) message: String,
    pub(crate) span: Option<SpanKey>,
    pub(crate) owner: Option<OwnerKey>,
}

impl NativeDiagnostic {
    #[must_use]
    pub const fn level(&self) -> StructuredDiagnosticLevel {
        self.level
    }
    #[must_use]
    pub const fn code(&self) -> &StructuredDiagnosticCode {
        &self.code
    }
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
    #[must_use]
    pub const fn span(&self) -> Option<SpanKey> {
        self.span
    }
    #[must_use]
    pub const fn owner(&self) -> Option<OwnerKey> {
        self.owner
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuredMetadata {
    pub(crate) macro_set: SourceFormat,
    pub(crate) title: Option<String>,
    pub(crate) section: Option<String>,
    pub(crate) volume: Option<String>,
    pub(crate) operating_system: Option<String>,
    pub(crate) architecture: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) date: Option<String>,
    pub(crate) alias_target: Option<String>,
    pub(crate) has_body: bool,
}

impl StructuredMetadata {
    #[must_use]
    pub const fn macro_set(&self) -> SourceFormat {
        self.macro_set
    }
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    #[must_use]
    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }
    #[must_use]
    pub fn volume(&self) -> Option<&str> {
        self.volume.as_deref()
    }
    #[must_use]
    pub fn operating_system(&self) -> Option<&str> {
        self.operating_system.as_deref()
    }
    #[must_use]
    pub fn architecture(&self) -> Option<&str> {
        self.architecture.as_deref()
    }
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    #[must_use]
    pub fn date(&self) -> Option<&str> {
        self.date.as_deref()
    }
    #[must_use]
    pub fn alias_target(&self) -> Option<&str> {
        self.alias_target.as_deref()
    }
    #[must_use]
    pub const fn has_body(&self) -> bool {
        self.has_body
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuredDocument {
    pub(crate) root_source: SourceKey,
    pub(crate) profile: StructuredProfile,
    pub(crate) width: u32,
    pub(crate) metadata: StructuredMetadata,
    pub(crate) sources: Vec<SourceRecord>,
    pub(crate) spans: Vec<SourceSpan>,
    pub(crate) provenances: Vec<Provenance>,
    pub(crate) owners: Vec<ContentOwner>,
    pub(crate) content_roots: Vec<ContentRoot>,
    pub(crate) content_atoms: Vec<ContentAtom>,
    pub(crate) content_refs: Vec<ContentRef>,
    pub(crate) links: Vec<LinkOccurrence>,
    pub(crate) blocks: Vec<NativeBlock>,
    pub(crate) diagnostics: Vec<NativeDiagnostic>,
}

impl StructuredDocument {
    #[must_use]
    pub const fn root_source(&self) -> SourceKey {
        self.root_source
    }
    #[must_use]
    pub const fn profile(&self) -> StructuredProfile {
        self.profile
    }
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }
    #[must_use]
    pub const fn metadata(&self) -> &StructuredMetadata {
        &self.metadata
    }
    #[must_use]
    pub fn sources(&self) -> &[SourceRecord] {
        &self.sources
    }
    #[must_use]
    pub fn spans(&self) -> &[SourceSpan] {
        &self.spans
    }
    #[must_use]
    pub fn provenances(&self) -> &[Provenance] {
        &self.provenances
    }
    #[must_use]
    pub fn owners(&self) -> &[ContentOwner] {
        &self.owners
    }
    #[must_use]
    pub fn content_roots(&self) -> &[ContentRoot] {
        &self.content_roots
    }
    #[must_use]
    pub fn content_atoms(&self) -> &[ContentAtom] {
        &self.content_atoms
    }
    #[must_use]
    pub fn content_refs(&self) -> &[ContentRef] {
        &self.content_refs
    }
    #[must_use]
    pub fn links(&self) -> &[LinkOccurrence] {
        &self.links
    }
    #[must_use]
    pub fn blocks(&self) -> &[NativeBlock] {
        &self.blocks
    }
    #[must_use]
    pub fn diagnostics(&self) -> &[NativeDiagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn source(&self, key: SourceKey) -> Option<&SourceRecord> {
        self.sources.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn span(&self, key: SpanKey) -> Option<&SourceSpan> {
        self.spans.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn provenance(&self, key: ProvenanceKey) -> Option<&Provenance> {
        self.provenances.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn owner(&self, key: OwnerKey) -> Option<&ContentOwner> {
        self.owners.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn content_root(&self, key: ContentRootKey) -> Option<&ContentRoot> {
        self.content_roots.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn content_atom(&self, key: ContentAtomKey) -> Option<&ContentAtom> {
        self.content_atoms.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn link(&self, key: LinkOccurrenceKey) -> Option<&LinkOccurrence> {
        self.links.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn block(&self, key: NativeBlockKey) -> Option<&NativeBlock> {
        self.blocks.get(key.get() as usize - 1)
    }

    /// Resolve a content reference without allocating or copying its atom text.
    #[must_use]
    pub fn resolve_content_ref(&self, reference: &ContentRef) -> Option<&str> {
        let atom = self.content_atom(reference.atom)?;
        let text = match &atom.kind {
            ContentAtomKind::Text { text, .. } | ContentAtomKind::Whitespace { text, .. } => text,
            ContentAtomKind::BreakOpportunity | ContentAtomKind::HardBreak => return None,
        };
        text.get(reference.bytes.start as usize..reference.bytes.end as usize)
    }

    #[must_use]
    pub fn link_label(&self, link: &LinkOccurrence) -> Option<&[ContentRef]> {
        self.content_refs.get(link.label_refs.clone())
    }
}
