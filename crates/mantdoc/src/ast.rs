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
pub(crate) struct DocumentBuilder {
    document: Document,
    children: Vec<Vec<NodeId>>,
    /// Only source lines with non-ASCII or malformed input retain this
    /// temporary tbl projection. It is consumed during preprocessing and is
    /// never copied into the completed public document.
    table_input_text: BTreeMap<NodeId, Box<str>>,
    /// Semantic preprocessor opener attached to the first normalized output
    /// event. Package restructuring consumes it as an otherwise invisible
    /// scope boundary; it is never copied into the completed public document.
    preprocessor_openers: BTreeMap<NodeId, &'static str>,
    /// Scanner-owned mdoc `nS` state changes.  They deliberately do not
    /// become public AST nodes: mdoc's parser observes the register as
    /// presentation state, not as a roff request in the syntax tree.
    ///
    /// Each boundary is the number of root source events already emitted when
    /// the change took effect.  The mdoc pass consumes it before that indexed
    /// flat source event, retaining source order even though `.nr` itself is
    /// transparent in the final tree.
    mdoc_synopsis_events: Vec<(usize, bool)>,
}

impl DocumentBuilder {
    pub(crate) fn new(macro_set: MacroSet, root_source: Source<'_>) -> Self {
        Self {
            document: Document::empty(macro_set, root_source),
            children: vec![Vec::new()],
            table_input_text: BTreeMap::new(),
            preprocessor_openers: BTreeMap::new(),
            mdoc_synopsis_events: Vec::new(),
        }
    }

    /// Return the macro package selected for this in-progress document.
    pub(crate) const fn macro_set(&self) -> MacroSet {
        self.document.macro_set
    }

    /// Record the resolved mdoc operating-system label.
    ///
    /// This is deliberately parser-internal: public metadata becomes
    /// immutable only when [`Self::finish`] returns the completed document.
    pub(crate) fn operating_system(&mut self, value: impl Into<Box<str>>) {
        self.document.metadata.os = Some(value.into());
    }

