//! Engine-owned recursive syntax projection used by the existing lowerer.
//!
//! This is deliberately a short-lived lowering representation, not a second
//! public parser API. Keeping it engine-owned lets the native arena feed the
//! existing lowering code without coupling it to parser storage types.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MacroSet {
    None,
    Mdoc,
    Man,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NodeKind {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NormalizedListKind {
    Bullet,
    Ordered,
    Definition,
    Column,
    Plain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DisplayKind {
    Literal,
    Filled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NormalizedFont {
    Emphasis,
    Literal,
    Symbolic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AuthorMode {
    Split,
    NoSplit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NormalizedEnclosure {
    pub(super) opening: String,
    pub(super) closing: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TableAlignment {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TableCell {
    pub(super) text: Option<String>,
    pub(super) text_block: bool,
    pub(super) vertical_continuation: bool,
    pub(super) column_span: u16,
    pub(super) row_span: u16,
    pub(super) alignment: TableAlignment,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct NodeFlags {
    pub(super) generated: bool,
    pub(super) sentence_end: bool,
    pub(super) no_print: bool,
    pub(super) no_fill: bool,
    pub(super) deep_link_target: bool,
    pub(super) permalink: bool,
    pub(super) line_start: bool,
    pub(super) delimiter_open: bool,
    pub(super) delimiter_close: bool,
    pub(super) line_continuation: bool,
    pub(super) synopsis_pretty: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Node {
    pub(super) kind: NodeKind,
    pub(super) macro_name: Option<String>,
    pub(super) text: Option<String>,
    pub(super) tag: Option<String>,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) flags: NodeFlags,
    pub(super) list_kind: Option<NormalizedListKind>,
    pub(super) display_kind: Option<DisplayKind>,
    pub(super) font: Option<NormalizedFont>,
    pub(super) author_mode: Option<AuthorMode>,
    pub(super) enclosure: Option<NormalizedEnclosure>,
    pub(super) compact: bool,
    pub(super) offset: Option<String>,
    pub(super) width: Option<String>,
    pub(super) table_cells: Vec<TableCell>,
    pub(super) equation: Option<String>,
    pub(super) children: Vec<Self>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Metadata {
    pub(super) title: Option<String>,
    pub(super) section: Option<String>,
    pub(super) volume: Option<String>,
    pub(super) os: Option<String>,
    pub(super) arch: Option<String>,
    pub(super) name: Option<String>,
    pub(super) date: Option<String>,
    pub(super) alias_target: Option<String>,
    pub(super) has_body: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SyntaxDocument {
    pub(super) macro_set: MacroSet,
    pub(super) metadata: Metadata,
    pub(super) root: Node,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DiagnosticLevel {
    Unsupported,
    Error,
    Warning,
    Style,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SourceLocation {
    pub(super) line: u32,
    pub(super) column: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SyntaxDiagnostic {
    pub(super) level: DiagnosticLevel,
    pub(super) message: String,
    pub(super) location: Option<SourceLocation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ParseReport {
    pub(super) document: SyntaxDocument,
    pub(super) diagnostics: Vec<SyntaxDiagnostic>,
}

// Keep the lowering vocabulary concise.  These aliases intentionally mirror
// the old owned-parser names only inside `mant_engine::mandoc`.
pub(super) use SyntaxDiagnostic as Diagnostic;
pub(super) use SyntaxDocument as Document;
