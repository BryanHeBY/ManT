//! Immutable arena-backed syntax tree and storage-independent views.

use std::{collections::BTreeMap, fmt};

use crate::{Source, SourceId, SourceName, SourcePosition, SourceSpan};

/// High-level macro package detected for a document.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacroSet {
    /// No supported semantic macro package was detected yet.
    None,
    /// The source uses the semantic mdoc(7) macro package.
    Mdoc,
    /// The source uses the traditional man(7) macro package.
    Man,
}

/// Renderer-neutral role of one syntax node.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeKind {
    /// Synthetic root containing the complete document.
    Root,
    /// Macro block such as a section or display.
    Block,
    /// Heading or term portion of a block.
    Head,
    /// Principal content portion of a block.
    Body,
    /// Trailing portion of a block.
    Tail,
    /// Leaf-level semantic macro invocation.
    Element,
    /// Literal source text after roff escape processing.
    Text,
    /// Retained source comment.
    Comment,
    /// tbl(7) table node.
    Table,
    /// eqn(7) equation node.
    Equation,
}

/// Normalized mdoc list behavior.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalizedListKind {
    /// Unordered bullet list.
    Bullet,
    /// Ordered list.
    Ordered,
    /// Term-and-description list.
    Definition,
    /// Aligned column list.
    Column,
    /// Marker-free list.
    Plain,
}

/// Source-level marker spelling retained only for native terminal rendering.
///
/// The public normalized AST deliberately projects `-bullet`, `-dash`, and
/// `-hyphen` into the same [`NormalizedListKind::Bullet`] behavior to match
/// the legacy owned tree.  The terminal device nevertheless prints different
/// glyphs and font treatment for those forms, so the arena keeps this private
/// provenance until rendering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MdocListMarker {
    /// `Bl -bullet`, displayed as a bold plus/circle glyph.
    Bullet,
    /// `Bl -dash`, displayed as a roman hyphen.
    Dash,
    /// `Bl -hyphen`, displayed as a bold hyphen.
    Hyphen,
    /// `Bl -enum`, displayed as an increasing decimal ordinal.
    Enum,
}

/// Whether a display preserves source line layout.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayKind {
    /// Preserve line breaks and horizontal whitespace.
    Literal,
    /// Reflow content as prose.
    Filled,
}

/// Normalized mdoc `Bf` font behavior.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalizedFont {
    /// Typographic emphasis.
    Emphasis,
    /// Literal or fixed-width text.
    Literal,
    /// Symbolic text conventionally displayed in bold.
    Symbolic,
}

/// Explicit author layout mode selected by mdoc `An`.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorMode {
    /// Render following authors separately.
    Split,
    /// Keep following authors in one group.
    NoSplit,
}

/// Stateful mdoc `Es`/`En` delimiters resolved on an `En` use.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedEnclosure {
    /// Visible opening delimiter.
    pub opening: Box<str>,
    /// Visible closing delimiter when configured.
    pub closing: Option<Box<str>>,
}

/// Horizontal alignment retained for one tbl cell.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableAlignment {
    /// Left edge alignment.
    Left,
    /// Horizontal centering.
    Center,
    /// Right edge alignment.
    Right,
}

/// Logical payload of a tbl cell.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableCell {
    /// Visible content, absent for a spanning or empty cell.
    pub text: Option<Box<str>>,
    /// This cell used `T{`/`T}` multi-line text-block syntax.
    pub text_block: bool,
    /// This cell continues a vertical span from an earlier row.
    pub vertical_continuation: bool,
    /// Number of occupied logical columns.
    pub column_span: u16,
    /// Number of occupied logical rows.
    pub row_span: u16,
    /// Requested horizontal alignment.
    pub alignment: TableAlignment,
}

/// Terminal-only border weight retained from a tbl layout declaration.
///
/// This is intentionally not part of [`TableCell`]: libmandoc's owned AST
/// drops tbl device layout while its terminal renderer still consumes it.
/// Keeping the information beside a generated `Table` node lets the native
/// renderer reproduce that device behavior without widening the public,
/// canonical AST contract.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum TableTerminalBorder {
    /// No border was requested.
    #[default]
    None,
    /// A one-cell terminal rule.
    Single,
    /// A doubled terminal rule.
    Double,
}

