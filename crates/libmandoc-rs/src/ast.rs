//! Owned, renderer-neutral syntax data transferred from a completed libmandoc parse.
//!
//! The private FFI layer retains the completed parser only while it copies
//! shallow borrowed snapshots directly into these types. No C pointer escapes
//! that synchronous transfer, so the data remains valid after the parser
//! session has been released. It deliberately describes source semantics
//! rather than imposing a presentation model on downstream renderers.

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

/// Native structural kind of an eqn(7) box.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EquationBoxKind {
    /// Text, number, variable, symbol, or operator payload.
    Text,
    /// A positional or fractional subexpression.
    Subexpression,
    /// An ordered expression list, including braced expressions.
    List,
    /// A vertical pile.
    Pile,
    /// A matrix whose structural children are columns.
    Matrix,
}

/// Font retained on an eqn(7) structural box.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EquationFont {
    /// Inherit the surrounding equation font.
    None,
    /// Upright roman text.
    Roman,
    /// Bold text.
    Bold,
    /// Extra-heavy text, retained separately from bold.
    Fat,
    /// Italic text.
    Italic,
}

/// Positional relation retained on an eqn(7) structural box.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EquationPosition {
    /// No positional operator.
    None,
    /// Superscript relation.
    Superscript,
    /// Combined subscript and superscript relation.
    SubscriptSuperscript,
    /// Subscript relation.
    Subscript,
    /// Upper-limit relation.
    To,
    /// Lower-limit relation.
    From,
    /// Combined lower- and upper-limit relation.
    FromTo,
    /// Fraction relation.
    Over,
    /// Square-root relation.
    SquareRoot,
}

/// One fully owned box in the native eqn(7) syntax tree.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EquationBox {
    /// Structural box kind.
    pub kind: EquationBoxKind,
    /// Native font declaration.
    pub font: EquationFont,
    /// Native positional relation.
    pub position: EquationPosition,
    /// Native font size; `i32::MIN` retains the upstream default sentinel.
    pub size: i32,
    /// Maximum argument count; upstream uses `u32::MAX` for an open-ended box.
    pub expected_args: u64,
    /// Actual direct-child count reported by the native parser.
    pub actual_args: u64,
    /// Optional box text payload.
    pub text: Option<String>,
    /// Optional explicit left fence.
    pub left: Option<String>,
    /// Optional explicit right fence.
    pub right: Option<String>,
    /// Optional decoration rendered above the box.
    pub top: Option<String>,
    /// Optional decoration rendered below the box.
    pub bottom: Option<String>,
    /// Direct children in native order.
    pub children: Vec<Self>,
}

/// Fully owned native eqn(7) structure carried by an equation node.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Equation {
    /// Artificial root box retained even for configuration-only and empty equations.
    pub root: EquationBox,
}

impl Equation {
    /// Derive the legacy renderer-neutral text projection from the retained tree.
    ///
    /// The tree remains authoritative; this one-way view exists for consumers
    /// that do not need mathematical structure.
    #[must_use]
    pub fn normalized_text(&self) -> String {
        let mut output = String::new();
        append_normalized_equation_box(&self.root, &mut output);
        output
    }
}

fn append_normalized_equation_box(box_value: &EquationBox, output: &mut String) {
    if box_value.position == EquationPosition::SquareRoot {
        output.push_str("sqrt(");
    }
    if let Some(left) = &box_value.left {
        output.push_str(left);
    }
    if let Some(text) = &box_value.text {
        output.push_str(if text == "ldots" { "..." } else { text });
    }
    let mut children = box_value.children.iter();
    if box_value.position == EquationPosition::SquareRoot {
        if let Some(child) = children.next() {
            append_normalized_equation_box(child, output);
        }
    } else if box_value.kind == EquationBoxKind::Subexpression
        && box_value.position != EquationPosition::None
    {
        if let Some(child) = children.next() {
            append_normalized_equation_box(child, output);
            output.push_str(match box_value.position {
                EquationPosition::Over => " / ",
                EquationPosition::Superscript | EquationPosition::To => " ^ ",
                _ => " _ ",
            });
        }
        if let Some(child) = children.next() {
            append_normalized_equation_box(child, output);
        }
        if matches!(
            box_value.position,
            EquationPosition::FromTo | EquationPosition::SubscriptSuperscript
        ) && let Some(child) = children.next()
        {
            output.push_str(" ^ ");
            append_normalized_equation_box(child, output);
        }
    } else {
        for (index, child) in box_value.children.iter().enumerate() {
            if index != 0 {
                output.push(' ');
            }
            append_normalized_equation_box(child, output);
        }
    }
    if let Some(top) = &box_value.top {
        output.push_str(top);
    }
    if box_value.bottom.is_some() {
        output.push('_');
    }
    if let Some(right) = &box_value.right {
        output.push_str(right);
    }
    if box_value.position == EquationPosition::SquareRoot {
        output.push(')');
    }
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

/// Source-level layout style of an mdoc definition list.
///
/// All variants lower to [`NormalizedListKind::Definition`], but retaining
/// the authored style lets semantic consumers distinguish tagged definitions
/// from diagnostic or hanging presentation without reparsing source text.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefinitionListStyle {
    /// `Bl -tag`: terms occupy a measured tag column.
    Tag,
    /// `Bl -diag`: terms use diagnostic-message presentation.
    Diagnostic,
    /// `Bl -hang`: descriptions hang from their terms.
    Hang,
    /// `Bl -inset`: terms are inset without a fixed tag column.
    Inset,
    /// `Bl -ohang`: descriptions begin below overhanging terms.
    Overhang,
}