    /// Mutably borrow parser-owned metadata before the document is frozen.
    pub(crate) fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.document.metadata
    }

    #[allow(dead_code)] // M2 scanner starts constructing syntax nodes through this builder.
    pub(crate) const fn root() -> NodeId {
        NodeId(0)
    }

    #[allow(dead_code)] // M2 scanner starts spans at the parser-owned root source.
    pub(crate) const fn root_source() -> SourceId {
        SourceId(0)
    }

    /// Register one resolver-owned source in the document-local source map.
    ///
    /// The parser validates byte and line budgets before calling this method;
    /// this builder only rejects an identity that cannot be represented by the
    /// opaque public [`SourceId`].
    pub(crate) fn add_source(&mut self, source: Source<'_>) -> Option<SourceId> {
        let index = u32::try_from(self.document.sources.len()).ok()?;
        self.document
            .sources
            .push(SourceRecord::from_source(source));
        Some(SourceId(index))
    }

    #[allow(dead_code)] // M2 scanner enforces the public AST node budget.
    pub(crate) fn node_count(&self) -> usize {
        self.document.nodes.len()
    }

    /// Record the scanner-observed value of mdoc's private `nS` register.
    pub(crate) fn record_mdoc_synopsis_state(&mut self, active: bool) {
        self.mdoc_synopsis_events
            .push((self.children[Self::root().0 as usize].len(), active));
    }

    /// Take the private `nS` state stream for one mdoc restructuring pass.
    pub(crate) fn take_mdoc_synopsis_events(&mut self) -> Vec<(usize, bool)> {
        std::mem::take(&mut self.mdoc_synopsis_events)
    }

    /// Return a parser-owned node role for semantic restructuring.
    pub(crate) fn node_kind(&self, node: NodeId) -> Option<NodeKind> {
        self.document
            .nodes
            .get(node.0 as usize)
            .map(|record| record.kind)
    }

    /// Return a parser-owned parent while semantic postprocessing still owns
    /// the arena topology.
    pub(crate) fn node_parent(&self, node: NodeId) -> Option<NodeId> {
        self.document.nodes.get(node.0 as usize)?.parent
    }

    /// Change a parser-owned node role before immutable edges are frozen.
    pub(crate) fn set_node_kind(&mut self, node: NodeId, kind: NodeKind) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.kind = kind;
        true
    }

    /// Read one parser-owned macro name without leaking string-table IDs.
    pub(crate) fn node_macro_name(&self, node: NodeId) -> Option<&str> {
        let record = self.document.nodes.get(node.0 as usize)?;
        record.macro_name.map(|id| self.document.string(id))
    }

    /// Read parser-owned visible text without leaking string-table IDs.
    pub(crate) fn node_text(&self, node: NodeId) -> Option<&str> {
        let record = self.document.nodes.get(node.0 as usize)?;
        record.text.map(|id| self.document.string(id))
    }

    /// Read a parser-owned validated destination without leaking string-table IDs.
    pub(crate) fn node_tag(&self, node: NodeId) -> Option<&str> {
        let record = self.document.nodes.get(node.0 as usize)?;
        record.tag.map(|id| self.document.string(id))
    }

    /// Replace provisional visible text during a semantic normalization pass.
    pub(crate) fn set_node_text(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        self.document.strings.push(value.into());
        self.document.nodes[node.0 as usize].text = Some(StringId(index));
        true
    }

    /// Return the scanner-owned byte immediately following one argument.
    ///
    /// This is intentionally private parser metadata, not source text or a
    /// public AST property.  It is consumed by mdoc phrase reconstruction
    /// before [`Self::finish`] freezes the arena.
    pub(crate) fn node_separator_after(&self, node: NodeId) -> Option<u8> {
        self.document.nodes.get(node.0 as usize)?.separator_after
    }

    /// Whether the scanner-owned separator after an argument contains a tab.
    pub(crate) fn node_separator_contains_tab(&self, node: NodeId) -> bool {
        self.document
            .nodes
            .get(node.0 as usize)
            .is_some_and(|record| record.separator_contains_tab)
    }

    /// Return the scanner-owned count of literal tabs within an argument.
    pub(crate) fn node_embedded_tab_count(&self, node: NodeId) -> u32 {
        self.document
            .nodes
            .get(node.0 as usize)
            .map_or(0, |record| record.embedded_tab_count)
    }

    /// Return the scanner-owned horizontal-whitespace run after one argument.
    pub(crate) fn node_separator_width(&self, node: NodeId) -> u32 {
        self.document
            .nodes
            .get(node.0 as usize)
            .map_or(0, |record| record.separator_width)
    }

    /// Retain one scanner-owned argument delimiter for package restructuring.
    pub(crate) fn set_node_separator_after(&mut self, node: NodeId, value: Option<u8>) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.separator_after = value;
        true
    }

    /// Retain whether a scanner-owned separator contains a tab.
    pub(crate) fn set_node_separator_contains_tab(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.separator_contains_tab = value;
        true
    }

    /// Retain the scanner-owned number of literal tabs within an argument.
    pub(crate) fn set_node_embedded_tab_count(&mut self, node: NodeId, value: usize) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.embedded_tab_count = u32::try_from(value).unwrap_or(u32::MAX);
        true
    }

    /// Retain the width of one scanner-owned argument delimiter.
    pub(crate) fn set_node_separator_width(&mut self, node: NodeId, value: usize) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.separator_width = u32::try_from(value).unwrap_or(u32::MAX);
        true
    }

    /// Record that a copy-mode argument contained an authored escaped
    /// tabulation escape.  This is scanner provenance, not public AST state.
    pub(crate) fn set_node_protected_tabulation_escape(
        &mut self,
        node: NodeId,
        value: bool,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.protected_tabulation_escape = value;
        true
    }

    /// Read the temporary copy-mode provenance for package restructuring.
    pub(crate) fn node_has_protected_tabulation_escape(&self, node: NodeId) -> bool {
        self.document
            .nodes
            .get(node.0 as usize)
            .is_some_and(|record| record.protected_tabulation_escape)
    }

    /// Record byte-encoding provenance until semantic preprocessing has
    /// consumed source-relative text offsets.
    pub(crate) fn set_node_input_unicode_provenance(
        &mut self,
        node: NodeId,
        has_invalid_input_bytes: bool,
        has_valid_utf8_non_ascii: bool,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.input_unicode_provenance =
            InputUnicodeProvenance::new(has_invalid_input_bytes, has_valid_utf8_non_ascii);
        true
    }

    /// Read malformed-byte provenance during semantic preprocessing.
    pub(crate) fn node_has_invalid_input_bytes(&self, node: NodeId) -> bool {
        self.document
            .nodes
            .get(node.0 as usize)
            .is_some_and(|record| record.input_unicode_provenance.has_invalid_input_bytes())
    }

    /// Read valid UTF-8 provenance during semantic preprocessing.
    pub(crate) fn node_has_valid_utf8_non_ascii(&self, node: NodeId) -> bool {
        self.document
            .nodes
            .get(node.0 as usize)
            .is_some_and(|record| record.input_unicode_provenance.has_valid_utf8_non_ascii())
    }

    /// Retain a byte-faithful tbl projection for one exceptional source line.
    pub(crate) fn set_node_table_input_text(
        &mut self,
        node: NodeId,
        value: impl Into<Box<str>>,
    ) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        self.table_input_text.insert(node, value.into());
        true
    }

    /// Read the transient byte-faithful tbl projection during preprocessing.
    pub(crate) fn node_table_input_text(&self, node: NodeId) -> Option<&str> {
        self.table_input_text.get(&node).map(Box::as_ref)
    }

    /// Mark one normalized preprocessing event as the first public result of
    /// an otherwise-elided roff preprocessor opener.
    pub(crate) fn set_node_preprocessor_opener(
        &mut self,
        node: NodeId,
        opener: &'static str,
    ) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        self.preprocessor_openers.insert(node, opener);
        true
    }

    /// Read the private preprocessor opener during package restructuring.
    pub(crate) fn node_preprocessor_opener(&self, node: NodeId) -> Option<&'static str> {
        self.preprocessor_openers.get(&node).copied()
    }

    /// Record the private post-expansion width adjustment for one argument.
    pub(crate) fn set_node_argument_expansion_width_delta(
        &mut self,
        node: NodeId,
        value: i32,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.argument_expansion_width_delta = value;
        true
    }

    /// Read the private post-expansion width adjustment for package validation.
    pub(crate) fn node_argument_expansion_width_delta(&self, node: NodeId) -> i32 {
        self.document
            .nodes
            .get(node.0 as usize)
            .map_or(0, |record| record.argument_expansion_width_delta)
    }

    /// Retain whether a scanner argument had an outer quote for package
    /// validators that need legacy suffix source positions.
    pub(crate) fn set_node_argument_quoted(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.argument_quoted = value;
        true
    }

    /// Read private outer-quote provenance during package validation.
    pub(crate) fn node_argument_quoted(&self, node: NodeId) -> bool {
        self.document
            .nodes
            .get(node.0 as usize)
            .is_some_and(|record| record.argument_quoted)
    }

    /// Read the source span attached to a provisional node.
    pub(crate) fn node_location(&self, node: NodeId) -> Option<SourceSpan> {
        self.document.nodes.get(node.0 as usize)?.location.clone()
    }

    /// Resolve a provisional node's current source location for package
    /// validators that need an explicit logical diagnostic column.
    pub(crate) fn node_source_position(&self, node: NodeId) -> Option<SourcePosition> {
        let location = self.node_location(node)?;
        self.document.source_position(&location)
    }

    /// Resolve an arbitrary provisional diagnostic span to its logical source
    /// position without exposing arena storage details to package validators.
    pub(crate) fn source_position(&self, span: &SourceSpan) -> Option<SourcePosition> {
        self.document.source_position(span)
    }

    /// Set a source span on a synthesized semantic node.
    pub(crate) fn set_node_location(&mut self, node: NodeId, value: Option<SourceSpan>) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.location = value;
        true
    }

    /// Rebase a continued control-line node onto the final physical line while
    /// preserving its original logical column.  mandoc's package parsers use
    /// this provenance for an argument list joined with a trailing escape.
    pub(crate) fn rebase_node_location_to_final_line(&mut self, node: NodeId) -> bool {
        let Some(location) = self
            .document
            .nodes
            .get(node.0 as usize)
            .and_then(|record| record.location.clone())
        else {
            return false;
        };
        let Some(source) = self.document.sources.get(location.source.0 as usize) else {
            return false;
        };
        let line_start_for = |offset: u32| {
            source
                .line_starts
                .get(
                    source
                        .line_starts
                        .partition_point(|start| *start <= offset)
                        .saturating_sub(1),
                )
                .copied()
        };
        let Some(initial_line_start) = line_start_for(location.start) else {
            return false;
        };
        let Some(final_line_start) = line_start_for(location.end.saturating_sub(1)) else {
            return false;
        };
        let Some(start) = final_line_start.checked_add(location.start - initial_line_start) else {
            return false;
        };
        if start > location.end {
            return false;
        }
        let final_line = source
            .position(location.end.saturating_sub(1))
            .map_or(1, |position| position.line);
        let logical_column = location
            .start
            .checked_sub(initial_line_start)
            .and_then(|column| column.checked_add(1))
            .unwrap_or(1);
        self.document.nodes[node.0 as usize].location =
            SourceSpan::new(location.source, start, location.end)
                .ok()
                .map(|span| {
                    span.with_logical_start(SourcePosition {
                        line: final_line,
                        column: logical_column,
                    })
                });
        true
    }

    /// Override the presentation location of a node while preserving its byte
    /// range.  This is restricted to parser lowering because it represents
    /// legacy logical-line provenance rather than a source edit.
    pub(crate) fn set_node_logical_start(
        &mut self,
        node: NodeId,
        position: SourcePosition,
    ) -> bool {
        let Some(location) = self
            .document
            .nodes
            .get_mut(node.0 as usize)
            .and_then(|record| record.location.as_mut())
        else {
            return false;
        };
        location.logical_start = Some(position);
        true
    }

    /// Read parser-owned flags while semantic passes still own the arena.
    pub(crate) fn node_flags(&self, node: NodeId) -> Option<NodeFlags> {
        self.document
            .nodes
            .get(node.0 as usize)
            .map(|record| record.flags)
    }

    /// Read provisional list semantics while mdoc postprocessing still owns
    /// the arena topology.
    pub(crate) fn node_list_kind(&self, node: NodeId) -> Option<NormalizedListKind> {
        self.document
            .nodes
            .get(node.0 as usize)
            .and_then(|record| record.list_kind)
    }

    /// Read provisional compact layout state during package postprocessing.
    pub(crate) fn node_compact(&self, node: NodeId) -> Option<bool> {
        self.document
            .nodes
            .get(node.0 as usize)
            .map(|record| record.compact)
    }

    /// Replace parser-owned flags before the document is frozen.
    pub(crate) fn set_node_flags(&mut self, node: NodeId, flags: NodeFlags) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.flags = flags;
        true
    }

    /// Attach a parser-validated same-document destination spelling.
    pub(crate) fn set_node_tag(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        self.document.strings.push(value.into());
        self.document.nodes[node.0 as usize].tag = Some(StringId(index));
        true
    }

    /// Remove a provisional tag superseded by a duplicate fallback heading.
    pub(crate) fn clear_node_tag(&mut self, node: NodeId) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.tag = None;
        true
    }

    /// Set the normalized list semantics for one provisional node.
    pub(crate) fn set_node_list_kind(
        &mut self,
        node: NodeId,
        value: Option<NormalizedListKind>,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.list_kind = value;
        true
    }

    /// Retain the exact mdoc list marker selected by validation for renderers.
    pub(crate) fn set_node_list_marker(
        &mut self,
        node: NodeId,
        value: Option<MdocListMarker>,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.list_marker = value;
        true
    }

    /// Retain renderer-only mdoc column declaration phrases.
    pub(crate) fn set_node_column_widths(
        &mut self,
        node: NodeId,
        values: impl IntoIterator<Item = String>,
    ) -> bool {
        let values = values.into_iter().collect::<Vec<_>>();
        let Some(total) = self.document.strings.len().checked_add(values.len()) else {
            return false;
        };
        if self.document.nodes.get(node.0 as usize).is_none() || total > u32::MAX as usize {
            return false;
        }
        let start = self.document.strings.len();
        self.document
            .strings
            .extend(values.into_iter().map(Into::into));
        let widths = (start..self.document.strings.len())
            .map(|index| {
                // `total` above is bounded by `u32::MAX`, and this half-open
                // range consequently cannot produce an out-of-range id.
                StringId(u32::try_from(index).expect("checked string index fits u32"))
            })
            .collect();
        self.document.nodes[node.0 as usize].column_widths = widths;
        true
    }

    /// Retain mdoc `Bl -hang` provenance for terminal layout only.
    pub(crate) fn set_node_terminal_hanging_list(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.terminal_hanging_list = value;
        true
    }

    /// Retain mdoc `Bl -ohang` provenance for terminal layout only.
    pub(crate) fn set_node_terminal_overhanging_list(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.terminal_overhanging_list = value;
        true
    }

    /// Retain mdoc `Bl -inset` provenance for terminal layout only.
    pub(crate) fn set_node_terminal_inset_list(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.terminal_inset_list = value;
        true
    }

    /// Retain mdoc `Bl -diag` provenance for terminal layout only.
    pub(crate) fn set_node_terminal_diagnostic_list(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.terminal_diagnostic_list = value;
        true
    }

    /// Retain a man validation-only blank-line suppression for terminal
    /// presentation without exposing it through the public AST schema.
    pub(crate) fn set_node_terminal_suppressed_leading_blank(
        &mut self,
        node: NodeId,
        value: bool,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.terminal_suppressed_leading_blank = value;
        true
    }

    /// Retain same-line conditional renderer provenance without exposing it
    /// in the public compatible AST schema.
    pub(crate) fn set_node_terminal_inline_conditional(
        &mut self,
        node: NodeId,
        value: bool,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.terminal_inline_conditional = value;
        true
    }

    /// Set the normalized display behavior for one provisional node.
    pub(crate) fn set_node_display_kind(
        &mut self,
        node: NodeId,
        value: Option<DisplayKind>,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.display_kind = value;
        true
    }

    /// Retain whether an mdoc display used the `-literal` spelling.
    pub(crate) fn set_node_literal_display(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.literal_display = value;
        true
    }

    /// Retain whether an mdoc display used the `-centered` spelling.
    pub(crate) fn set_node_centered_display(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.centered_display = value;
        true
    }

    /// Set the normalized font behavior for one provisional node.
    pub(crate) fn set_node_font(&mut self, node: NodeId, value: Option<NormalizedFont>) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.font = value;
        true
    }

    /// Set the normalized mdoc author layout behavior for one provisional node.
    pub(crate) fn set_node_author_mode(&mut self, node: NodeId, value: Option<AuthorMode>) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.author_mode = value;
        true
    }

    /// Set the mdoc `Es`/`En` delimiters resolved for one provisional node.
    pub(crate) fn set_node_enclosure(
        &mut self,
        node: NodeId,
        value: Option<NormalizedEnclosure>,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.enclosure = value;
        true
    }

    /// Set compact layout behavior for one provisional node.
    pub(crate) fn set_node_compact(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.compact = value;
        true
    }

    /// Set a normalized roff offset string for one provisional node.
    pub(crate) fn set_node_offset(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        self.document.strings.push(value.into());
        self.document.nodes[node.0 as usize].offset = Some(StringId(index));
        true
    }

    /// Set a normalized width string for one provisional node.
    pub(crate) fn set_node_width(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        self.document.strings.push(value.into());
        self.document.nodes[node.0 as usize].width = Some(StringId(index));
        true
    }

    /// Copy the normalized layout fields of one provisional node to another.
    ///
    /// Semantic recovery sometimes materializes a closer-owned Body node;
    /// its source span and flags belong to the closer, while its layout is
    /// inherited from the interrupted block.
    pub(crate) fn copy_node_layout(&mut self, source: NodeId, target: NodeId) -> bool {
        let Some(source) = self.document.nodes.get(source.0 as usize) else {
            return false;
        };
        let layout = (
            source.list_kind,
            source.list_marker,
            source.column_widths.clone(),
            source.display_kind,
            source.literal_display,
            source.centered_display,
            source.font,
            source.author_mode,
            source.enclosure.clone(),
            source.compact,
            source.offset,
            source.width,
        );
        let Some(target) = self.document.nodes.get_mut(target.0 as usize) else {
            return false;
        };
        (
            target.list_kind,
            target.list_marker,
            target.column_widths,
            target.display_kind,
            target.literal_display,
            target.centered_display,
            target.font,
            target.author_mode,
            target.enclosure,
            target.compact,
            target.offset,
            target.width,
        ) = layout;
        true
    }

    /// Set normalized tbl cells on a synthesized table-row node.
    pub(crate) fn set_node_table_cells(&mut self, node: NodeId, value: Vec<TableCell>) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.table_cells = value;
        true
    }

    /// Set private terminal tbl layout metadata on a generated row.
    pub(crate) fn set_node_table_terminal(
        &mut self,
        node: NodeId,
        value: TableTerminalRow,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.table_terminal = Some(value);
        true
    }

    /// Set a normalized eqn expression on a synthesized equation node.
    pub(crate) fn set_node_equation(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        self.document.strings.push(value.into());
        self.document.nodes[node.0 as usize].equation = Some(StringId(index));
        true
    }

    /// Set private device eqn metadata without exposing it through the AST.
    pub(crate) fn set_node_equation_terminal(
        &mut self,
        node: NodeId,
        value: EquationTerminal,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.equation_terminal = Some(value);
        true
    }

    /// Copy the provisional direct children of an in-progress node.
    pub(crate) fn children(&self, parent: NodeId) -> Option<&[NodeId]> {
        self.children.get(parent.0 as usize).map(Vec::as_slice)
    }

    /// Replace an in-progress node's direct children in source order.
    ///
    /// This primitive intentionally does not retain the children at their
    /// previous parent. Semantic restructurers call it only on nodes taken
    /// from the provisional flat scanner tree.
    pub(crate) fn replace_children(&mut self, parent: NodeId, children: &[NodeId]) -> bool {
        if parent.0 as usize >= self.document.nodes.len()
            || children
                .iter()
                .any(|child| child.0 as usize >= self.document.nodes.len())
        {
            return false;
        }
        for child in children {
            self.document.nodes[child.0 as usize].parent = Some(parent);
        }
        self.children[parent.0 as usize].clear();
        self.children[parent.0 as usize].extend_from_slice(children);
        true
    }

    /// Append an existing provisional node under a new semantic parent.
    pub(crate) fn append_existing_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        if parent.0 as usize >= self.document.nodes.len()
            || child.0 as usize >= self.document.nodes.len()
        {
            return false;
        }
        self.document.nodes[child.0 as usize].parent = Some(parent);
        self.children[parent.0 as usize].push(child);
        true
    }

    /// Retain only the finite prefix reachable through `max_depth` node
    /// levels, counting the synthetic root as level one.
    ///
    /// The old FFI adapter copied its root at recursive depth zero and did
    /// not descend from a node at depth 255.  Keeping 256 levels here gives
    /// callers the same observable prefix without reintroducing a recursive
    /// owned-tree copy.  The semantic passes have not exposed any [`NodeId`]
    /// yet, so discarded arena entries can be compacted immediately.
    pub(crate) fn truncate_descendants_at_depth(&mut self, max_depth: usize) -> bool {
        debug_assert!(max_depth > 0);
        let mut truncated = false;
        let mut pending = vec![(Self::root(), 1_usize)];

        while let Some((node, depth)) = pending.pop() {
            let Some(children) = self.children.get(node.0 as usize) else {
                continue;
            };
            if depth >= max_depth {
                if !children.is_empty() {
                    self.children[node.0 as usize].clear();
                    truncated = true;
                }
                continue;
            }
            pending.extend(
                children
                    .iter()
                    .rev()
                    .copied()
                    .map(|child| (child, depth + 1)),
            );
        }

        if truncated {
            self.compact_reachable_nodes();
        }
        truncated
    }

    /// Remove arena entries no longer reachable from the synthetic root.
    ///
    /// Restructuring may replace provisional child lists.  That is harmless
    /// while the builder is private, but a finite-prefix result must not keep
    /// detached nodes observable through `Document::node_count`.  Node IDs
    /// are intentionally opaque and no public view exists until `finish`, so
    /// rebuilding the private arena is safe.
    fn compact_reachable_nodes(&mut self) {
        let old_node_count = self.document.nodes.len();
        let mut mapping = vec![None; old_node_count];
        let mut order = Vec::new();
        let mut pending = vec![Self::root()];

        while let Some(node) = pending.pop() {
            let index = node.0 as usize;
            if mapping.get(index).is_none() || mapping[index].is_some() {
                continue;
            }
            let next = NodeId(
                u32::try_from(order.len()).expect("reachable node count fits opaque NodeId"),
            );
            mapping[index] = Some(next);
            order.push(node);
            if let Some(children) = self.children.get(index) {
                pending.extend(children.iter().rev().copied());
            }
        }

        let mut nodes = Vec::with_capacity(order.len());
        let mut children = Vec::with_capacity(order.len());
        for old in &order {
            let mut record = self.document.nodes[old.0 as usize].clone();
            record.parent = None;
            record.child_start = 0;
            record.child_len = 0;
            nodes.push(record);
            children.push(Vec::new());
        }
        for old in order {
            let new = mapping[old.0 as usize].expect("reachable node has a new ID");
            let new_children = self.children[old.0 as usize]
                .iter()
                .filter_map(|child| mapping[child.0 as usize])
                .collect::<Vec<_>>();
            for child in &new_children {
                nodes[child.0 as usize].parent = Some(new);
            }
            children[new.0 as usize] = new_children;
        }

        self.document.nodes = nodes;
        self.children = children;
    }

    #[allow(dead_code)] // M2 scanner starts constructing syntax nodes through this builder.
    pub(crate) fn push(&mut self, parent: NodeId, kind: NodeKind) -> Option<NodeId> {
        if parent.0 as usize >= self.document.nodes.len()
            || self.document.nodes.len() >= u32::MAX as usize
        {
            return None;
        }
        let id = NodeId(u32::try_from(self.document.nodes.len()).ok()?);
        let mut record = NodeRecord::root();
        record.kind = kind;
        record.parent = Some(parent);
        self.document.nodes.push(record);
        self.children.push(Vec::new());
        self.children[parent.0 as usize].push(id);
        Some(id)
    }

    #[allow(dead_code)] // M2 scanner starts constructing syntax nodes through this builder.
    pub(crate) fn text(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        let id = StringId(index);
        self.document.strings.push(value.into());
        record.text = Some(id);
        true
    }

    /// Clear temporary scanner text when a token is reclassified as an mdoc
    /// inline macro during private semantic restructuring.
    pub(crate) fn clear_node_text(&mut self, node: NodeId) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.text = None;
        true
    }

    #[allow(dead_code)] // M2 scanner retains control names before macro parsing starts.
    pub(crate) fn macro_name(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        self.document.strings.push(value.into());
        self.document.nodes[node.0 as usize].macro_name = Some(StringId(index));
        true
    }

    #[allow(dead_code)] // M2 scanner supplies physical-line and continuation flags.
    pub(crate) fn flags(&mut self, node: NodeId, flags: NodeFlags) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.flags = flags;
        true
    }

    #[allow(dead_code)] // M2 scanner associates every emitted node with a source span.
    pub(crate) fn location(&mut self, node: NodeId, span: SourceSpan) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        let Some(source) = self.document.sources.get(span.source.0 as usize) else {
            return false;
        };
        if span.end > source.byte_len {
            return false;
        }
        record.location = Some(span);
        true
    }

    pub(crate) fn finish(mut self) -> Document {
        for (index, children) in self.children.into_iter().enumerate() {
            let start = self.document.child_edges.len();
            self.document.child_edges.extend(children);
            self.document.nodes[index].child_start =
                u32::try_from(start).expect("node count bounds the number of child edges");
            self.document.nodes[index].child_len =
                u32::try_from(self.document.child_edges.len() - start)
                    .expect("node count bounds each node's child count");
        }
        // Builders grow geometrically, but a completed document is immutable.
        // Reclaim that transient capacity before it becomes observable memory.
        self.document.nodes.shrink_to_fit();
        self.document.child_edges.shrink_to_fit();
        self.document.strings.shrink_to_fit();
        self.document.sources.shrink_to_fit();
        for source in &mut self.document.sources {
            source.line_starts.shrink_to_fit();
        }
        self.document
    }
}

