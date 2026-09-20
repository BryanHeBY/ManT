//! Experimental typed facade for native structured rendering.
//!
//! This module is deliberately feature-gated and hidden from generated
//! documentation while the structured result model is integrated by its first
//! consumer.  In particular, none of the numeric C ABI discriminants cross
//! this boundary.
#![allow(missing_docs)]

use crate::{InputFormat, SourceBundle};
use std::{error::Error, fmt, num::NonZeroU32, ops::Range};

/// Default deterministic native render width.
pub const DEFAULT_STRUCTURED_WIDTH: u32 = 78;
/// Smallest supported native render width.
pub const MIN_STRUCTURED_WIDTH: u32 = 20;
/// Largest supported native render width.
pub const MAX_STRUCTURED_WIDTH: u32 = 1_000;

macro_rules! key_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(NonZeroU32);

        impl $name {
            #[must_use]
            pub const fn new(value: u32) -> Option<Self> {
                match NonZeroU32::new(value) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }

            #[must_use]
            pub const fn get(self) -> u32 {
                self.0.get()
            }
        }
    };
}

key_type!(SourceKey);
key_type!(SpanKey);
key_type!(ProvenanceKey);
key_type!(OwnerKey);
key_type!(ContentRootKey);
key_type!(ContentAtomKey);
key_type!(LinkOccurrenceKey);
key_type!(NativeBlockKey);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StructuredProfile {
    #[default]
    Utf8,
    Ascii,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceFormat {
    Man,
    Mdoc,
    Markdown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceIdentity {
    Path(String),
    BundleMember(String),
    Anonymous(String),
}

impl SourceIdentity {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Path(name) | Self::BundleMember(name) | Self::Anonymous(name) => name,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceCoordinates {
    DecodedUtf8Bytes,
    NativeNormalizedBytes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRecord {
    pub(crate) key: SourceKey,
    pub(crate) identity: SourceIdentity,
    pub(crate) format: SourceFormat,
    pub(crate) decoded_byte_len: u64,
    pub(crate) content_sha256: Option<[u8; 32]>,
    pub(crate) coordinates: SourceCoordinates,
}

impl SourceRecord {
    #[must_use]
    pub const fn key(&self) -> SourceKey {
        self.key
    }
    #[must_use]
    pub const fn identity(&self) -> &SourceIdentity {
        &self.identity
    }
    #[must_use]
    pub const fn format(&self) -> SourceFormat {
        self.format
    }
    #[must_use]
    pub const fn decoded_byte_len(&self) -> u64 {
        self.decoded_byte_len
    }
    #[must_use]
    pub const fn content_sha256(&self) -> Option<&[u8; 32]> {
        self.content_sha256.as_ref()
    }
    #[must_use]
    pub const fn coordinates(&self) -> SourceCoordinates {
        self.coordinates
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineColumn {
    line: NonZeroU32,
    column: NonZeroU32,
}

impl LineColumn {
    #[must_use]
    pub const fn new(line: u32, column: u32) -> Option<Self> {
        match (NonZeroU32::new(line), NonZeroU32::new(column)) {
            (Some(line), Some(column)) => Some(Self { line, column }),
            _ => None,
        }
    }

    #[must_use]
    pub const fn line(self) -> u32 {
        self.line.get()
    }

    #[must_use]
    pub const fn column(self) -> u32 {
        self.column.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineColumns {
    pub(crate) start: LineColumn,
    pub(crate) end: Option<LineColumn>,
}

impl LineColumns {
    #[must_use]
    pub const fn new(start: LineColumn, end: Option<LineColumn>) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn start(self) -> LineColumn {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> Option<LineColumn> {
        self.end
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    pub(crate) source: SourceKey,
    pub(crate) line_columns: Option<LineColumns>,
    pub(crate) byte_range: Option<Range<u64>>,
}

impl SourceSpan {
    #[must_use]
    pub const fn source(&self) -> SourceKey {
        self.source
    }
    #[must_use]
    pub const fn line_columns(&self) -> Option<LineColumns> {
        self.line_columns
    }
    #[must_use]
    pub const fn byte_range(&self) -> Option<&Range<u64>> {
        self.byte_range.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Provenance {
    Authored { span: SpanKey },
    Generated { trigger: Option<SpanKey> },
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentOwnerKind {
    Document,
    Section,
    Paragraph,
    ListItem,
    DefinitionItem,
    TableCell,
    FixedDisplay,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentOwner {
    pub(crate) key: OwnerKey,
    pub(crate) kind: ContentOwnerKind,
    pub(crate) provenance: ProvenanceKey,
}

impl ContentOwner {
    #[must_use]
    pub const fn key(&self) -> OwnerKey {
        self.key
    }
    #[must_use]
    pub const fn kind(&self) -> ContentOwnerKind {
        self.kind
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentRootKind {
    Heading,
    Term,
    Body,
    Cell,
    FixedBody,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentRoot {
    pub(crate) key: ContentRootKey,
    pub(crate) owner: OwnerKey,
    pub(crate) ordinal: u32,
    pub(crate) kind: ContentRootKind,
    pub(crate) provenance: ProvenanceKey,
}

impl ContentRoot {
    #[must_use]
    pub const fn key(&self) -> ContentRootKey {
        self.key
    }
    #[must_use]
    pub const fn owner(&self) -> OwnerKey {
        self.owner
    }
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }
    #[must_use]
    pub const fn kind(&self) -> ContentRootKind {
        self.kind
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContentAtomKind {
    Text {
        text: String,
        display_override: Option<String>,
    },
    Whitespace {
        text: String,
        display_override: Option<String>,
        breakable: bool,
    },
    BreakOpportunity,
    HardBreak,
}

impl ContentAtomKind {
    #[must_use]
    pub fn logical_text(&self) -> Option<&str> {
        match self {
            Self::Text { text, .. } | Self::Whitespace { text, .. } => Some(text),
            Self::HardBreak => Some("\n"),
            Self::BreakOpportunity => None,
        }
    }

    #[must_use]
    pub fn display_override(&self) -> Option<&str> {
        match self {
            Self::Text {
                display_override, ..
            }
            | Self::Whitespace {
                display_override, ..
            } => display_override.as_deref(),
            Self::BreakOpportunity | Self::HardBreak => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StructuredStyle {
    #[default]
    Plain,
    Bold,
    Italic,
    Literal,
    Underline,
    BoldItalic,
    BoldLiteral,
    BoldUnderline,
    ItalicLiteral,
    ItalicUnderline,
    LiteralUnderline,
    BoldItalicLiteral,
    BoldItalicUnderline,
    BoldLiteralUnderline,
    ItalicLiteralUnderline,
    BoldItalicLiteralUnderline,
}

impl StructuredStyle {
    #[must_use]
    pub const fn is_bold(self) -> bool {
        matches!(
            self,
            Self::Bold
                | Self::BoldItalic
                | Self::BoldLiteral
                | Self::BoldUnderline
                | Self::BoldItalicLiteral
                | Self::BoldItalicUnderline
                | Self::BoldLiteralUnderline
                | Self::BoldItalicLiteralUnderline
        )
    }
    #[must_use]
    pub const fn is_italic(self) -> bool {
        matches!(
            self,
            Self::Italic
                | Self::BoldItalic
                | Self::ItalicLiteral
                | Self::ItalicUnderline
                | Self::BoldItalicLiteral
                | Self::BoldItalicUnderline
                | Self::ItalicLiteralUnderline
                | Self::BoldItalicLiteralUnderline
        )
    }
    #[must_use]
    pub const fn is_literal(self) -> bool {
        matches!(
            self,
            Self::Literal
                | Self::BoldLiteral
                | Self::ItalicLiteral
                | Self::LiteralUnderline
                | Self::BoldItalicLiteral
                | Self::BoldLiteralUnderline
                | Self::ItalicLiteralUnderline
                | Self::BoldItalicLiteralUnderline
        )
    }
    #[must_use]
    pub const fn is_underline(self) -> bool {
        matches!(
            self,
            Self::Underline
                | Self::BoldUnderline
                | Self::ItalicUnderline
                | Self::LiteralUnderline
                | Self::BoldItalicUnderline
                | Self::BoldLiteralUnderline
                | Self::ItalicLiteralUnderline
                | Self::BoldItalicLiteralUnderline
        )
    }

    pub(crate) const fn from_flags([bold, italic, literal, underline]: [bool; 4]) -> Self {
        match (bold, italic, literal, underline) {
            (false, false, false, false) => Self::Plain,
            (true, false, false, false) => Self::Bold,
            (false, true, false, false) => Self::Italic,
            (false, false, true, false) => Self::Literal,
            (false, false, false, true) => Self::Underline,
            (true, true, false, false) => Self::BoldItalic,
            (true, false, true, false) => Self::BoldLiteral,
            (true, false, false, true) => Self::BoldUnderline,
            (false, true, true, false) => Self::ItalicLiteral,
            (false, true, false, true) => Self::ItalicUnderline,
            (false, false, true, true) => Self::LiteralUnderline,
            (true, true, true, false) => Self::BoldItalicLiteral,
            (true, true, false, true) => Self::BoldItalicUnderline,
            (true, false, true, true) => Self::BoldLiteralUnderline,
            (false, true, true, true) => Self::ItalicLiteralUnderline,
            (true, true, true, true) => Self::BoldItalicLiteralUnderline,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRole {
    Flag,
    EnvironmentVariable,
    Argument,
    CommandOrDirective,
    Path,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentAtom {
    pub(crate) key: ContentAtomKey,
    pub(crate) root: ContentRootKey,
    pub(crate) ordinal: u32,
    pub(crate) owner: OwnerKey,
    pub(crate) kind: ContentAtomKind,
    pub(crate) style: StructuredStyle,
    pub(crate) role: Option<NativeRole>,
    pub(crate) link: Option<LinkOccurrenceKey>,
    pub(crate) provenance: ProvenanceKey,
}

impl ContentAtom {
    #[must_use]
    pub const fn key(&self) -> ContentAtomKey {
        self.key
    }
    #[must_use]
    pub const fn root(&self) -> ContentRootKey {
        self.root
    }
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }
    #[must_use]
    pub const fn owner(&self) -> OwnerKey {
        self.owner
    }
    #[must_use]
    pub const fn kind(&self) -> &ContentAtomKind {
        &self.kind
    }
    #[must_use]
    pub const fn style(&self) -> StructuredStyle {
        self.style
    }
    #[must_use]
    pub const fn role(&self) -> Option<NativeRole> {
        self.role
    }
    #[must_use]
    pub const fn link(&self) -> Option<LinkOccurrenceKey> {
        self.link
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentRef {
    pub(crate) atom: ContentAtomKey,
    pub(crate) bytes: Range<u32>,
}

impl ContentRef {
    #[must_use]
    pub const fn atom(&self) -> ContentAtomKey {
        self.atom
    }
    #[must_use]
    pub const fn bytes(&self) -> &Range<u32> {
        &self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeLinkTarget {
    External(String),
    Email(String),
    Document(String),
    Manual { name: String, section: String },
    Section(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkOccurrence {
    pub(crate) key: LinkOccurrenceKey,
    pub(crate) owner: OwnerKey,
    pub(crate) target: NativeLinkTarget,
    pub(crate) title: Option<String>,
    pub(crate) label_refs: Range<usize>,
    pub(crate) provenance: ProvenanceKey,
}

impl LinkOccurrence {
    #[must_use]
    pub const fn key(&self) -> LinkOccurrenceKey {
        self.key
    }
    #[must_use]
    pub const fn owner(&self) -> OwnerKey {
        self.owner
    }
    #[must_use]
    pub const fn target(&self) -> &NativeLinkTarget {
        &self.target
    }
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    #[must_use]
    pub const fn label_refs(&self) -> &Range<usize> {
        &self.label_refs
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeBlockKind {
    Heading,
    Paragraph,
    List,
    DefinitionList,
    Table,
    Indented,
    FixedDisplay,
    VerticalSpace,
    ThematicBreak,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeBlock {
    pub(crate) key: NativeBlockKey,
    pub(crate) owner: OwnerKey,
    pub(crate) kind: NativeBlockKind,
    pub(crate) parent: Option<NativeBlockKey>,
    pub(crate) ordinal: u32,
    pub(crate) provenance: ProvenanceKey,
    pub(crate) root: Option<ContentRootKey>,
}

impl NativeBlock {
    #[must_use]
    pub const fn key(&self) -> NativeBlockKey {
        self.key
    }
    #[must_use]
    pub const fn owner(&self) -> OwnerKey {
        self.owner
    }
    #[must_use]
    pub const fn kind(&self) -> NativeBlockKind {
        self.kind
    }
    #[must_use]
    pub const fn parent(&self) -> Option<NativeBlockKey> {
        self.parent
    }
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
    #[must_use]
    pub const fn root(&self) -> Option<ContentRootKey> {
        self.root
    }
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuredStage {
    Marshal,
    Resolve,
    Parse,
    Render,
    Finalize,
    Check,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuredLimitKind {
    InputSources,
    Sources,
    SourcePathBytes,
    DecodedSourceBytesPerSource,
    DecodedSourceBytesTotal,
    SourceMapEntries,
    SourceMapBytes,
    BuilderOperations,
    BuilderAllocatedBytes,
    ContentBytes,
    Owners,
    Blocks,
    ContentAtoms,
    ContentRefs,
    ContentPoints,
    Links,
    Tables,
    TableRows,
    TableCells,
    FixedViews,
    FixedLines,
    Placements,
    Decorations,
    Forms,
    NameHints,
    Relations,
    ConnectionAtoms,
    AnnotationRuns,
    AnnotationMutations,
    RelationEdges,
    Diagnostics,
    TransferObjects,
    TransferEdges,
    TransferBytes,
    NestingDepth,
    IncludeDepth,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuredErrorKind {
    InvalidInput,
    Reentrant,
    Budget,
    BuilderAllocation,
    Native,
    InvalidResult,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuredError {
    pub(crate) kind: StructuredErrorKind,
    pub(crate) stage: StructuredStage,
    pub(crate) limit: Option<StructuredLimitKind>,
    pub(crate) observed: u64,
    pub(crate) allowed: u64,
    pub(crate) message: String,
}

impl StructuredError {
    pub(crate) fn new(
        kind: StructuredErrorKind,
        stage: StructuredStage,
        limit: Option<StructuredLimitKind>,
        observed: u64,
        allowed: u64,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            stage,
            limit,
            observed,
            allowed,
            message: message.into(),
        }
    }

    fn invalid_width(width: u32) -> Self {
        Self::new(
            StructuredErrorKind::InvalidInput,
            StructuredStage::Marshal,
            None,
            u64::from(width),
            u64::from(MAX_STRUCTURED_WIDTH),
            format!(
                "structured render width must be between {MIN_STRUCTURED_WIDTH} and {MAX_STRUCTURED_WIDTH}"
            ),
        )
    }

    #[must_use]
    pub const fn kind(&self) -> StructuredErrorKind {
        self.kind
    }
    #[must_use]
    pub const fn stage(&self) -> StructuredStage {
        self.stage
    }
    #[must_use]
    pub const fn limit(&self) -> Option<StructuredLimitKind> {
        self.limit
    }
    #[must_use]
    pub const fn observed(&self) -> u64 {
        self.observed
    }
    #[must_use]
    pub const fn allowed(&self) -> u64 {
        self.allowed
    }
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for StructuredError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for StructuredError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StructuredLimits {
    pub max_input_sources: u64,
    pub max_sources: u64,
    pub max_source_path_bytes: u64,
    pub max_decoded_source_bytes_per_source: u64,
    pub max_decoded_source_bytes_total: u64,
    pub max_source_map_entries: u64,
    pub max_source_map_bytes: u64,
    pub max_builder_operations: u64,
    pub max_builder_allocated_bytes: u64,
    pub max_content_bytes: u64,
    pub max_owners: u64,
    pub max_blocks: u64,
    pub max_content_atoms: u64,
    pub max_content_refs: u64,
    pub max_content_points: u64,
    pub max_links: u64,
    pub max_tables: u64,
    pub max_table_rows: u64,
    pub max_table_cells: u64,
    pub max_fixed_views: u64,
    pub max_fixed_lines: u64,
    pub max_placements: u64,
    pub max_decorations: u64,
    pub max_forms: u64,
    pub max_name_hints: u64,
    pub max_relations: u64,
    pub max_connection_atoms: u64,
    pub max_annotation_runs: u64,
    pub max_annotation_mutations: u64,
    pub max_relation_edges: u64,
    pub max_diagnostics: u64,
    pub max_transfer_objects: u64,
    pub max_transfer_edges: u64,
    pub max_transfer_bytes: u64,
    pub max_nesting_depth: u64,
    pub max_include_depth: u64,
}

impl Default for StructuredLimits {
    fn default() -> Self {
        Self {
            max_input_sources: 4_096,
            max_sources: 4_096,
            max_source_path_bytes: 4 * 1024 * 1024,
            max_decoded_source_bytes_per_source: 64 * 1024 * 1024,
            max_decoded_source_bytes_total: 256 * 1024 * 1024,
            max_source_map_entries: 4_194_304,
            max_source_map_bytes: 64 * 1024 * 1024,
            max_builder_operations: 67_108_864,
            max_builder_allocated_bytes: 256 * 1024 * 1024,
            max_content_bytes: 128 * 1024 * 1024,
            max_owners: 1_048_576,
            max_blocks: 1_048_576,
            max_content_atoms: 4_194_304,
            max_content_refs: 8_388_608,
            max_content_points: 1_048_576,
            max_links: 1_048_576,
            max_tables: 1_048_576,
            max_table_rows: 1_048_576,
            max_table_cells: 4_194_304,
            max_fixed_views: 1_048_576,
            max_fixed_lines: 1_048_576,
            max_placements: 8_388_608,
            max_decorations: 8_388_608,
            max_forms: 1_048_576,
            max_name_hints: 1_048_576,
            max_relations: 4_194_304,
            max_connection_atoms: 4_194_304,
            max_annotation_runs: 4_194_304,
            max_annotation_mutations: 16_777_216,
            max_relation_edges: 8_388_608,
            max_diagnostics: 65_536,
            max_transfer_objects: 16_777_216,
            max_transfer_edges: 16_777_216,
            max_transfer_bytes: 512 * 1024 * 1024,
            max_nesting_depth: 256,
            max_include_depth: 64,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuredRenderer {
    profile: StructuredProfile,
    width: u32,
    limits: StructuredLimits,
}

impl Default for StructuredRenderer {
    fn default() -> Self {
        Self {
            profile: StructuredProfile::default(),
            width: DEFAULT_STRUCTURED_WIDTH,
            limits: StructuredLimits::default(),
        }
    }
}

impl StructuredRenderer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
    pub const fn limits(&self) -> &StructuredLimits {
        &self.limits
    }

    #[must_use]
    pub const fn with_profile(mut self, profile: StructuredProfile) -> Self {
        self.profile = profile;
        self
    }

    pub fn with_width(mut self, width: u32) -> Result<Self, StructuredError> {
        if !(MIN_STRUCTURED_WIDTH..=MAX_STRUCTURED_WIDTH).contains(&width) {
            return Err(StructuredError::invalid_width(width));
        }
        self.width = width;
        Ok(self)
    }

    #[must_use]
    pub const fn with_limits(mut self, limits: StructuredLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn render_bundle(
        &self,
        root: &str,
        bundle: &SourceBundle,
        format: InputFormat,
    ) -> Result<StructuredDocument, StructuredError> {
        crate::ffi::render_structured(root, bundle, format, self.profile, self.width, &self.limits)
    }
}

pub fn render_bundle(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
) -> Result<StructuredDocument, StructuredError> {
    StructuredRenderer::default().render_bundle(root, bundle, format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_reject_the_absent_sentinel() {
        assert_eq!(SourceKey::new(0), None);
        assert_eq!(SpanKey::new(0), None);
        assert_eq!(ProvenanceKey::new(0), None);
        assert_eq!(OwnerKey::new(0), None);
        assert_eq!(ContentRootKey::new(0), None);
        assert_eq!(ContentAtomKey::new(0), None);
        assert_eq!(LinkOccurrenceKey::new(0), None);
        assert_eq!(NativeBlockKey::new(0), None);
    }

    #[test]
    fn content_ref_resolves_the_documents_atom_store_without_copying() {
        let text = String::from("alphabet");
        let text_pointer = text.as_ptr();
        let atom_key = ContentAtomKey::new(1).unwrap();
        let reference = ContentRef {
            atom: atom_key,
            bytes: 1..4,
        };
        let document = test_document(vec![ContentAtom {
            key: atom_key,
            root: ContentRootKey::new(1).unwrap(),
            ordinal: 0,
            owner: OwnerKey::new(1).unwrap(),
            kind: ContentAtomKind::Text {
                text,
                display_override: None,
            },
            style: StructuredStyle::default(),
            role: None,
            link: None,
            provenance: ProvenanceKey::new(1).unwrap(),
        }]);

        let resolved = document.resolve_content_ref(&reference).unwrap();
        assert_eq!(resolved, "lph");
        assert_eq!(resolved.as_ptr(), text_pointer.wrapping_add(1));
    }

    #[test]
    fn width_is_deterministic_and_explicitly_overridable() {
        assert_eq!(StructuredRenderer::default().width(), 78);
        assert_eq!(
            StructuredRenderer::new().with_width(100).unwrap().width(),
            100
        );
        assert_eq!(
            StructuredRenderer::new().with_width(19).unwrap_err().kind(),
            StructuredErrorKind::InvalidInput
        );
    }

    #[test]
    fn line_ends_and_hard_breaks_have_no_numeric_sentinels() {
        assert_eq!(LineColumn::new(0, 7), None);
        assert_eq!(LineColumn::new(3, 0), None);
        let point = LineColumns::new(LineColumn::new(3, 7).unwrap(), None);
        assert_eq!(point.end(), None);
        assert_eq!(ContentAtomKind::HardBreak.logical_text(), Some("\n"));
        assert_eq!(ContentAtomKind::BreakOpportunity.logical_text(), None);

        let atom_key = ContentAtomKey::new(1).unwrap();
        let document = test_document(vec![ContentAtom {
            key: atom_key,
            root: ContentRootKey::new(1).unwrap(),
            ordinal: 0,
            owner: OwnerKey::new(1).unwrap(),
            kind: ContentAtomKind::HardBreak,
            style: StructuredStyle::default(),
            role: None,
            link: None,
            provenance: ProvenanceKey::new(1).unwrap(),
        }]);
        assert_eq!(
            document.resolve_content_ref(&ContentRef {
                atom: atom_key,
                bytes: 0..1,
            }),
            None
        );
    }

    fn test_document(content_atoms: Vec<ContentAtom>) -> StructuredDocument {
        StructuredDocument {
            root_source: SourceKey::new(1).unwrap(),
            profile: StructuredProfile::Utf8,
            width: DEFAULT_STRUCTURED_WIDTH,
            metadata: StructuredMetadata {
                macro_set: SourceFormat::Man,
                title: None,
                section: None,
                volume: None,
                operating_system: None,
                architecture: None,
                name: None,
                date: None,
                alias_target: None,
                has_body: true,
            },
            sources: Vec::new(),
            spans: Vec::new(),
            provenances: vec![Provenance::Unknown],
            owners: Vec::new(),
            content_roots: Vec::new(),
            content_atoms,
            content_refs: Vec::new(),
            links: Vec::new(),
            blocks: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}