/// Whether an mdoc display preserves source line layout.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayKind {
    /// Preserve input line breaks and horizontal whitespace.
    Literal,
    /// Preserve input line breaks without literal tab semantics.
    Unfilled,
    /// Reflow content as filled prose.
    Filled,
    /// Reflow content without right-margin adjustment.
    Ragged,
    /// Center each output line.
    Centered,
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

/// Effective native tbl cell content after layout-rule precedence.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableCellKind {
    /// Printable data (subject to vertical continuation).
    Text,
    /// Uninitialized data, with no printable payload.
    Empty,
    /// A connecting single horizontal rule.
    HorizontalRule,
    /// A connecting double horizontal rule.
    DoubleHorizontalRule,
    /// An isolated single horizontal rule.
    IsolatedHorizontalRule,
    /// An isolated double horizontal rule.
    IsolatedDoubleHorizontalRule,
}

/// Native tbl(7) layout role before data precedence is applied.
///
/// CVS keeps this fact in `tbl_cell::pos` independently from the payload kind
/// in `tbl_dat::pos`.  Consumers must retain both: a layout rule can suppress
/// an otherwise ordinary text payload without changing that payload's kind.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableCellLayoutKind {
    /// Centered data column.
    Center,
    /// Right-aligned data column.
    Right,
    /// Left-aligned data column.
    Left,
    /// Numeric alignment around a decimal point.
    Numeric,
    /// Horizontal continuation of the preceding cell.
    Span,
    /// Long-cell indentation.
    Long,
    /// Vertical continuation of a preceding cell.
    Down,
    /// Single horizontal layout rule.
    HorizontalRule,
    /// Double horizontal layout rule.
    DoubleHorizontalRule,
}

/// Native tbl(7) data role before layout precedence is applied.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableCellDataKind {
    /// No initialized payload.
    Empty,
    /// Ordinary text payload.
    Text,
    /// Connecting single horizontal rule.
    HorizontalRule,
    /// Connecting double horizontal rule.
    DoubleHorizontalRule,
    /// Isolated single horizontal rule.
    IsolatedHorizontalRule,
    /// Isolated double horizontal rule.
    IsolatedDoubleHorizontalRule,
}

/// Native kind of one tbl(7) row.
///
/// Empty data rows and whole-row rules both contain no cells, so consumers
/// must not infer this distinction from [`Node::table_cells`].
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TableRowKind {
    /// A data row, including an intentionally empty input line.
    Data,
    /// A whole-row single horizontal rule (`_`).
    HorizontalRule,
    /// A whole-row double horizontal rule (`=`).
    DoubleHorizontalRule,
    /// A rule row authored in the tbl layout, retaining one rule strength
    /// for each logical column.  CVS represents this as an otherwise empty
    /// data span rather than as a whole-row rule span.
    LayoutRule {
        /// Rule strengths in logical column order.
        cells: Vec<TableRuleCellKind>,
    },
}

/// Horizontal rule strength for one cell of a layout-only tbl row.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableRuleCellKind {
    /// A single horizontal rule (`_` or `-`).
    Horizontal,
    /// A double horizontal rule (`=`).
    DoubleHorizontal,
}