/// Terminal-only font selected by a tbl format modifier.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum TableTerminalFont {
    /// Inherit the ordinary terminal font.
    #[default]
    Roman,
    /// Terminal bold.
    Bold,
    /// Terminal italic.
    Italic,
}

/// Renderer-private presentation metadata for one physical tbl column.
#[allow(clippy::struct_excessive_bools)] // Each flag mirrors one independent tbl layout modifier.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TableTerminalCell {
    /// Number of vertical rules before this column, bounded by tbl to two.
    pub before_vertical_rules: u8,
    /// The leading rule came from a standalone layout line rather than this
    /// row's own leading `|`. It reserves a device field but does not paint
    /// a downward segment on the preceding data row.
    pub leading_vertical_from_standalone: bool,
    /// Number of vertical rules after this column, bounded by tbl to two.
    pub after_vertical_rules: u8,
    /// Horizontal rule occupying this column instead of cell text.
    pub horizontal_rule: TableTerminalBorder,
    /// This physical column extends the preceding cell rather than consuming
    /// one data field.
    pub span: bool,
    /// This physical column continues a cell from an earlier row.
    pub vertical_continuation: bool,
    /// tbl `n` decimal-alignment column.
    pub numeric: bool,
    /// tbl `z` modifier: this cell is rendered but does not contribute to
    /// calculated terminal column width.
    pub width_ignored: bool,
    /// tbl `x` modifier: this column receives a share of spare terminal
    /// table width after fixed fields have been measured.
    pub width_expanding: bool,
    /// Requested inter-column spacing after this column, when authored.
    pub spacing: Option<u8>,
    /// Minimum terminal field width selected by a tbl `w` modifier.
    ///
    /// This stays out of the public AST because tbl's owned-cell projection
    /// does not expose device geometry.  The native terminal renderer needs
    /// it before allocating columns, however, including for an otherwise
    /// short or empty cell.
    pub minimum_width: Option<u16>,
    /// Font modifier attached to this column.
    pub font: TableTerminalFont,
}

/// Renderer-private tbl row metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TableTerminalRow {
    /// Whether this row begins a distinct `.TS`/`.TE` table range.
    ///
    /// The public AST intentionally keeps generated table rows flat as
    /// siblings.  The device renderer still needs this private boundary to
    /// avoid merging adjacent source tables into one calculated layout.
    pub starts_table: bool,
    /// Physical layout descriptors, including span and horizontal-rule cells.
    pub cells: Vec<TableTerminalCell>,
    /// Physical start column for each retained public data cell.
    ///
    /// A horizontal-rule layout cell still consumes one input field even
    /// though the terminal suppresses that field. The owned AST preserves it
    /// for compatibility, so the device renderer must not infer these
    /// positions merely by filtering visible layout cells.
    pub data_columns: Vec<u16>,
    /// Outer box style selected by tbl options.
    pub outer_border: TableTerminalBorder,
    /// Whether every ordinary cell has a frame.
    pub all_box: bool,
    /// Whether tbl centers the whole table in its device field.
    pub centered: bool,
    /// A physical tbl data-rule line between ordinary table rows.
    pub horizontal_rule: TableTerminalBorder,
}

/// One token from an eqn expression retained only for native device output.
///
/// The public owned AST deliberately exposes the legacy flattened equation
/// spelling.  That spelling loses font, accent, and grouping boxes that the
/// terminal and HTML devices still need.  Keep the bounded, definition-
/// expanded token stream adjacent to the generated equation node instead of
/// widening the public compatibility contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EquationTerminalToken {
    /// Authored or definition-expanded eqn token spelling.
    pub text: Box<str>,
    /// Whether the spelling was quoted and must bypass grammar keywords.
    pub quoted: bool,
}

/// Renderer-private eqn token stream for a single generated equation node.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct EquationTerminal {
    /// Bounded token sequence after parser-side definition expansion.
    pub tokens: Vec<EquationTerminalToken>,
}

/// Source and semantic flags used by lowering and rendering.
#[allow(clippy::struct_excessive_bools)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NodeFlags {
    /// Node was synthesized rather than written explicitly.
    pub generated: bool,
    /// Node ends a sentence according to roff punctuation rules.
    pub sentence_end: bool,
    /// Node must not contribute visible output.
    pub no_print: bool,
    /// Node belongs to a no-fill region.
    pub no_fill: bool,
    /// Node is a validated same-document destination.
    pub deep_link_target: bool,
    /// Node renders a self-link for that destination.
    pub permalink: bool,
    /// Node begins a roff input line.
    pub line_start: bool,
    /// Text suppresses spacing after opening punctuation.
    pub delimiter_open: bool,
    /// Text suppresses spacing before closing punctuation.
    pub delimiter_close: bool,
    /// Text ends in a roff `\c` line continuation.
    pub line_continuation: bool,
    /// Node uses synopsis-style presentation semantics.
    pub synopsis_pretty: bool,
}

