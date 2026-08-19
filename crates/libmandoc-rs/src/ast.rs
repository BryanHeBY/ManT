//! Owned, renderer-neutral syntax data copied from a completed libmandoc parse.
//!
//! These types contain no C pointers and remain valid after the parser session
//! has been released.  They deliberately describe source semantics rather than
//! imposing a presentation model on downstream renderers.

/// High-level macro package detected by libmandoc.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacroSet {
    /// No supported semantic macro package was detected.
    None,
    /// The source uses the semantic mdoc(7) macro package.
    Mdoc,
    /// The source uses the traditional man(7) macro package.
    Man,
}

/// Renderer-neutral node role copied from the libmandoc syntax tree.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeKind {
    /// Synthetic root containing the complete syntax tree.
    Root,
    /// A macro block, such as a section or display.
    Block,
    /// The heading or term portion of a block.
    Head,
    /// The principal content portion of a block.
    Body,
    /// The trailing portion of a block, when the macro defines one.
    Tail,
    /// A leaf-level semantic macro invocation.
    Element,
    /// Literal source text after roff escape processing.
    Text,
    /// A source comment retained by libmandoc.
    Comment,
    /// A tbl(7) table node.
    Table,
    /// An eqn(7) equation node.
    Equation,
}

/// Normalized mdoc list behavior copied independently of upstream enum values.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalizedListKind {
    /// An unordered list whose items carry bullets.
    Bullet,
    /// An ordered list whose items carry ordinal markers.
    Ordered,
    /// A term-and-description list.
    Definition,
    /// A list laid out as aligned columns.
    Column,
    /// A marker-free list.
    Plain,
}

/// Whether an mdoc display preserves source line layout.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayKind {
    /// Preserve input line breaks and horizontal whitespace.
    Literal,
    /// Reflow content as filled prose.
    Filled,
}

/// Normalized font selected by an mdoc `Bf` block.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalizedFont {
    /// Typographic emphasis.
    Emphasis,
    /// Literal or fixed-width text.
    Literal,
    /// Symbolic text, conventionally rendered in bold.
    Symbolic,
}

/// Explicit author layout mode selected by an mdoc `An` control macro.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorMode {
    /// Render each subsequent author separately.
    Split,
    /// Keep subsequent authors in a continuous group.
    NoSplit,
}

/// Delimiters selected by the obsolete mdoc `Es`/`En` enclosure pair.
///
/// libmandoc resolves the stateful `Es` definition while validating each
/// `En` invocation. Copying that result keeps downstream renderers from
/// replaying formatter state or exposing the non-printing `Es` arguments.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedEnclosure {
    /// Visible opening delimiter.
    pub opening: String,
    /// Visible closing delimiter, when the definition supplied one.
    pub closing: Option<String>,
}

/// Horizontal alignment retained for one parsed tbl(7) cell.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableAlignment {
    /// Align cell content to the left edge.
    Left,
    /// Center cell content horizontally.
    Center,
    /// Align cell content to the right edge.
    Right,
}

/// Owned payload of one cell in a libmandoc table row.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableCell {
    /// Visible cell content, or `None` for a spanning/empty cell.
    pub text: Option<String>,
    /// The cell was written using a multiline tbl(7) `T{`/`T}` text block.
    pub text_block: bool,
    /// Number of logical columns occupied by the cell.
    pub column_span: u16,
    /// Number of logical rows occupied by the cell.
    pub row_span: u16,
    /// Horizontal alignment requested by tbl(7).
    pub alignment: TableAlignment,
}