/// Owned payload of one cell in a libmandoc table row.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableCell {
    /// Native content kind; rule cells suppress even a nonempty text payload.
    pub kind: TableCellKind,
    /// Original layout role from `tbl_cell::pos`.
    pub layout_kind: TableCellLayoutKind,
    /// Original payload role from `tbl_dat::pos`.
    pub data_kind: TableCellDataKind,
    /// Native cell payload, or `None` for a spanning/empty cell.
    /// Only printable when [`Self::kind`] is [`TableCellKind::Text`] and the
    /// cell is not a vertical continuation.
    pub text: Option<String>,
    /// Direct post-comment, pre-expansion source admitted to this `T{}` cell.
    ///
    /// This is optional evidence for a bounded semantic overlay. The native
    /// [`Self::text`] remains the authoritative executed operand.
    pub source: Option<String>,
    /// One-based first direct source line retained for this cell.
    pub source_line: Option<u32>,
    /// One-based first direct source column retained for this cell.
    pub source_column: Option<u32>,
    /// One-based line containing the exclusive end of this source interval.
    /// For a complete text block, this is the closing `T}` line.
    pub source_end_line: Option<u32>,
    /// One-based exclusive-end column of the syntactic ownership envelope.
    /// For a complete text block, this is the column immediately after the
    /// closing `T}` sentinel.
    pub source_end_column: Option<u32>,
    /// Escape byte active when the retained source interval began.
    pub source_escape: Option<u8>,
    /// The cell was written using a multiline tbl(7) `T{`/`T}` text block.
    pub text_block: bool,
    /// Whether the complete native input for this cell bypassed user-defined
    /// or renamed roff macros.
    ///
    /// This is a cell-local execution fact. A source-backed consumer may use
    /// it to enrich a closed inline fragment only when its own operands also
    /// corroborate this native payload; it is not a claim about sibling cells
    /// or the table row as a whole.
    pub source_recovery_safe: bool,
    /// This cell continues a vertical span owned by a cell in an earlier row.
    ///
    /// tbl(7) permits both a `^` layout cell and a literal `\^` data cell for
    /// this purpose. Neither spelling produces printable cell content.
    pub vertical_continuation: bool,
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
    /// First data row of a distinct native tbl table, even after leading
    /// rule-only rows. A `T&` layout change does not begin another table.
    pub table_start: bool,
}

/// An owned syntax node with no pointers into the C parser.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    /// Report-local key assigned when this tree was copied together with a
    /// native execution report. Ordinary parse-only trees have no key.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub execution_node_key: Option<u32>,
    /// Structural role of this node in the libmandoc tree.
    pub kind: NodeKind,
    /// Source macro name, without the leading dot, when applicable.
    pub macro_name: Option<String>,
    /// Visible text carried by a text node, with libmandoc's internal break,
    /// discretionary-hyphen, and non-breaking-space sentinels normalized.
    pub text: Option<String>,
    /// Canonical same-document tag assigned during libmandoc validation, with
    /// libmandoc's internal text sentinels normalized.
    pub tag: Option<String>,
    /// One-based source line reported by libmandoc, or zero when unavailable.
    pub line: u32,
    /// One-based source column reported by libmandoc, or zero when unavailable.
    pub column: u32,
    /// Per-document generation of executed paragraph/container boundaries.
    ///
    /// Recorded when the native node is allocated, after conditionals and
    /// user macros execute but before empty paragraphs can be removed. Equal
    /// values on adjacent declaration blocks mean no intervening executed
    /// flow boundary. This is not a source line or a persistent identity;
    /// generations are only comparable within the same parse report.
    pub flow_epoch: usize,
    /// Escape character active when the native parser read this tbl(7) row.
    ///
    /// `Some(0)` means `.eo` had disabled escapes. `None` means the node is
    /// not a table row. This is an execution fact, not a source scan.
    pub table_escape: Option<u8>,
    /// Whether native tbl input for this row reached tbl without user macro
    /// expansion or request renaming.
    ///
    /// This is a parser execution fact. It lets a downstream presentation
    /// layer enrich a closed high-level macro fragment only when the original
    /// spelling and the native table payload have the same provenance.
    pub table_source_recovery_safe: bool,
    /// Native tbl row kind, when this node represents a table span.
    pub table_row_kind: Option<TableRowKind>,
    /// Source and renderer flags attached to the node.
    pub flags: NodeFlags,
    /// Normalized list behavior for an mdoc list block.
    pub list_kind: Option<NormalizedListKind>,
    /// Authored mdoc definition-list style, when [`Self::list_kind`] is
    /// [`NormalizedListKind::Definition`].
    pub definition_list_style: Option<DefinitionListStyle>,
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
    /// Native eqn(7) structure carried by this node.
    pub equation: Option<Box<Equation>>,
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
