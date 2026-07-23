//! Owned, renderer-neutral syntax data copied from a completed libmandoc parse.
//!
//! These types contain no C pointers and remain valid after the parser session
//! has been released.  They deliberately describe source semantics rather than
//! imposing a presentation model on downstream renderers.

/// High-level macro package detected by libmandoc.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacroSet {
    None,
    Mdoc,
    Man,
}

/// Renderer-neutral node role copied from the libmandoc syntax tree.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeKind {
    Root,
    Block,
    Head,
    Body,
    Tail,
    Element,
    Text,
    Comment,
    Table,
    Equation,
}

/// Normalized mdoc list behavior copied independently of upstream enum values.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalizedListKind {
    Bullet,
    Ordered,
    Definition,
    Column,
    Plain,
}

/// Whether an mdoc display preserves source line layout.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayKind {
    Literal,
    Filled,
}

/// Horizontal alignment retained for one parsed tbl(7) cell.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableAlignment {
    Left,
    Center,
    Right,
}

/// Owned payload of one cell in a libmandoc table row.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableCell {
    pub text: Option<String>,
    pub column_span: u16,
    pub row_span: u16,
    pub alignment: TableAlignment,
}

/// Source and renderer flags needed by a lowering or rendering pass.
#[allow(clippy::struct_excessive_bools)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NodeFlags {
    pub generated: bool,
    pub sentence_end: bool,
    pub no_print: bool,
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
}

/// An owned syntax node with no pointers into the C parser.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    pub macro_name: Option<String>,
    pub text: Option<String>,
    /// Canonical same-document tag assigned during libmandoc validation.
    pub tag: Option<String>,
    pub line: u32,
    pub column: u32,
    pub flags: NodeFlags,
    pub list_kind: Option<NormalizedListKind>,
    pub display_kind: Option<DisplayKind>,
    pub compact: bool,
    pub offset: Option<String>,
    /// Normalized mdoc(7) list width, including its roff scale suffix.
    pub width: Option<String>,
    pub table_cells: Vec<TableCell>,
    pub equation: Option<String>,
    pub children: Vec<Self>,
}

/// Metadata copied from a completed libmandoc parse.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Metadata {
    pub title: Option<String>,
    pub section: Option<String>,
    pub volume: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub name: Option<String>,
    pub date: Option<String>,
    pub alias_target: Option<String>,
    pub has_body: bool,
}

/// Complete owned output of the low-level parser, excluding diagnostics.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Document {
    pub macro_set: MacroSet,
    pub metadata: Metadata,
    pub root: Node,
}
