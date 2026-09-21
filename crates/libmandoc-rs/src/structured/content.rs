//! Logical content, ownership, roles, links, and block records.

use super::{
    ContentAtomKey, ContentRootKey, LinkOccurrenceKey, NativeBlockKey, NativeFormKey,
    NativeItemKey, NativeListKey, NativeNameHintKey, OwnerKey, ProvenanceKey,
};
use std::ops::Range;

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
pub enum NativeListKind {
    Bullet,
    Ordered,
    Plain,
    Definition,
    NativeMarker,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeList {
    pub(crate) key: NativeListKey,
    pub(crate) block: NativeBlockKey,
    pub(crate) kind: NativeListKind,
    pub(crate) compact: bool,
    pub(crate) start: Option<u32>,
    pub(crate) provenance: ProvenanceKey,
}

impl NativeList {
    #[must_use]
    pub const fn key(&self) -> NativeListKey {
        self.key
    }
    #[must_use]
    pub const fn block(&self) -> NativeBlockKey {
        self.block
    }
    #[must_use]
    pub const fn kind(&self) -> NativeListKind {
        self.kind
    }
    #[must_use]
    pub const fn compact(&self) -> bool {
        self.compact
    }
    #[must_use]
    pub const fn start(&self) -> Option<u32> {
        self.start
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeTargetOrigin {
    Generated,
    Authored,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeItem {
    pub(crate) key: NativeItemKey,
    pub(crate) list: NativeListKey,
    pub(crate) owner: OwnerKey,
    pub(crate) ordinal: u32,
    pub(crate) forms: Range<usize>,
    pub(crate) target: Option<String>,
    pub(crate) target_origin: Option<NativeTargetOrigin>,
    pub(crate) provenance: ProvenanceKey,
}

impl NativeItem {
    #[must_use]
    pub const fn key(&self) -> NativeItemKey {
        self.key
    }
    #[must_use]
    pub const fn list(&self) -> NativeListKey {
        self.list
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
    pub const fn forms(&self) -> &Range<usize> {
        &self.forms
    }
    #[must_use]
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }
    #[must_use]
    pub const fn target_origin(&self) -> Option<NativeTargetOrigin> {
        self.target_origin
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeForm {
    pub(crate) key: NativeFormKey,
    pub(crate) owner: OwnerKey,
    pub(crate) role: Option<NativeRole>,
    pub(crate) refs: Range<usize>,
    pub(crate) provenance: ProvenanceKey,
}

impl NativeForm {
    #[must_use]
    pub const fn key(&self) -> NativeFormKey {
        self.key
    }
    #[must_use]
    pub const fn owner(&self) -> OwnerKey {
        self.owner
    }
    #[must_use]
    pub const fn role(&self) -> Option<NativeRole> {
        self.role
    }
    #[must_use]
    pub const fn refs(&self) -> &Range<usize> {
        &self.refs
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeNameHint {
    pub(crate) key: NativeNameHintKey,
    pub(crate) form: NativeFormKey,
    pub(crate) refs: Range<usize>,
    pub(crate) provenance: ProvenanceKey,
}

impl NativeNameHint {
    #[must_use]
    pub const fn key(&self) -> NativeNameHintKey {
        self.key
    }
    #[must_use]
    pub const fn form(&self) -> NativeFormKey {
        self.form
    }
    #[must_use]
    pub const fn refs(&self) -> &Range<usize> {
        &self.refs
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}