/// Opaque document-local node identity.
///
/// It is checked by [`Document::node`] and cannot be constructed by callers;
/// arena layout and allocation order remain implementation details.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NodeId(u32);

impl fmt::Debug for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NodeId(..)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct StringId(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputUnicodeProvenance {
    None,
    Invalid,
    ValidNonAscii,
    Mixed,
}

impl InputUnicodeProvenance {
    const fn new(has_invalid_input_bytes: bool, has_valid_utf8_non_ascii: bool) -> Self {
        match (has_invalid_input_bytes, has_valid_utf8_non_ascii) {
            (false, false) => Self::None,
            (true, false) => Self::Invalid,
            (false, true) => Self::ValidNonAscii,
            (true, true) => Self::Mixed,
        }
    }

    const fn has_invalid_input_bytes(self) -> bool {
        matches!(self, Self::Invalid | Self::Mixed)
    }

    const fn has_valid_utf8_non_ascii(self) -> bool {
        matches!(self, Self::ValidNonAscii | Self::Mixed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // Private scanner provenance is intentionally flat until package restructuring consumes it.
struct NodeRecord {
    kind: NodeKind,
    parent: Option<NodeId>,
    child_start: u32,
    child_len: u32,
    macro_name: Option<StringId>,
    text: Option<StringId>,
    /// Scanner-only delimiter retained until package restructuring.  It is
    /// never exposed by the public arena view: mdoc column lists use it to
    /// distinguish a phrase-ending tab from ordinary horizontal whitespace.
    separator_after: Option<u8>,
    /// Whether the scanner-owned whitespace run contains a tab after this
    /// argument.  This remains private until mdoc column restructuring has
    /// consumed it.
    separator_contains_tab: bool,
    /// Number of literal tab bytes retained inside one scanner argument.
    /// This is private mdoc phrase provenance rather than public text state.
    embedded_tab_count: u32,
    /// Width of that scanner-only delimiter.  The public tree never exposes
    /// it; mdoc's D1/Dl partial blocks use a doubled separator as a phrase
    /// boundary.
    separator_width: u32,
    /// Copy-mode provenance retained only while man package restructuring is
    /// in progress.  It distinguishes an authored `\\t` from an effective
    /// tabulation escape after both spellings converge in the public text
    /// projection.
    protected_tabulation_escape: bool,
    /// Scanner provenance retained only through semantic preprocessing. It
    /// distinguishes malformed bytes from a valid non-ASCII UTF-8 scalar
    /// when both decode to the same Rust character and lets validators map
    /// visible text offsets back to physical source bytes safely.
    input_unicode_provenance: InputUnicodeProvenance,
    /// Difference between this parsed argument's normalized visible byte
    /// width and its original lexical width.  A few package validators assign
    /// later argument locations after expansion, so this stays private until
    /// that semantic pass has consumed it.
    argument_expansion_width_delta: i32,
    /// Scanner provenance retained for package diagnostics that calculate a
    /// suffix location after a quoted numeric argument has been expanded.
    argument_quoted: bool,
    tag: Option<StringId>,
    location: Option<SourceSpan>,
    flags: NodeFlags,
    list_kind: Option<NormalizedListKind>,
    list_marker: Option<MdocListMarker>,
    /// Renderer-only column declaration phrases from mdoc `Bl -column`.
    ///
    /// The legacy public AST exposes only the normalized `Column` list kind;
    /// it drops the declaration text after the list options.  The terminal
    /// device nevertheless uses those phrases to select each field width, so
    /// retain their normalized spellings privately until rendering.
    column_widths: Vec<StringId>,
    /// Renderer-only source provenance for mdoc `Bl -hang`.
    ///
    /// The legacy normalized AST projects both `-hang` and `-tag` as a
    /// definition list.  The terminal device nevertheless keeps a hanging
    /// list's first Body phrase on the term line even for a non-positive
    /// width, so retain that selector without widening the public AST.
    terminal_hanging_list: bool,
    /// Renderer-only source provenance for mdoc `Bl -ohang`.
    ///
    /// It shares the public normalized definition-list behavior with `-tag`,
    /// but its terminal tag and Body are independently indented lines.
    terminal_overhanging_list: bool,
    /// Renderer-only source provenance for mdoc `Bl -inset`.
    ///
    /// It shares the public normalized definition-list behavior with `-tag`,
    /// but its terminal Body begins one ordinary separator after the term.
    terminal_inset_list: bool,
    /// Renderer-only source provenance for mdoc `Bl -diag`.
    ///
    /// It shares the public normalized definition-list behavior with `-tag`,
    /// but its terminal term is bold and its Body starts after two cells.
    terminal_diagnostic_list: bool,
    /// Man renderer provenance for the blank source line that package
    /// validation consumes immediately before a first indented field.
    terminal_suppressed_leading_blank: bool,
    /// A visible body selected by a same-line roff conditional.  Its public
    /// source flags deliberately retain the line-start compatibility shape,
    /// while the terminal device must keep an initial tab in the current
    /// field rather than starting a new physical output line.
    terminal_inline_conditional: bool,
    display_kind: Option<DisplayKind>,
    /// Renderer-only distinction retained for mdoc `Bd -literal`.
    ///
    /// Public normalized ASTs intentionally classify both `-literal` and
    /// `-unfilled` as [`DisplayKind::Literal`], matching the legacy owned
    /// tree. Their terminal tab stops differ, so the parser retains this
    /// provenance only for the native renderer.
    literal_display: bool,
    /// Renderer-only distinction retained for mdoc `Bd -centered`.
    ///
    /// The public AST deliberately normalizes centered displays as filled;
    /// the terminal device nevertheless centers each completed physical line.
    centered_display: bool,
    font: Option<NormalizedFont>,
    author_mode: Option<AuthorMode>,
    enclosure: Option<NormalizedEnclosure>,
    compact: bool,
    offset: Option<StringId>,
    width: Option<StringId>,
    table_cells: Vec<TableCell>,
    /// Device-layout metadata deliberately kept out of the public AST.
    table_terminal: Option<TableTerminalRow>,
    equation: Option<StringId>,
    /// Device-only eqn grammar retained beside the flattened public text.
    equation_terminal: Option<EquationTerminal>,
}

impl NodeRecord {
    fn root() -> Self {
        Self {
            kind: NodeKind::Root,
            parent: None,
            child_start: 0,
            child_len: 0,
            macro_name: None,
            text: None,
            separator_after: None,
            separator_contains_tab: false,
            embedded_tab_count: 0,
            separator_width: 0,
            protected_tabulation_escape: false,
            input_unicode_provenance: InputUnicodeProvenance::None,
            argument_expansion_width_delta: 0,
            argument_quoted: false,
            tag: None,
            location: None,
            flags: NodeFlags::default(),
            list_kind: None,
            list_marker: None,
            column_widths: Vec::new(),
            terminal_hanging_list: false,
            terminal_overhanging_list: false,
            terminal_inset_list: false,
            terminal_diagnostic_list: false,
            terminal_suppressed_leading_blank: false,
            terminal_inline_conditional: false,
            display_kind: None,
            literal_display: false,
            centered_display: false,
            font: None,
            author_mode: None,
            enclosure: None,
            compact: false,
            offset: None,
            width: None,
            table_cells: Vec::new(),
            table_terminal: None,
            equation: None,
            equation_terminal: None,
        }
    }
}

/// Document metadata normalized from document control macros.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Metadata {
    /// Canonical manual title.
    pub title: Option<Box<str>>,
    /// Native manual category such as `1` or `3p`.
    pub section: Option<Box<str>>,
    /// Manual volume or collection label.
    pub volume: Option<Box<str>>,
    /// Operating-system label declared by the source.
    pub os: Option<Box<str>>,
    /// Architecture qualifier declared by the source.
    pub arch: Option<Box<str>>,
    /// Primary display name extracted from NAME.
    pub name: Option<Box<str>>,
    /// Normalized source date.
    pub date: Option<Box<str>>,
    /// Target from a top-level `.so` alias page.
    pub alias_target: Option<Box<str>>,
    /// Whether the parser observed a document body.
    pub has_body: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceRecord {
    name: SourceName,
    byte_len: u32,
    line_starts: Vec<u32>,
}

impl SourceRecord {
    fn from_source(source: Source<'_>) -> Self {
        let byte_len = u32::try_from(source.bytes.len())
            .expect("parser rejects sources that cannot fit public span offsets");
        let mut line_starts = vec![0];
        for (index, byte) in source.bytes.iter().enumerate() {
            if *byte == b'\n' {
                line_starts.push(
                    u32::try_from(index + 1)
                        .expect("parser rejects sources that cannot fit public span offsets"),
                );
            }
        }
        Self {
            name: source.name.clone(),
            byte_len,
            line_starts,
        }
    }

    fn position(&self, offset: u32) -> Option<SourcePosition> {
        if offset > self.byte_len {
            return None;
        }
        let line_index = self.line_starts.partition_point(|start| *start <= offset);
        let line_start = *self.line_starts.get(line_index.checked_sub(1)?)?;
        Some(SourcePosition {
            line: u32::try_from(line_index).ok()?,
            column: offset.checked_sub(line_start)?.checked_add(1)?,
        })
    }
}

/// Complete immutable owned syntax document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Document {
    macro_set: MacroSet,
    metadata: Metadata,
    sources: Vec<SourceRecord>,
    nodes: Vec<NodeRecord>,
    child_edges: Vec<NodeId>,
    strings: Vec<Box<str>>,
}

impl Document {
    pub(crate) fn empty(macro_set: MacroSet, root_source: Source<'_>) -> Self {
        Self {
            macro_set,
            metadata: Metadata::default(),
            sources: vec![SourceRecord::from_source(root_source)],
            nodes: vec![NodeRecord::root()],
            child_edges: Vec::new(),
            strings: Vec::new(),
        }
    }

    /// Return the selected macro package.
    #[must_use]
    pub const fn macro_set(&self) -> MacroSet {
        self.macro_set
    }

    /// Return validated document metadata.
    #[must_use]
    pub const fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Return the number of root and resolved sources in this document.
    #[must_use]
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// Return the root source identity.
    #[must_use]
    pub const fn root_source(&self) -> SourceId {
        SourceId(0)
    }

    /// Resolve one document-local source identity to its logical name.
    #[must_use]
    pub fn source_name(&self, id: SourceId) -> Option<&SourceName> {
        self.sources.get(id.0 as usize).map(|source| &source.name)
    }

    /// Derive a one-based line and byte column from a validated source span.
    #[must_use]
    pub fn source_position(&self, span: &SourceSpan) -> Option<SourcePosition> {
        if let Some(position) = span.logical_start {
            return Some(position);
        }
        let source = self.sources.get(span.source.0 as usize)?;
        (span.end <= source.byte_len)
            .then(|| source.position(span.start))
            .flatten()
    }

    /// Return the synthetic root node.
    #[must_use]
    pub const fn root(&self) -> NodeId {
        NodeId(0)
    }

    /// Return a checked immutable node view.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<NodeRef<'_>> {
        self.nodes.get(id.0 as usize).map(|record| NodeRef {
            document: self,
            id,
            record,
        })
    }

    /// Return the number of nodes stored in this document.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Traverse nodes in deterministic depth-first preorder without recursion.
    #[must_use]
    pub fn preorder(&self) -> Preorder<'_> {
        Preorder {
            document: self,
            pending: vec![self.root()],
        }
    }

    fn string(&self, id: StringId) -> &str {
        self.strings[id.0 as usize].as_ref()
    }
}

/// Borrowed read-only view of one node.
#[derive(Clone, Copy, Debug)]
pub struct NodeRef<'doc> {
    document: &'doc Document,
    id: NodeId,
    record: &'doc NodeRecord,
}