#[cfg(test)]
mod tests {
    use std::{
        hint::black_box,
        mem::size_of,
        time::{Duration, Instant},
    };

    use crate::{Source, SourceName, SourcePosition, SourceSpan};

    use super::{DocumentBuilder, MacroSet, NodeKind};

    #[derive(Clone)]
    struct RecursiveNode {
        record: super::NodeRecord,
        children: Vec<Self>,
    }

    impl RecursiveNode {
        fn root() -> Self {
            Self {
                record: super::NodeRecord::root(),
                children: Vec::new(),
            }
        }

        fn child(kind: NodeKind) -> Self {
            let mut record = super::NodeRecord::root();
            record.kind = kind;
            Self {
                record,
                children: Vec::new(),
            }
        }
    }

    #[test]
    fn traversal_is_iterative_and_storage_indices_do_not_escape() {
        let mut builder = builder(MacroSet::Man);
        let root = DocumentBuilder::root();
        let section = builder.push(root, NodeKind::Block).unwrap();
        let text = builder.push(section, NodeKind::Text).unwrap();
        assert!(builder.text(text, "visible"));
        let document = builder.finish();

        let kinds = document
            .preorder()
            .map(super::NodeRef::kind)
            .collect::<Vec<_>>();
        assert_eq!(kinds, [NodeKind::Root, NodeKind::Block, NodeKind::Text]);
        let text = document.node(text).unwrap();
        assert_eq!(text.text(), Some("visible"));
        assert_eq!(text.ancestors().count(), 2);
        assert_eq!(document.node_count(), 3);
    }