/// Source and renderer flags needed by a lowering or rendering pass.
#[allow(clippy::struct_excessive_bools)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NodeFlags {
    /// The node was synthesized by libmandoc rather than written explicitly.
    pub generated: bool,
    /// The node ends a sentence according to libmandoc punctuation rules.
    pub sentence_end: bool,
    /// The node must not contribute visible output.
    pub no_print: bool,
    /// The node belongs to a no-fill region that preserves source lines.
    pub no_fill: bool,
    /// libmandoc selected this node as a same-document destination.
    pub deep_link_target: bool,
    /// libmandoc renders a self-link for this destination.
    pub permalink: bool,
    /// This node begins a roff input line (`NODE_LINE`).
    ///
    /// Some man macros keep same-line layout arguments and next-line visible
    /// content in one syntax head, so source-line role is semantic data.
    pub line_start: bool,
    /// This text node is opening punctuation and suppresses spacing after it.
    pub delimiter_open: bool,
    /// This text node is closing punctuation and suppresses spacing before it.
    pub delimiter_close: bool,
    /// This text node ends with the roff `\c` escape and joins the next input
    /// line without an implicit space or line break.
    pub line_continuation: bool,
    /// libmandoc selected synopsis-style presentation for this node.
    ///
    /// Some semantic punctuation is generated only in this context, notably
    /// the terminating semicolon of mdoc `Fn` and `Fo` declarations.
    pub synopsis_pretty: bool,
}

/// An owned syntax node with no pointers into the C parser.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    /// Structural role of this node in the libmandoc tree.
    pub kind: NodeKind,
    /// Source macro name, without the leading dot, when applicable.
    pub macro_name: Option<String>,
    /// Visible text carried by a text node.
    pub text: Option<String>,
    /// Canonical same-document tag assigned during libmandoc validation.
    pub tag: Option<String>,
    /// One-based source line reported by libmandoc, or zero when unavailable.
    pub line: u32,
    /// One-based source column reported by libmandoc, or zero when unavailable.
    pub column: u32,
    /// Source and renderer flags attached to the node.
    pub flags: NodeFlags,
    /// Normalized list behavior for an mdoc list block.
    pub list_kind: Option<NormalizedListKind>,
    /// Fill behavior for an mdoc display block.
    pub display_kind: Option<DisplayKind>,
    /// Font selected by an mdoc font block.
    pub font: Option<NormalizedFont>,
    /// Author layout mode selected by an mdoc author macro.
    pub author_mode: Option<AuthorMode>,
    /// Stateful delimiters resolved for an mdoc `En` invocation.
    pub enclosure: Option<NormalizedEnclosure>,
    /// Whether the enclosing list requests compact vertical layout.
    pub compact: bool,
    /// Raw normalized display/list offset, including a roff scale suffix.
    pub offset: Option<String>,
    /// Normalized mdoc(7) list width, including its roff scale suffix.
    pub width: Option<String>,
    /// Cells copied from a tbl(7) row represented by this node.
    pub table_cells: Vec<TableCell>,
    /// Normalized eqn(7) expression carried by this node.
    pub equation: Option<String>,
    /// Child nodes in source order.
    pub children: Vec<Self>,
}

/// Metadata copied from a completed libmandoc parse.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Metadata {
    /// Canonical manual title, normally derived from `TH` or `Dt`.
    pub title: Option<String>,
    /// Native manual category such as `1` or `3p`.
    pub section: Option<String>,
    /// Manual volume or collection label.
    pub volume: Option<String>,
    /// Operating-system label declared by the page.
    pub os: Option<String>,
    /// Architecture qualifier declared by the page.
    pub arch: Option<String>,
    /// Primary display name extracted from the NAME section.
    pub name: Option<String>,
    /// Normalized source date when libmandoc recognized it.
    pub date: Option<String>,
    /// Target named by a top-level `.so` alias page.
    pub alias_target: Option<String>,
    /// Whether the parsed source produced a document body.
    pub has_body: bool,
}

/// Complete owned output of the low-level parser, excluding diagnostics.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Document {
    /// Macro package selected for the source.
    pub macro_set: MacroSet,
    /// Metadata validated and normalized by libmandoc.
    pub metadata: Metadata,
    /// Root of the owned syntax tree.
    pub root: Node,
}