impl<'doc> NodeRef<'doc> {
    /// Return this node's opaque identity.
    #[must_use]
    pub const fn id(self) -> NodeId {
        self.id
    }

    /// Return the renderer-neutral structural role.
    #[must_use]
    pub const fn kind(self) -> NodeKind {
        self.record.kind
    }

    /// Return the containing node when this is not the synthetic root.
    #[must_use]
    pub fn parent(self) -> Option<Self> {
        self.record.parent.and_then(|id| self.document.node(id))
    }

    /// Return the source macro/request name without a leading dot.
    #[must_use]
    pub fn macro_name(self) -> Option<&'doc str> {
        self.record.macro_name.map(|id| self.document.string(id))
    }

    /// Return normalized visible text.
    #[must_use]
    pub fn text(self) -> Option<&'doc str> {
        self.record.text.map(|id| self.document.string(id))
    }

    /// Return a validated same-document tag.
    #[must_use]
    pub fn tag(self) -> Option<&'doc str> {
        self.record.tag.map(|id| self.document.string(id))
    }

    /// Return the source location when one is available.
    #[must_use]
    pub fn location(self) -> Option<&'doc SourceSpan> {
        self.record.location.as_ref()
    }

    /// Return the one-based logical source position of this node's start.
    ///
    /// This is a convenience for read-only tree consumers that need a stable
    /// source coordinate without exposing arena internals.
    #[must_use]
    pub fn source_position(self) -> Option<SourcePosition> {
        self.location()
            .and_then(|span| self.document.source_position(span))
    }

    /// Return source and semantic flags.
    #[must_use]
    pub const fn flags(self) -> NodeFlags {
        self.record.flags
    }

    /// Return normalized list behavior.
    #[must_use]
    pub const fn list_kind(self) -> Option<NormalizedListKind> {
        self.record.list_kind
    }

    /// Return native-only source marker provenance for an mdoc list.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) const fn list_marker(self) -> Option<MdocListMarker> {
        self.record.list_marker
    }

    /// Return mdoc `Bl -column` declaration phrases for terminal layout.
    ///
    /// This is deliberately renderer-private: it has no counterpart in the
    /// legacy owned AST's public schema.
    #[cfg(feature = "render")]
    pub(crate) fn column_widths(self) -> impl Iterator<Item = &'doc str> {
        self.record
            .column_widths
            .iter()
            .map(|id| self.document.string(*id))
    }

    /// Whether this normalized definition list was authored as `Bl -hang`.
    /// This is renderer-only provenance; public AST consumers observe the
    /// common [`NormalizedListKind::Definition`] behavior.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) const fn terminal_hanging_list(self) -> bool {
        self.record.terminal_hanging_list
    }

    /// Whether this normalized definition list was authored as `Bl -ohang`.
    /// This is renderer-only provenance; public AST consumers observe the
    /// common [`NormalizedListKind::Definition`] behavior.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) const fn terminal_overhanging_list(self) -> bool {
        self.record.terminal_overhanging_list
    }

    /// Whether this normalized definition list was authored as `Bl -inset`.
    /// This is renderer-only provenance; public AST consumers observe the
    /// common [`NormalizedListKind::Definition`] behavior.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) const fn terminal_inset_list(self) -> bool {
        self.record.terminal_inset_list
    }

    /// Whether this normalized definition list was authored as `Bl -diag`.
    /// This is renderer-only provenance; public AST consumers observe the
    /// common [`NormalizedListKind::Definition`] behavior.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) const fn terminal_diagnostic_list(self) -> bool {
        self.record.terminal_diagnostic_list
    }

    /// Whether man validation consumed an otherwise leading blank before this
    /// terminal field block. This is renderer-only provenance and never
    /// changes the public normalized AST.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) const fn terminal_suppressed_leading_blank(self) -> bool {
        self.record.terminal_suppressed_leading_blank
    }

    /// Whether this text was emitted by an active same-line roff condition.
    /// This renderer-only provenance preserves terminal tab behavior without
    /// changing the public compatible source flags.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) const fn terminal_inline_conditional(self) -> bool {
        self.record.terminal_inline_conditional
    }

    /// Return display fill behavior.
    #[must_use]
    pub const fn display_kind(self) -> Option<DisplayKind> {
        self.record.display_kind
    }

    /// Whether this node belongs to an mdoc `Bd -literal` display.
    ///
    /// This is crate-private renderer provenance; public AST consumers use
    /// [`Self::display_kind`] for normalized display semantics.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) const fn literal_display(self) -> bool {
        self.record.literal_display
    }

    /// Whether this node belongs to an mdoc `Bd -centered` display.
    ///
    /// This is renderer-only provenance; public AST consumers observe the
    /// normalized [`DisplayKind::Filled`] classification instead.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) const fn centered_display(self) -> bool {
        self.record.centered_display
    }

    /// Return the scanner separator following this node, for renderer-only
    /// inline tab reconstruction.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) const fn separator_after(self) -> Option<u8> {
        self.record.separator_after
    }

    /// Return the width of the scanner-owned horizontal separator following
    /// this argument.  This stays renderer-private: the public owned AST
    /// deliberately exposes normalized semantic text rather than source
    /// layout provenance, while the terminal device must retain an authored
    /// run of adjacent spaces.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) const fn separator_width(self) -> u32 {
        self.record.separator_width
    }

    /// Whether this text argument originated in a quoted scanner phrase.
    ///
    /// The renderer uses this only to distinguish a quoted argument's
    /// significant trailing blanks from ordinary end-of-line whitespace.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) const fn argument_quoted(self) -> bool {
        self.record.argument_quoted
    }

    /// Return mdoc font behavior.
    #[must_use]
    pub const fn font(self) -> Option<NormalizedFont> {
        self.record.font
    }

    /// Return mdoc author behavior.
    #[must_use]
    pub const fn author_mode(self) -> Option<AuthorMode> {
        self.record.author_mode
    }

    /// Return stateful delimiters already resolved by the parser.
    #[must_use]
    pub fn enclosure(self) -> Option<&'doc NormalizedEnclosure> {
        self.record.enclosure.as_ref()
    }

    /// Return whether the enclosing list requests compact layout.
    #[must_use]
    pub const fn compact(self) -> bool {
        self.record.compact
    }

    /// Return normalized roff offset including its scale suffix.
    #[must_use]
    pub fn offset(self) -> Option<&'doc str> {
        self.record.offset.map(|id| self.document.string(id))
    }

    /// Return normalized mdoc list width including its scale suffix.
    #[must_use]
    pub fn width(self) -> Option<&'doc str> {
        self.record.width.map(|id| self.document.string(id))
    }

    /// Return logical tbl cells for this row node.
    #[must_use]
    pub fn table_cells(self) -> &'doc [TableCell] {
        &self.record.table_cells
    }

    /// Return renderer-only tbl presentation metadata.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) fn table_terminal(self) -> Option<&'doc TableTerminalRow> {
        self.record.table_terminal.as_ref()
    }

    /// Return normalized eqn expression text.
    #[must_use]
    pub fn equation(self) -> Option<&'doc str> {
        self.record.equation.map(|id| self.document.string(id))
    }

    /// Return renderer-only eqn grammar retained for device lowering.
    #[must_use]
    #[cfg(feature = "render")]
    pub(crate) fn equation_terminal(self) -> Option<&'doc EquationTerminal> {
        self.record.equation_terminal.as_ref()
    }

    /// Iterate direct children in source order.
    #[must_use]
    pub fn children(self) -> Children<'doc> {
        let start = self.record.child_start as usize;
        let end = start + self.record.child_len as usize;
        Children {
            document: self.document,
            edges: self.document.child_edges[start..end].iter(),
        }
    }

    /// Iterate nearest parent first without recursion.
    #[must_use]
    pub fn ancestors(self) -> Ancestors<'doc> {
        Ancestors {
            document: self.document,
            next: self.record.parent,
        }
    }
}