    #[test]
    fn finite_depth_prefix_keeps_the_legacy_root_counting_boundary() {
        let mut builder = builder(MacroSet::None);
        let mut parent = DocumentBuilder::root();
        for _ in 0..256 {
            parent = builder.push(parent, NodeKind::Element).unwrap();
        }

        assert!(builder.truncate_descendants_at_depth(256));
        let document = builder.finish();
        assert_eq!(document.node_count(), 256);
        assert_eq!(document.preorder().count(), 256);
        assert!(
            document.node(parent).is_none(),
            "discarded node IDs must not remain observable after compaction"
        );
    }

    #[test]
    fn unknown_ids_are_checked_not_indexed_unconditionally() {
        let document = builder(MacroSet::None).finish();
        assert!(document.node(super::NodeId(u32::MAX)).is_none());
        assert_eq!(document.preorder().count(), 1);
    }

    /// Records a transparent M1 storage comparison; run explicitly with
    /// `cargo test -p mantdoc arena_layout_microbenchmark --release -- --ignored --nocapture`.
    #[test]
    #[ignore = "microbenchmark output is recorded in the M0/M1 baseline manifest"]
    fn arena_layout_microbenchmark() {
        const CHILDREN: usize = 50_000;
        const ROUNDS: usize = 100;

        let mut builder = builder(MacroSet::Man);
        let root = DocumentBuilder::root();
        for _ in 0..CHILDREN {
            builder.push(root, NodeKind::Element).unwrap();
        }
        let arena = builder.finish();

        let mut recursive = RecursiveNode::root();
        recursive.children.reserve(CHILDREN);
        for _ in 0..CHILDREN {
            recursive
                .children
                .push(RecursiveNode::child(NodeKind::Element));
        }

        let arena_bytes = arena.nodes.capacity() * size_of::<super::NodeRecord>()
            + arena.child_edges.capacity() * size_of::<super::NodeId>();
        let recursive_bytes = recursive_storage_bytes(&recursive);
        assert!(
            arena_bytes < recursive_bytes,
            "arena must reduce final topology storage"
        );

        let arena_time = time_rounds(ROUNDS, || arena.preorder().count());
        let recursive_time = time_rounds(ROUNDS, || recursive_preorder_count(&recursive));
        println!(
            "arena-layout\tchildren={CHILDREN}\tarena_bytes={arena_bytes}\trecursive_bytes={recursive_bytes}\tarena_ns={}\trecursive_ns={}",
            arena_time.as_nanos() / ROUNDS as u128,
            recursive_time.as_nanos() / ROUNDS as u128,
        );
    }

