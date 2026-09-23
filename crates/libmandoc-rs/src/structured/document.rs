//! Structured document metadata, diagnostics, and table ownership.

use super::{
    AnchorEvidence, AnchorEvidenceKey, ContentAtom, ContentAtomKey, ContentAtomKind, ContentOwner,
    ContentPoint, ContentPointKey, ContentRef, ContentRoot, ContentRootKey, HeadingEvidence,
    HeadingEvidenceKey, LinkLabelPart, LinkOccurrence, LinkOccurrenceKey, NativeBlock,
    NativeBlockKey, NativeForm, NativeFormKey, NativeItem, NativeItemKey, NativeList,
    NativeListKey, NativeNameHint, NativeNameHintKey, NativeTable, NativeTableCell, NativeTableRow,
    OwnerKey, Provenance, ProvenanceKey, SourceFormat, SourceKey, SourceRecord, SourceSpan,
    SpanKey, StructuredProfile,
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
    pub(crate) content_points: Vec<ContentPoint>,
    pub(crate) links: Vec<LinkOccurrence>,
    pub(crate) link_label_parts: Vec<LinkLabelPart>,
    pub(crate) anchors: Vec<AnchorEvidence>,
    pub(crate) heading_evidence: Vec<HeadingEvidence>,
    pub(crate) blocks: Vec<NativeBlock>,
    pub(crate) lists: Vec<NativeList>,
    pub(crate) items: Vec<NativeItem>,
    pub(crate) tables: Vec<NativeTable>,
    pub(crate) table_rows: Vec<NativeTableRow>,
    pub(crate) table_cells: Vec<NativeTableCell>,
    pub(crate) forms: Vec<NativeForm>,
    pub(crate) name_hints: Vec<NativeNameHint>,
    pub(crate) diagnostics: Vec<NativeDiagnostic>,
}

/// Owned native tables transferred into one normalized content store.
///
/// The remaining [`StructuredDocument`] keeps block, list, evidence, source,
/// and diagnostic tables so a consumer can finish structural lowering after
/// this one-way transfer.  Its content-table accessors return empty slices.
#[derive(Debug)]
pub struct StructuredContentTables {
    owners: Vec<ContentOwner>,
    roots: Vec<ContentRoot>,
    atoms: Vec<ContentAtom>,
    points: Vec<ContentPoint>,
    links: Vec<LinkOccurrence>,
}

/// Dense native content tables in owner/root/atom/point/link order.
pub type StructuredContentTableParts = (
    Vec<ContentOwner>,
    Vec<ContentRoot>,
    Vec<ContentAtom>,
    Vec<ContentPoint>,
    Vec<LinkOccurrence>,
);

impl StructuredContentTables {
    /// Consume the transfer object into its dense native tables.
    #[must_use]
    pub fn into_parts(self) -> StructuredContentTableParts {
        (self.owners, self.roots, self.atoms, self.points, self.links)
    }
}

impl StructuredDocument {
    /// Move the native content-store records out after relation planning.
    ///
    /// This deliberately leaves all structural and evidence tables in place.
    /// Callers must finish every operation that needs native owners, roots,
    /// points, or link-label tables before invoking this one-way transfer.
    #[must_use]
    pub fn take_content_tables(&mut self) -> StructuredContentTables {
        StructuredContentTables {
            owners: std::mem::take(&mut self.owners),
            roots: std::mem::take(&mut self.content_roots),
            atoms: std::mem::take(&mut self.content_atoms),
            points: std::mem::take(&mut self.content_points),
            links: std::mem::take(&mut self.links),
        }
    }

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
    pub fn content_points(&self) -> &[ContentPoint] {
        &self.content_points
    }
    #[must_use]
    pub fn links(&self) -> &[LinkOccurrence] {
        &self.links
    }
    #[must_use]
    pub fn link_label_parts(&self) -> &[LinkLabelPart] {
        &self.link_label_parts
    }
    #[must_use]
    pub fn anchors(&self) -> &[AnchorEvidence] {
        &self.anchors
    }
    #[must_use]
    pub fn heading_evidence(&self) -> &[HeadingEvidence] {
        &self.heading_evidence
    }
    #[must_use]
    pub fn blocks(&self) -> &[NativeBlock] {
        &self.blocks
    }
    #[must_use]
    pub fn lists(&self) -> &[NativeList] {
        &self.lists
    }
    #[must_use]
    pub fn items(&self) -> &[NativeItem] {
        &self.items
    }
    #[must_use]
    pub fn tables(&self) -> &[NativeTable] {
        &self.tables
    }
    #[must_use]
    pub fn table_rows(&self) -> &[NativeTableRow] {
        &self.table_rows
    }
    #[must_use]
    pub fn table_cells(&self) -> &[NativeTableCell] {
        &self.table_cells
    }
    #[must_use]
    pub fn forms(&self) -> &[NativeForm] {
        &self.forms
    }
    #[must_use]
    pub fn name_hints(&self) -> &[NativeNameHint] {
        &self.name_hints
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
    pub fn content_point(&self, key: ContentPointKey) -> Option<&ContentPoint> {
        self.content_points.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn link(&self, key: LinkOccurrenceKey) -> Option<&LinkOccurrence> {
        self.links.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn anchor(&self, key: AnchorEvidenceKey) -> Option<&AnchorEvidence> {
        self.anchors.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn heading(&self, key: HeadingEvidenceKey) -> Option<&HeadingEvidence> {
        self.heading_evidence.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn block(&self, key: NativeBlockKey) -> Option<&NativeBlock> {
        self.blocks.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn list(&self, key: NativeListKey) -> Option<&NativeList> {
        self.lists.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn item(&self, key: NativeItemKey) -> Option<&NativeItem> {
        self.items.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn form(&self, key: NativeFormKey) -> Option<&NativeForm> {
        self.forms.get(key.get() as usize - 1)
    }
    #[must_use]
    pub fn name_hint(&self, key: NativeNameHintKey) -> Option<&NativeNameHint> {
        self.name_hints.get(key.get() as usize - 1)
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
    pub fn link_label(&self, link: &LinkOccurrence) -> Option<&[LinkLabelPart]> {
        self.link_label_parts.get(link.label_parts.clone())
    }
}