/// Direct-child iterator for a [`NodeRef`].
pub struct Children<'doc> {
    document: &'doc Document,
    edges: std::slice::Iter<'doc, NodeId>,
}

impl<'doc> Iterator for Children<'doc> {
    type Item = NodeRef<'doc>;

    fn next(&mut self) -> Option<Self::Item> {
        self.edges.next().and_then(|id| self.document.node(*id))
    }
}

impl DoubleEndedIterator for Children<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.edges
            .next_back()
            .and_then(|id| self.document.node(*id))
    }
}

/// Ancestor iterator for a [`NodeRef`].
pub struct Ancestors<'doc> {
    document: &'doc Document,
    next: Option<NodeId>,
}

impl<'doc> Iterator for Ancestors<'doc> {
    type Item = NodeRef<'doc>;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.next?;
        let node = self.document.node(id)?;
        self.next = node.record.parent;
        Some(node)
    }
}

/// Preorder iterator for a [`Document`].
pub struct Preorder<'doc> {
    document: &'doc Document,
    pending: Vec<NodeId>,
}

impl<'doc> Iterator for Preorder<'doc> {
    type Item = NodeRef<'doc>;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.pending.pop()?;
        let node = self.document.node(id)?;
        self.pending.extend(node.children().map(NodeRef::id).rev());
        Some(node)
    }
}

/// Internal incremental arena builder used by parser milestones and tests.
mod builder;
pub(crate) use builder::DocumentBuilder;

#[cfg(test)]
mod tests;