    fn recursive_storage_bytes(root: &RecursiveNode) -> usize {
        let mut bytes = size_of::<RecursiveNode>();
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            bytes += node.children.capacity() * size_of::<RecursiveNode>();
            pending.extend(&node.children);
        }
        bytes
    }

    fn recursive_preorder_count(root: &RecursiveNode) -> usize {
        let mut count = 0;
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            black_box(node.record.kind);
            count += 1;
            pending.extend(&node.children);
        }
        count
    }

    fn time_rounds(rounds: usize, operation: impl Fn() -> usize) -> Duration {
        let start = Instant::now();
        for _ in 0..rounds {
            black_box(operation());
        }
        start.elapsed()
    }

    fn builder(macro_set: MacroSet) -> DocumentBuilder {
        let name = SourceName::new("test.1").expect("fixed source name");
        DocumentBuilder::new(macro_set, Source::new(&name, b""))
    }

    #[test]
    fn source_ids_resolve_through_document_owned_line_indexes() {
        let name = SourceName::new("manual.1").expect("fixed source name");
        let bytes = b"first\nsecond";
        let mut builder = DocumentBuilder::new(MacroSet::Man, Source::new(&name, bytes));
        let text = builder
            .push(DocumentBuilder::root(), NodeKind::Text)
            .unwrap();
        let span = SourceSpan::new(DocumentBuilder::root_source(), 6, 12).expect("monotonic span");
        assert!(builder.location(text, span.clone()));
        let document = builder.finish();

        assert_eq!(document.source_count(), 1);
        assert_eq!(document.source_name(document.root_source()), Some(&name));
        assert_eq!(
            document.source_position(&span),
            Some(SourcePosition { line: 2, column: 1 })
        );
        assert_eq!(document.node(text).unwrap().location(), Some(&span));
    }
}
