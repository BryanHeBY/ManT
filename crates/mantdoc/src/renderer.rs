//! Bounded native reference rendering built on the public arena view.

// This module deliberately keeps terminal state machines and the pinned device
// character catalogue contiguous. Splitting either by arbitrary line count or
// merging equal catalogue spellings obscures source-order device semantics.
#![allow(clippy::match_same_arms, clippy::too_many_lines)]

use std::{collections::BTreeMap, fmt, path::Path};

use unicode_width::UnicodeWidthChar;

use crate::preprocess::normalize_equation_symbol;

use crate::{
    AuthorMode, Compression, Diagnostic, DisplayKind, Document, FatalError, Limits, MacroSet,
    NodeKind, NodeRef, NormalizedFont, NormalizedListKind, ParseReport, Parser, Source,
    SourceBundle, SourceName, TableAlignment,
    ast::{
        EquationTerminal, EquationTerminalToken, MdocListMarker, TableTerminalBorder,
        TableTerminalFont, TableTerminalRow,
    },
};

/// Default maximum bytes retained for one renderer call.
pub const DEFAULT_RENDER_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
/// Default terminal width for text render formats.
pub const DEFAULT_RENDER_WIDTH: usize = 78;
/// Smallest accepted terminal width.
pub const MIN_RENDER_WIDTH: usize = 20;
/// Largest accepted terminal width.
pub const MAX_RENDER_WIDTH: usize = 1_000;
/// Largest accepted caller output budget.
pub const MAX_RENDER_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

// Internal line marker removed before returning output. It lets the terminal
// walker preserve no-fill source lines until the final width pass without
// making arena storage or the public renderer API carry layout buffers.
const TERMINAL_NO_WRAP_MARKER: char = '\u{1e}';
// A second private marker retains expanded tab fields. They are already
// terminal-positioned and must not be collapsed by ordinary prose reflow.
const TERMINAL_KEEP_SPACING_MARKER: char = '\u{1d}';
// `Bd -literal` uses physical eight-column tabs, unlike the relative fields
// used for ordinary no-fill and unfilled mdoc text.
const TERMINAL_LITERAL_TAB_MARKER: char = '\u{1c}';
// A word-sized marker lets the final width pass retain the terminal device's
// double inter-sentence spacing without making all filled prose no-wrap.
const TERMINAL_SENTENCE_SPACE_MARKER: char = '\u{1b}';
// Roff's `\:` is a zero-width discretionary terminal break.
const TERMINAL_OPTIONAL_BREAK_MARKER: char = '\u{1a}';
// mdoc's `No` macro keeps its arguments together even when an ordinary
// terminal word would otherwise be eligible for a hyphen break.  The marker
// is placed after every authored hyphen and removed by the final width pass.
const TERMINAL_NO_HYPHEN_BREAK_MARKER: char = '\u{19}';
// `\\~`, `\\0`, and `\\ ` occupy one terminal cell but never split a word
// during filled-text wrapping.  It is converted back to a plain space only
// after layout has selected the physical line breaks.
const TERMINAL_NONBREAKING_SPACE_MARKER: char = '\u{18}';
// A sentence boundary from normalized AST flags can precede an attached
// closing delimiter.  Keep it private until the following ordinary phrase
// requests its two-cell terminal separator.
const TERMINAL_SENTENCE_PENDING_MARKER: char = '\u{17}';
// `\z` motion is resolved before font formatting; defer its actual terminal
// backspace until after that formatting so a bold/italic zero-width glyph
// remains an ordinary device overstrike rather than a styled control byte.
const TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER: char = '\u{e004}';
// Quoted/numeric renderer escapes can resolve to a backslash immediately
// before an authored escape.  Protect that resolved scalar until the generic
// one-pass roff normalizer has consumed the authored input, otherwise the
// newly emitted byte would spuriously become a second escape introducer.
const RENDER_LITERAL_BACKSLASH_MARKER: char = '\u{16}';
// `.Ns` owns no text of its own; it suppresses the separator before the next
// visible node.  Retain that state in the terminal buffer instead of adding a
// mutable parser/render session flag.
const TERMINAL_ATTACH_NEXT_MARKER: char = '\u{15}';
// Roff `\p` completes a terminal field at a word boundary.  It stays private
// until `append_terminal_text()` can supply the destination field's indent.
const TERMINAL_PENDING_LINE_BREAK_MARKER: char = '\u{e002}';
// Literal punctuation emitted by selected mdoc macros is not an
// end-of-sentence boundary for the terminal device.  Keep that one-token
// state in the private layout buffer, rather than weakening normal prose
// sentence handling globally.
const TERMINAL_LITERAL_PUNCTUATION_MARKER: char = '\u{14}';
// mdoc's empty explicit enclosure is a terminal zero-width word. It keeps
// the preceding and following word separators independently observable.
const TERMINAL_EMPTY_WORD_MARKER: char = '\u{13}';
// An opening-only explicit enclosure clears parser attachment, but forces the
// following sibling to receive one ordinary terminal separator.
const TERMINAL_FORCE_SEPARATOR_MARKER: char = '\u{12}';
// A recovered display may end inside an open mdoc enclosure.  Its next
// source-line sibling stays in that enclosure's terminal phrase, so bypass
// the usual `line_start` break while retaining one ordinary separator.
const TERMINAL_CONTINUE_SOURCE_LINE_MARKER: char = '\u{e005}';
// A paired private marker records one pending roff `.ti` target column. The
// renderer places normal structural indentation after it; the width pass uses
// the encoded value only for the first physical output line.
const TERMINAL_TEMPORARY_INDENT_MARKER: char = '\u{11}';
// A paired private marker records a man `.HP` continuation column. The first
// terminal line keeps its enclosing structural indent, while wrapped and
// explicit-break continuation lines use the encoded hanging field.
const TERMINAL_HANGING_INDENT_MARKER: char = '\u{10}';
// A negative `.sp` does not remove already-emitted lines.  Instead, mandoc's
// terminal device remembers how many *future* vertical spaces to suppress.
// Keep that small device state beside the pending physical line break so it
// remains local to one rendering buffer and survives ordinary filled prose.
const TERMINAL_VERTICAL_SKIP_MARKER: char = '\u{e}';
// A completed tbl device field suppresses only the next ordinary vertical
// slot. Unlike a negative `.sp`, a following section or explicit `.sp` owns
// its own spacing and clears this table-local marker first.
const TERMINAL_TABLE_VERTICAL_SKIP_MARKER: char = '\u{e001}';
// A paired private marker carries one source-ordered roff `.ta` request to
// the terminal width pass.  Tab stops are terminal-device state, but the
// public AST deliberately remains renderer-neutral.
const TERMINAL_TAB_STOPS_MARKER: char = '\u{e003}';
// `Bd -centered` is publicly a filled display but centers each completed
// device line inside its own offset field.  Keep that device state private to
// the renderer width pass.
const TERMINAL_CENTER_MARKER: char = '\u{f}';
// Roff's `.rj` consumes the following physical input lines and aligns their
// terminal glyphs to the device right margin.  Like centering, this stays a
// width-pass concern instead of leaking into the public compatible AST.
const TERMINAL_RIGHT_MARKER: char = '\u{c}';
// A paired marker records the effective roff `.ll` line length for the raw
// terminal line.  It is interpreted only by the final width pass so the
// public AST continues to expose the original request and arguments.
const TERMINAL_LINE_LENGTH_MARKER: char = '\u{b}';
// Mdoc's `.Sm off` persists independently of source lines and structural
// blocks. Keeping it at the start of the private render buffer makes the
// state survive ordinary appends and newlines without shared renderer state.
const TERMINAL_NO_SPACE_MARKER: char = '\u{d}';

#[derive(Clone, Copy, Eq, PartialEq)]
enum TerminalMdocSmRelink {
    Valid,
    Invalid,
}

#[derive(Clone, Copy, Default)]
enum TerminalTabLayout {
    #[default]
    Relative,
    PhysicalLiteral,
}

/// Stateful terminal tab configuration reconstructed from `.ta` requests.
/// The explicit positions are absolute; positions after `T` repeat relative
/// to the last established tab stop, exactly like `term_tab_next()`.
#[derive(Clone, Default)]
struct TerminalTabStops {
    absolute: Vec<usize>,
    periodic: Vec<usize>,
    configured: bool,
}

#[derive(Clone, Copy, Default)]
enum TerminalJoin {
    #[default]
    Separate,
    Attach,
}

#[derive(Clone, Copy, Default)]
#[allow(clippy::struct_excessive_bools)] // Compact terminal layout state is traversed in source order.
struct TerminalTextLayout {
    line_start: bool,
    join: TerminalJoin,
    no_fill: bool,
    /// A later owned token on the same physical no-fill source line.
    no_fill_continuation: bool,
    keep_spacing: bool,
    sentence_end: bool,
    literal_punctuation: bool,
    tabs: TerminalTabLayout,
}

#[derive(Clone, Copy, Default)]
enum TerminalFont {
    #[default]
    Roman,
    Bold,
    Italic,
    BoldItalic,
}

#[derive(Clone, Copy)]
enum TerminalFontChange {
    Set(TerminalFont),
    Restore,
}

#[derive(Clone, Copy, Default)]
enum HtmlFont {
    #[default]
    Roman,
    Bold,
    Italic,
    BoldItalic,
    LiteralRoman,
    LiteralBold,
    LiteralItalic,
}

#[derive(Clone, Copy)]
enum HtmlFontChange {
    Set(HtmlFont),
    Restore,
}

/// HTML keeps the full roff font family, including literal (`C*`) variants,
/// so its request register is separate from the terminal's reduced font
/// state.  It is reconstructed from immutable source-order siblings.
#[derive(Clone, Copy, Default)]
struct HtmlRequestFontState {
    current: HtmlFont,
    previous: HtmlFont,
}

/// The terminal device keeps `.ft` selection independently from structural
/// mdoc font scopes.  The renderer reconstructs this small state from earlier
/// document-order requests whenever it reaches a text node, keeping rendering
/// calls re-entrant and avoiding process- or thread-global formatter state.
#[derive(Clone, Copy, Default)]
struct TerminalRequestFontState {
    current: TerminalFont,
    previous: TerminalFont,
}

/// Roff's `.po` retains both its current raw page offset and the previous
/// request value.  An invalid request restores that previous value, while
/// application to an individual device field performs the terminal bounds
/// clamp only at render time.
#[derive(Clone, Copy, Default)]
struct TerminalPageOffsetState {
    current: isize,
    previous: isize,
}

/// The roff `.in` request owns a physical terminal field independently of
/// structural AST indentation. `None` means normal enclosing layout; a stored
/// value is an absolute device column retained across later siblings.
#[derive(Clone, Copy, Default)]
struct TerminalRequestIndentState {
    current: Option<isize>,
}

/// Roff `.ll` is expressed either as an absolute terminal line length or as
/// a delta from the renderer's configured width.  Keeping the latter symbolic
/// lets `.ll` preserve the caller-selected device width rather than assuming
/// the standard 78 columns while reconstructing node-local state.
#[derive(Clone, Copy, Default)]
enum TerminalLineLength {
    #[default]
    Default,
    Absolute(usize),
    Relative(isize),
}

/// Native reference output format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderFormat {
    /// Seven-bit terminal-oriented text.
    Ascii,
    /// UTF-8 terminal-oriented text.
    Utf8,
    /// Escaped HTML5 output.
    Html,
}

/// Complete bounded output and the recoverable parser findings that produced it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderReport {
    /// Complete output; an overflow never returns a partial value.
    pub output: String,
    /// Parser diagnostics retained for the rendered source.
    pub diagnostics: Vec<Diagnostic>,
}

/// Stable renderer failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderErrorKind {
    /// Renderer width or output budget is invalid.
    InvalidOptions,
    /// Parsing or explicit transport input failed.
    Parse,
    /// The complete rendered output exceeds its configured budget.
    OutputLimit,
}

/// A fatal native renderer failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderError {
    /// Machine-readable error category.
    pub kind: RenderErrorKind,
    /// Human-readable detail.
    pub message: Box<str>,
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RenderError {}

/// Pure-Rust bounded renderer configured independently from parser storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Renderer {
    parser: Parser,
    format: RenderFormat,
    width: usize,
    max_output_bytes: usize,
    html_fragment: bool,
}

impl Renderer {
    /// Create a renderer with the default parser, width, and output budget.
    #[must_use]
    pub fn new(format: RenderFormat) -> Self {
        Self {
            parser: Parser::default(),
            format,
            width: DEFAULT_RENDER_WIDTH,
            max_output_bytes: DEFAULT_RENDER_OUTPUT_BYTES,
            html_fragment: false,
        }
    }

    /// Replace the immutable parser session configuration.
    #[must_use]
    pub fn with_parser(mut self, parser: Parser) -> Self {
        self.parser = parser;
        self
    }

    /// Set text-format width.
    #[must_use]
    pub const fn with_width(mut self, width: usize) -> Self {
        self.width = width;
        self
    }

    /// Set the maximum retained complete output size.
    #[must_use]
    pub const fn with_max_output_bytes(mut self, maximum: usize) -> Self {
        self.max_output_bytes = maximum;
        self
    }

    /// Select an HTML fragment instead of a complete document.
    #[must_use]
    pub const fn with_html_fragment(mut self, value: bool) -> Self {
        self.html_fragment = value;
        self
    }

    /// Return the configured parser.
    #[must_use]
    pub const fn parser(&self) -> &Parser {
        &self.parser
    }

    /// Return the selected format.
    #[must_use]
    pub const fn format(&self) -> RenderFormat {
        self.format
    }

    /// Return the text width.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Return the complete-output budget.
    #[must_use]
    pub const fn max_output_bytes(&self) -> usize {
        self.max_output_bytes
    }

    /// Return whether HTML is emitted as a fragment.
    #[must_use]
    pub const fn html_fragment(&self) -> bool {
        self.html_fragment
    }

    /// Parse and render borrowed plain source bytes.
    ///
    /// # Errors
    ///
    /// Returns a typed error for invalid renderer options, a fatal parser
    /// failure, or complete output exceeding the configured budget.
    pub fn render(&self, source: Source<'_>) -> Result<RenderReport, RenderError> {
        self.validate()?;
        self.finish(self.parser.parse(source))
    }

    /// Parse explicit transport bytes and render them.
    ///
    /// # Errors
    ///
    /// Returns a typed error for invalid renderer options, unavailable or
    /// malformed transport data, a fatal parser failure, or output overflow.
    pub fn render_bytes(
        &self,
        name: &SourceName,
        bytes: &[u8],
        compression: Compression,
    ) -> Result<RenderReport, RenderError> {
        self.validate()?;
        self.finish(self.parser.parse_bytes(name, bytes, compression))
    }

    /// Parse a caller-authorized file and render it.
    ///
    /// # Errors
    ///
    /// Returns a typed error for invalid options, file/transport/parser
    /// failures, or output overflow.
    pub fn render_file(
        &self,
        name: &SourceName,
        path: impl AsRef<Path>,
        compression: Compression,
    ) -> Result<RenderReport, RenderError> {
        self.validate()?;
        self.finish(self.parser.parse_file(name, path, compression))
    }

    /// Parse a bundle root through the explicit in-memory resolver and render it.
    ///
    /// # Errors
    ///
    /// Returns a typed error for invalid options, a missing bundle root,
    /// resolver/parser failure, or output overflow.
    pub fn render_bundle(
        &self,
        bundle: &mut SourceBundle,
        root: &str,
    ) -> Result<RenderReport, RenderError> {
        self.validate()?;
        self.finish(self.parser.parse_bundle(bundle, root))
    }

    fn validate(&self) -> Result<(), RenderError> {
        if !(MIN_RENDER_WIDTH..=MAX_RENDER_WIDTH).contains(&self.width) {
            return Err(RenderError {
                kind: RenderErrorKind::InvalidOptions,
                message: format!("render width must be in {MIN_RENDER_WIDTH}..={MAX_RENDER_WIDTH}")
                    .into(),
            });
        }
        let limit = self.parser.config().limits.max_render_output_bytes;
        if self.max_output_bytes > MAX_RENDER_OUTPUT_BYTES || self.max_output_bytes > limit {
            return Err(RenderError {
                kind: RenderErrorKind::InvalidOptions,
                message: format!(
                    "render output budget must not exceed {} bytes",
                    MAX_RENDER_OUTPUT_BYTES.min(limit)
                )
                .into(),
            });
        }
        Ok(())
    }

    fn finish(&self, report: Result<ParseReport, FatalError>) -> Result<RenderReport, RenderError> {
        let report = report.map_err(|error| RenderError {
            kind: RenderErrorKind::Parse,
            message: error.to_string().into(),
        })?;
        let output = render_document(
            &report.document,
            self.format,
            self.html_fragment,
            self.width,
            self.max_output_bytes,
            &self.parser.config().limits,
        )?;
        Ok(RenderReport {
            output,
            diagnostics: report.diagnostics,
        })
    }
}

#[allow(clippy::too_many_lines)] // HTML and terminal node dispatch stay adjacent for output-contract review.
fn render_document(
    document: &Document,
    format: RenderFormat,
    fragment: bool,
    width: usize,
    maximum: usize,
    limits: &Limits,
) -> Result<String, RenderError> {
    if format != RenderFormat::Html {
        return render_terminal_document(document, format, width, maximum, limits);
    }
    render_html_document(document, fragment, maximum, limits)
}

/// Render the native HTML device from semantic blocks instead of flattening
/// the arena preorder.  The compatibility AST intentionally represents both
/// man and mdoc as generic nodes; headings, paragraphs, and lists nevertheless
/// need their Head/Body boundaries to produce stable HTML structure.
fn render_html_document(
    document: &Document,
    fragment: bool,
    maximum: usize,
    limits: &Limits,
) -> Result<String, RenderError> {
    let mut output = String::new();
    let mut state = HtmlState::default();
    if !fragment {
        append(
            &mut output,
            "<!doctype html><html><body><main class=\"mantdoc\">",
            maximum,
        )?;
    }
    if let Some(root) = document.node(document.root()) {
        for node in root.children() {
            render_html_node(node, limits, &mut state, &mut output, maximum)?;
        }
    }
    if !fragment {
        append(&mut output, "</main></body></html>", maximum)?;
    }
    Ok(output.trim_end().to_owned())
}

/// Document-scoped HTML state which the arena deliberately does not expose as
/// syntax.  mandoc makes repeated heading destinations unique at render time:
/// the second `DESCRIPTION`, for example, becomes `DESCRIPTION~2`.
#[derive(Default)]
struct HtmlState {
    headings: BTreeMap<String, usize>,
    man_targets: BTreeMap<String, usize>,
    definition_targets: BTreeMap<String, usize>,
    display_targets: BTreeMap<String, usize>,
}

fn render_html_node(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if node.flags().no_print || node.ancestors().any(|ancestor| ancestor.flags().no_print) {
        return Ok(());
    }
    match (node.kind(), node.macro_name()) {
        (NodeKind::Block, Some("SH" | "SS" | "Sh" | "Ss")) => {
            render_html_section(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("PP" | "LP")) => {
            render_html_man_paragraph_block(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("Pp")) => {
            let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
                return Ok(());
            };
            render_html_paragraph(
                body.children().collect::<Vec<_>>(),
                limits,
                None,
                output,
                maximum,
            )
        }
        (NodeKind::Block, Some("TP" | "TQ")) => {
            render_html_man_tagged_paragraph(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("IP")) => {
            render_html_man_indented_paragraph(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("HP")) => {
            render_html_man_hanging_paragraph(node, limits, output, maximum)
        }
        (NodeKind::Block, Some("RS")) => {
            render_html_man_indent(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("SY")) => render_html_man_synopsis(node, limits, output, maximum),
        (NodeKind::Block, Some("Bf")) => {
            render_html_font_block(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("Bd")) => {
            render_html_mdoc_display(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("D1" | "Dl")) => {
            render_html_one_line_display(node, limits, output, maximum)
        }
        (NodeKind::Block, Some("Bl"))
            if node.list_kind() == Some(NormalizedListKind::Definition) =>
        {
            render_html_mdoc_tag_list(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("Bl"))
            if node.list_kind() == Some(NormalizedListKind::Bullet)
                && html_list_direct_target_tag(node).is_some() =>
        {
            render_html_mdoc_marker_list(node, limits, output, maximum)
        }
        (NodeKind::Block, Some("Bl"))
            if node.list_kind() == Some(NormalizedListKind::Column)
                && html_list_direct_target_tag(node).is_some() =>
        {
            render_html_mdoc_column_list(node, limits, output, maximum)
        }
        // mdoc paragraph and vertical controls are structural in HTML.  Their
        // retained arguments exist for diagnostics/AST compatibility, never
        // as prose.  `br` is the one inline exception and is handled by the
        // enclosing Body so it can remain inside the current paragraph.
        (NodeKind::Element, Some("Pp" | "sp" | "br" | "PD" | "ft")) => Ok(()),
        (NodeKind::Text | NodeKind::Equation, _) => {
            render_html_flat_node(node, limits, output, maximum)
        }
        (NodeKind::Table, _) if !node.table_cells().is_empty() => {
            render_html_table(node, limits, output, maximum)
        }
        _ => {
            for child in node.children() {
                render_html_node(child, limits, state, output, maximum)?;
            }
            Ok(())
        }
    }
}

/// Render a contiguous tbl range as one HTML table when private tbl layout
/// metadata is present.  Public `Table` nodes deliberately remain one row at
/// a time for owned-AST compatibility; HTML, like the terminal device, needs
/// their shared layout to recover borders, rule rows, column fonts, and
/// alignment without changing that public contract.
fn render_html_table(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(layout) = node.table_terminal() else {
        return render_html_plain_table(node, output, maximum);
    };
    if terminal_previous_sibling(node)
        .is_some_and(|previous| previous.kind() == NodeKind::Table && !layout.starts_table)
    {
        return Ok(());
    }
    let rows = html_table_range(node);
    let styled = rows
        .iter()
        .filter_map(|row| row.table_terminal())
        .any(|layout| {
            layout.outer_border != TableTerminalBorder::None
                || layout.all_box
                || layout.horizontal_rule != TableTerminalBorder::None
                || layout
                    .cells
                    .iter()
                    .any(|cell| cell.font != TableTerminalFont::Roman)
        });
    if !styled {
        return render_html_plain_table(node, output, maximum);
    }

    let outer = rows
        .iter()
        .filter_map(|row| row.table_terminal().map(|layout| layout.outer_border))
        .find(|border| *border != TableTerminalBorder::None)
        .unwrap_or(TableTerminalBorder::None);
    let all_box = rows
        .iter()
        .filter_map(|row| row.table_terminal())
        .any(|layout| layout.all_box);
    let mut data_rows: Vec<(NodeRef<'_>, Option<TableTerminalBorder>)> = Vec::new();
    for row in rows {
        let layout = row.table_terminal().cloned().unwrap_or_default();
        let rule = (layout.horizontal_rule != TableTerminalBorder::None)
            .then_some(layout.horizontal_rule)
            .or_else(|| {
                (row.table_cells().is_empty()).then(|| {
                    layout
                        .cells
                        .iter()
                        .map(|cell| cell.horizontal_rule)
                        .find(|rule| *rule != TableTerminalBorder::None)
                        .unwrap_or(TableTerminalBorder::None)
                })
            })
            .filter(|rule| *rule != TableTerminalBorder::None);
        if let Some(rule) = rule {
            if let Some((_, divider)) = data_rows.last_mut() {
                *divider = Some(rule);
            }
            continue;
        }
        if !row.table_cells().is_empty() {
            data_rows.push((row, None));
        }
    }
    if data_rows.is_empty() {
        return Ok(());
    }

    append(output, "<table class=\"tbl\"", maximum)?;
    if outer != TableTerminalBorder::None || all_box {
        append(
            output,
            &format!(
                " style=\"border-style: {};\"",
                html_table_border_style(if outer == TableTerminalBorder::None {
                    TableTerminalBorder::Single
                } else {
                    outer
                })
            ),
            maximum,
        )?;
    }
    append(output, ">\n", maximum)?;
    for (row_index, (row, divider)) in data_rows.iter().enumerate() {
        append(output, "  <tr", maximum)?;
        if let Some(divider) = divider.or_else(|| {
            (all_box && row_index + 1 < data_rows.len()).then_some(TableTerminalBorder::Single)
        }) {
            append(
                output,
                &format!(
                    " style=\"border-bottom-style: {};\"",
                    html_table_border_style(divider)
                ),
                maximum,
            )?;
        }
        append(output, ">\n", maximum)?;
        let layout = row.table_terminal();
        let column_count = layout.map_or_else(
            || row.table_cells().len(),
            |layout| layout.cells.len().max(row.table_cells().len()),
        );
        let starts = table_terminal_cell_starts(row, column_count);
        for (index, cell) in row.table_cells().iter().enumerate() {
            append(output, "    <td", maximum)?;
            if cell.column_span > 1 {
                append(
                    output,
                    &format!(" colspan=\"{}\"", cell.column_span),
                    maximum,
                )?;
            }
            if cell.row_span > 1 {
                append(output, &format!(" rowspan=\"{}\"", cell.row_span), maximum)?;
            }
            let alignment = match cell.alignment {
                TableAlignment::Left => None,
                TableAlignment::Center => Some("center"),
                TableAlignment::Right => Some("right"),
            };
            if let Some(alignment) = alignment {
                append(
                    output,
                    &format!(" style=\"text-align: {alignment};\""),
                    maximum,
                )?;
            }
            append(output, ">", maximum)?;
            if let Some(text) = &cell.text {
                let font = starts
                    .get(index)
                    .and_then(|column| layout.and_then(|layout| layout.cells.get(*column)))
                    .map_or(TableTerminalFont::Roman, |cell| cell.font);
                append(
                    output,
                    &render_html_table_cell_text(text, font, limits),
                    maximum,
                )?;
            }
            append(output, "</td>\n", maximum)?;
        }
        append(output, "  </tr>\n", maximum)?;
    }
    append(output, "</table>\n", maximum)
}

fn render_html_plain_table(
    node: NodeRef<'_>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    append(output, "<table class=\"Tbl\"><tr>", maximum)?;
    for cell in node.table_cells() {
        append(output, "<td>", maximum)?;
        if let Some(text) = &cell.text {
            append(output, &escape_html(text), maximum)?;
        }
        append(output, "</td>", maximum)?;
    }
    append(output, "</tr></table>\n", maximum)
}

fn html_table_range(node: NodeRef<'_>) -> Vec<NodeRef<'_>> {
    let Some(parent) = node.parent() else {
        return vec![node];
    };
    parent
        .children()
        .skip_while(|sibling| sibling.id() != node.id())
        .enumerate()
        .take_while(|(index, sibling)| {
            sibling.kind() == NodeKind::Table
                && (*index == 0
                    || !sibling
                        .table_terminal()
                        .is_some_and(|layout| layout.starts_table))
        })
        .map(|(_, row)| row)
        .collect()
}

fn html_table_border_style(border: TableTerminalBorder) -> &'static str {
    match border {
        TableTerminalBorder::Double => "double",
        TableTerminalBorder::None | TableTerminalBorder::Single => "solid",
    }
}

fn render_html_table_cell_text(text: &str, font: TableTerminalFont, limits: &Limits) -> String {
    let font = match font {
        TableTerminalFont::Roman => HtmlFont::Roman,
        TableTerminalFont::Bold => HtmlFont::Bold,
        TableTerminalFont::Italic => HtmlFont::Italic,
    };
    render_html_visible_text_with_font(text, limits, font)
}

/// Render an mdoc display as a structural region.  A `Bd` Body may switch
/// from no-fill back to filled text, nest displays or lists, and carry a
/// `.Tg` destination.  Flattening it through the surrounding section loses
/// all four boundaries, so keep its source-order flow local to the display.
fn render_html_mdoc_display(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let target = html_node_target_tag(node).map(|tag| html_unique_display_target(tag, state));
    let class = match (
        node.literal_display(),
        node.offset().is_some(),
        target.is_some(),
    ) {
        (true, _, _) => "Bd Pp Li",
        (false, true, true) => "Bd Pp\n  Bd-indent",
        (false, true, false) => "Bd Pp Bd-indent",
        (false, false, _) => "Bd Pp",
    };
    append(output, &format!("<div class=\"{class}\""), maximum)?;
    if let Some(target) = &target {
        append(output, &format!(" id=\"{}\"", escape_html(target)), maximum)?;
    }
    append(output, ">", maximum)?;
    if node.literal_display() {
        render_html_mdoc_literal_display_body(body, limits, target, output, maximum)?;
    } else {
        render_html_mdoc_display_body(body, limits, state, target, output, maximum)?;
    }
    append(output, "</div>\n", maximum)
}

/// Literal displays stay in one `pre` element even when an mdoc paragraph
/// marker occurs inside them.  Such a marker contributes an empty HTML target
/// followed by the linked literal phrase; it never opens an HTML paragraph.
fn render_html_mdoc_literal_display_body(
    body: NodeRef<'_>,
    limits: &Limits,
    mut target: Option<String>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut content = String::new();
    let mut previous: Option<NodeRef<'_>> = None;
    for child in body.children() {
        if child.macro_name() == Some("Pp") {
            if let Some(tag) = child.tag().filter(|tag| !tag.is_empty()) {
                if !content.is_empty() && !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str("<mark id=\"");
                content.push_str(&escape_html(tag));
                content.push_str("\"></mark>\n");
                target = Some(tag.to_owned());
                previous = None;
            }
            continue;
        }
        let fragment = render_html_display_fragment(child, limits, &mut target);
        if fragment.is_empty() {
            continue;
        }
        if let Some(previous) = previous {
            if child.flags().line_start {
                content.push('\n');
            } else if !previous.flags().delimiter_open && !child.flags().delimiter_close {
                content.push(' ');
            }
        }
        content.push_str(&fragment);
        previous = Some(child);
    }
    if content.is_empty() {
        return Ok(());
    }
    append(output, "\n<pre>", maximum)?;
    append(output, &content, maximum)?;
    append(output, "</pre>\n", maximum)
}

/// Preserve a filled or `-unfilled` display's local flow.  Paragraph markers
/// select an HTML paragraph only when the display is filled; direct phrases
/// and following nested blocks remain raw display flow.
fn render_html_mdoc_display_body(
    body: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    mut target: Option<String>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut inline: Vec<NodeRef<'_>> = Vec::new();
    let mut paragraph_tag = None;
    let mut first_flow = true;
    for child in body.children() {
        if child.macro_name() != Some("Pp") && html_is_mdoc_display_inline(child) {
            if inline
                .last()
                .is_some_and(|previous| previous.flags().no_fill != child.flags().no_fill)
            {
                render_html_mdoc_display_flow(
                    std::mem::take(&mut inline),
                    limits,
                    paragraph_tag.take(),
                    &mut target,
                    first_flow,
                    true,
                    output,
                    maximum,
                )?;
                first_flow = false;
            }
            inline.push(child);
            continue;
        }
        if child.macro_name() == Some("Pp") {
            render_html_mdoc_display_flow(
                std::mem::take(&mut inline),
                limits,
                paragraph_tag.take(),
                &mut target,
                first_flow,
                true,
                output,
                maximum,
            )?;
            first_flow = false;
            paragraph_tag = child.tag().filter(|tag| !tag.is_empty()).map(str::to_owned);
            continue;
        }
        render_html_mdoc_display_flow(
            std::mem::take(&mut inline),
            limits,
            paragraph_tag.take(),
            &mut target,
            first_flow,
            true,
            output,
            maximum,
        )?;
        first_flow = false;
        render_html_node(child, limits, state, output, maximum)?;
    }
    render_html_mdoc_display_flow(
        inline,
        limits,
        paragraph_tag,
        &mut target,
        first_flow,
        false,
        output,
        maximum,
    )
}

#[allow(clippy::too_many_arguments)] // Flow state mirrors mdoc's distinct device boundaries.
fn render_html_mdoc_display_flow(
    nodes: Vec<NodeRef<'_>>,
    limits: &Limits,
    paragraph_tag: Option<String>,
    target: &mut Option<String>,
    first_flow: bool,
    terminate_line: bool,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if nodes.is_empty() {
        return Ok(());
    }
    if let Some(tag) = paragraph_tag {
        let content = render_html_display_inline_nodes(nodes, limits, target, "    ", false);
        let tag = escape_html(&tag);
        append(output, &format!("<p class=\"Pp\" id=\"{tag}\">"), maximum)?;
        append(output, &content, maximum)?;
        return append(output, "</p>\n", maximum);
    }
    if nodes.iter().any(|node| node.flags().no_fill) {
        if first_flow {
            append(output, "\n", maximum)?;
        }
        let content = render_html_display_inline_nodes(nodes, limits, target, "", true);
        append(output, "<pre>", maximum)?;
        append(output, &content, maximum)?;
        return append(output, "</pre>\n", maximum);
    }
    let content = render_html_display_inline_nodes(nodes, limits, target, "  ", false);
    append(output, &content, maximum)?;
    if terminate_line {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Render one display phrase with mandoc's source-line continuation geometry.
/// The normal inline renderer deliberately collapses that geometry for prose;
/// `Bd` keeps it for raw display flow and makes its leading `.Tg` link local.
fn render_html_display_inline_nodes(
    nodes: Vec<NodeRef<'_>>,
    limits: &Limits,
    target: &mut Option<String>,
    continuation: &str,
    preserve_text_lines: bool,
) -> String {
    let mut output = String::new();
    let mut previous: Option<NodeRef<'_>> = None;
    let mut previous_was_target = false;
    for node in nodes {
        let wraps_target = target.is_some();
        let mut fragment = render_html_display_fragment(node, limits, target);
        if fragment.is_empty() {
            continue;
        }
        if let Some(previous) = previous {
            if (previous_was_target || previous.flags().permalink) && node.flags().line_start {
                if continuation == "    "
                    && let Some(split) = fragment.find(' ')
                {
                    output.push(' ');
                    output.push_str(&fragment[..split]);
                    output.push('\n');
                    output.push_str(continuation);
                    fragment.replace_range(..=split, "");
                } else {
                    output.push('\n');
                    output.push_str(continuation);
                }
            } else if (preserve_text_lines || node.kind() != NodeKind::Text)
                && node.flags().line_start
            {
                output.push('\n');
                output.push_str(continuation);
            } else if !previous.flags().delimiter_open && !node.flags().delimiter_close {
                output.push(' ');
            }
        }
        output.push_str(&fragment);
        previous = Some(node);
        previous_was_target = wraps_target;
    }
    output
}

fn html_is_mdoc_display_inline(node: NodeRef<'_>) -> bool {
    matches!(node.macro_name(), Some("Pq"))
        || matches!(
            node.kind(),
            NodeKind::Text | NodeKind::Equation | NodeKind::Element
        ) && !matches!(node.macro_name(), Some("sp"))
}

fn render_html_display_fragment(
    node: NodeRef<'_>,
    limits: &Limits,
    target: &mut Option<String>,
) -> String {
    let mut content = render_html_inline_nodes(vec![node], limits);
    if content.is_empty() {
        return content;
    }
    if let Some(tag) = target.take() {
        if content.contains("class=\"permalink\"") {
            content = html_retarget_permalink(content, &tag);
        } else {
            let tag = escape_html(&tag);
            content = format!("<a class=\"permalink\" href=\"#{tag}\">{content}</a>");
        }
    }
    if node
        .parent()
        .is_some_and(|parent| parent.flags().deep_link_target)
        && !node.flags().permalink
    {
        content = content.replace(" (", "\n  (");
    }
    content
}

/// Render an mdoc font block without leaking its configuration Head into the
/// DOM.  The normalized font belongs to the whole Body, including explicit
/// paragraphs nested below it, so the wrapper must outlive those child blocks.
fn render_html_font_block(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let class = match node.font() {
        Some(NormalizedFont::Emphasis) => "Bf Em",
        Some(NormalizedFont::Literal) => "Bf Li",
        Some(NormalizedFont::Symbolic) => "Bf Sy",
        None => "Bf",
    };
    append(output, &format!("<div class=\"{class}\">"), maximum)?;

    let mut inline: Vec<NodeRef<'_>> = Vec::new();
    let mut next_inline_is_paragraph = false;
    for child in body.children() {
        if matches!(child.kind(), NodeKind::Text | NodeKind::Equation) {
            inline.push(child);
            continue;
        }
        if !inline.is_empty() {
            let inline = std::mem::take(&mut inline);
            if next_inline_is_paragraph {
                render_html_paragraph(inline, limits, None, output, maximum)?;
            } else {
                append(output, &render_html_inline_nodes(inline, limits), maximum)?;
                append(output, "\n", maximum)?;
            }
            next_inline_is_paragraph = false;
        }
        if matches!(child.macro_name(), Some("PP" | "LP" | "Pp")) {
            if let Some(paragraph) = child.children().find(|node| node.kind() == NodeKind::Body) {
                render_html_paragraph(
                    paragraph.children().collect::<Vec<_>>(),
                    limits,
                    None,
                    output,
                    maximum,
                )?;
            } else {
                next_inline_is_paragraph = true;
            }
        } else {
            render_html_node(child, limits, state, output, maximum)?;
        }
    }
    if !inline.is_empty() {
        if next_inline_is_paragraph {
            render_html_paragraph(inline, limits, None, output, maximum)?;
        } else {
            append(output, &render_html_inline_nodes(inline, limits), maximum)?;
            append(output, "\n", maximum)?;
        }
    }
    append(output, "</div>\n", maximum)
}

/// Render mdoc's `D1` and `Dl` as their one-line display DOM forms.  The
/// parser retains the first doubled argument separator on the following
/// phrase node, which is exactly the point where the HTML device breaks and
/// indents the display continuation.
fn render_html_one_line_display(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let children = body.children().collect::<Vec<_>>();
    let mut content = String::new();
    let literal = node.macro_name() == Some("Dl");
    for child in children {
        let mut fragment = render_html_inline_nodes(vec![child], limits);
        if !literal
            && !child.flags().permalink
            && child.separator_width() > 1
            && let Some(index) = fragment.find(' ')
        {
            fragment.replace_range(index..=index, "\n  ");
        }
        if !fragment.is_empty() {
            if !content.is_empty() {
                if literal {
                    content.push_str("\n  ");
                } else {
                    content.push(' ');
                }
            }
            content.push_str(&fragment);
        }
    }
    let class = if content.is_empty() {
        "Bd Bd-indent"
    } else {
        "Bd\n  Bd-indent"
    };
    append(output, &format!("<div class=\"{class}\""), maximum)?;
    if let Some(tag) = body.tag().filter(|tag| !tag.is_empty()) {
        append(output, &format!(" id=\"{}\"", escape_html(tag)), maximum)?;
    }
    append(output, ">", maximum)?;
    if literal {
        append(output, "<code class=\"Li\">", maximum)?;
        append(output, &content, maximum)?;
        append(output, "</code>", maximum)?;
    } else {
        append(output, &content, maximum)?;
    }
    append(output, "</div>\n", maximum)
}

/// Render a man paragraph's Body in source-order so `.nf` and `.fi` retain
/// their HTML preformatted boundaries.  The public AST stores both controls
/// among the Body children rather than turning them into independent blocks.
fn render_html_man_paragraph_block(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let mut inline: Vec<NodeRef<'_>> = Vec::new();
    let mut raw_after_literal = false;
    for child in body.children() {
        if matches!(child.kind(), NodeKind::Text | NodeKind::Equation)
            || child.macro_name() == Some("br")
            || child.macro_name() == Some("sp") && child.flags().no_fill
        {
            if inline
                .last()
                .is_some_and(|previous| previous.flags().no_fill != child.flags().no_fill)
            {
                let was_literal = inline.iter().any(|node| node.flags().no_fill);
                render_html_inline_flow(
                    std::mem::take(&mut inline),
                    limits,
                    None,
                    raw_after_literal,
                    output,
                    maximum,
                )?;
                raw_after_literal = was_literal;
            }
            inline.push(child);
            continue;
        }
        if matches!(child.macro_name(), Some("nf" | "fi")) {
            let was_literal = inline.iter().any(|node| node.flags().no_fill);
            render_html_inline_flow(
                std::mem::take(&mut inline),
                limits,
                None,
                raw_after_literal,
                output,
                maximum,
            )?;
            raw_after_literal = raw_after_literal || was_literal;
            continue;
        }
        render_html_inline_flow(
            std::mem::take(&mut inline),
            limits,
            None,
            raw_after_literal,
            output,
            maximum,
        )?;
        raw_after_literal = false;
        render_html_node(child, limits, state, output, maximum)?;
    }
    render_html_inline_flow(inline, limits, None, raw_after_literal, output, maximum)
}

/// Render man(7)'s tagged paragraphs through their Head/Body ownership.  A
/// `TP` Head retains its same-line width request in the compatible tree, so
/// only its following physical-line terms are visible in the HTML `dt`.
fn render_html_man_tagged_paragraph(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(fields) = html_man_tagged_paragraph_group(node) else {
        return Ok(());
    };
    append(output, "<dl class=\"Bl-tag\">\n", maximum)?;
    for field in fields {
        render_html_man_tagged_item(field, limits, state, output, maximum)?;
    }
    append(output, "</dl>\n", maximum)
}

fn html_man_tagged_paragraph_group(node: NodeRef<'_>) -> Option<Vec<NodeRef<'_>>> {
    let parent = node.parent()?;
    let siblings = parent.children().collect::<Vec<_>>();
    let index = siblings
        .iter()
        .position(|sibling| sibling.id() == node.id())?;
    if index > 0 && matches!(siblings[index - 1].macro_name(), Some("TP" | "TQ")) {
        return None;
    }
    Some(
        siblings[index..]
            .iter()
            .copied()
            .take_while(|sibling| matches!(sibling.macro_name(), Some("TP" | "TQ")))
            .collect(),
    )
}

fn render_html_man_tagged_item(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(head) = node.children().find(|child| child.kind() == NodeKind::Head) else {
        return Ok(());
    };
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let term_nodes = head
        .children()
        .filter(|child| child.flags().line_start)
        .collect::<Vec<_>>();
    let mut term = render_html_inline_nodes(term_nodes, limits);
    let tag = html_unique_man_target(head, state);
    if let Some(tag) = &tag
        && !term.contains("class=\"permalink\"")
    {
        let escaped = escape_html(tag);
        term = format!("<a class=\"permalink\" href=\"#{escaped}\">{term}</a>");
    }
    append(output, "  <dt", maximum)?;
    if let Some(tag) = tag {
        append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
    }
    append(output, ">", maximum)?;
    append(output, &term, maximum)?;
    append(output, "</dt>\n", maximum)?;
    render_html_man_definition_body(body, limits, output, maximum)
}

/// A man field Body can transition between regular and no-fill text.  Each
/// transition is a distinct HTML preformatted boundary; rendering the entire
/// `dd` as one `pre` loses later ordinary phrase flow, and doing the opposite
/// loses the literal source lines.
fn render_html_man_definition_body(
    body: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut segments: Vec<(bool, Vec<NodeRef<'_>>)> = Vec::new();
    for child in body.children() {
        let no_fill = child.flags().no_fill;
        if segments
            .last()
            .is_some_and(|(previous, _)| *previous == no_fill)
        {
            segments.last_mut().expect("checked segment").1.push(child);
        } else {
            segments.push((no_fill, vec![child]));
        }
    }
    if segments.is_empty() {
        return append(output, "  <dd></dd>\n", maximum);
    }
    append(output, "  <dd>", maximum)?;
    for (index, (no_fill, nodes)) in segments.iter().enumerate() {
        if *no_fill {
            append(output, "\n    <pre>", maximum)?;
            append(
                output,
                &render_html_inline_nodes(nodes.clone(), limits),
                maximum,
            )?;
            append(output, "</pre>", maximum)?;
        } else {
            if index > 0 {
                append(output, "\n    ", maximum)?;
            }
            append(
                output,
                &render_html_inline_nodes(nodes.clone(), limits),
                maximum,
            )?;
        }
    }
    if segments.last().is_some_and(|(no_fill, _)| *no_fill) {
        append(output, "\n  ", maximum)?;
    }
    append(output, "</dd>\n", maximum)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum HtmlManIpKind {
    Tag,
    AsteriskBullet,
    DotBullet,
    Dash,
}

/// Render adjacent man `IP` fields in their shared DOM container.  mandoc
/// starts a new list when the authored marker changes, even when two markers
/// share the bullet class, so the marker spelling remains part of this small
/// renderer-only grouping key.
fn render_html_man_indented_paragraph(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some((kind, fields)) = html_man_ip_group(node, limits) else {
        return Ok(());
    };
    match kind {
        HtmlManIpKind::Tag => {
            append(output, "<dl class=\"Bl-tag\">\n", maximum)?;
            for field in fields {
                render_html_man_ip_tag_item(field, limits, state, output, maximum)?;
            }
            append(output, "</dl>\n", maximum)
        }
        HtmlManIpKind::AsteriskBullet | HtmlManIpKind::DotBullet | HtmlManIpKind::Dash => {
            let class = if kind == HtmlManIpKind::Dash {
                "Bl-dash"
            } else {
                "Bl-bullet"
            };
            append(output, &format!("<ul class=\"{class}\">\n"), maximum)?;
            for field in fields {
                let body = field
                    .children()
                    .find(|child| child.kind() == NodeKind::Body);
                let content = body.map_or_else(String::new, |body| {
                    render_html_inline_nodes(body.children().collect::<Vec<_>>(), limits)
                });
                append(output, "  <li>", maximum)?;
                append(output, &content, maximum)?;
                append(output, "</li>\n", maximum)?;
            }
            append(output, "</ul>\n", maximum)
        }
    }
}

fn html_man_ip_group<'document>(
    node: NodeRef<'document>,
    limits: &Limits,
) -> Option<(HtmlManIpKind, Vec<NodeRef<'document>>)> {
    let kind = html_man_ip_kind(node, limits)?;
    let parent = node.parent()?;
    let siblings = parent.children().collect::<Vec<_>>();
    let index = siblings
        .iter()
        .position(|sibling| sibling.id() == node.id())?;
    if index > 0
        && siblings[index - 1].macro_name() == Some("IP")
        && html_man_ip_kind(siblings[index - 1], limits) == Some(kind)
    {
        return None;
    }
    Some((
        kind,
        siblings[index..]
            .iter()
            .copied()
            .take_while(|sibling| {
                sibling.macro_name() == Some("IP")
                    && html_man_ip_kind(*sibling, limits) == Some(kind)
            })
            .collect(),
    ))
}

fn html_man_ip_kind(node: NodeRef<'_>, limits: &Limits) -> Option<HtmlManIpKind> {
    let head = node
        .children()
        .find(|child| child.kind() == NodeKind::Head)?;
    let Some(marker) = head.children().next() else {
        return Some(HtmlManIpKind::Tag);
    };
    match render_html_inline_nodes(vec![marker], limits).as_str() {
        "*" => Some(HtmlManIpKind::AsteriskBullet),
        "&#x2022;" => Some(HtmlManIpKind::DotBullet),
        "-" => Some(HtmlManIpKind::Dash),
        _ => Some(HtmlManIpKind::Tag),
    }
}

fn render_html_man_ip_tag_item(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(head) = node.children().find(|child| child.kind() == NodeKind::Head) else {
        return Ok(());
    };
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let term = head.children().next().map_or_else(String::new, |marker| {
        render_html_inline_nodes(vec![marker], limits)
    });
    let tag = html_unique_man_target(head, state);
    let term = if let Some(tag) = &tag {
        if term.contains("class=\"permalink\"") {
            term
        } else {
            let escaped = escape_html(tag);
            format!("<a class=\"permalink\" href=\"#{escaped}\">{term}</a>")
        }
    } else {
        term
    };
    append(output, "  <dt", maximum)?;
    if let Some(tag) = tag {
        append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
    }
    append(output, ">", maximum)?;
    append(output, &term, maximum)?;
    append(output, "</dt>\n", maximum)?;
    render_html_man_definition_body(body, limits, output, maximum)
}

/// Render man(7)'s `HP` as an owned paragraph rather than leaking its width
/// Head.  A no-fill Body is the device's literal block and therefore has no
/// Pp wrapper.
fn render_html_man_hanging_paragraph(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let body_nodes = body.children().collect::<Vec<_>>();
    if body_nodes.iter().any(|child| child.flags().no_fill) {
        return render_html_preformatted(body_nodes, limits, output, maximum);
    }
    let content = render_html_inline_nodes(body_nodes, limits);
    if content.is_empty() {
        return Ok(());
    }
    append(output, "<p class=\"Pp HP\">", maximum)?;
    append(output, &content, maximum)?;
    append(output, "</p>\n", maximum)
}

/// Render a man `RS` Body as a structural indented region.  Its initial raw
/// phrase is not a paragraph, while nested PP/LP blocks retain their Pp DOM
/// ownership inside the indent.
fn render_html_man_indent(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    append(output, "<div class=\"Bd-indent\">", maximum)?;
    let mut inline = Vec::new();
    for child in body.children() {
        if matches!(child.kind(), NodeKind::Text | NodeKind::Equation)
            || child.macro_name() == Some("br")
        {
            inline.push(child);
            continue;
        }
        render_html_man_indent_inline(std::mem::take(&mut inline), limits, output, maximum)?;
        if output.ends_with("<div class=\"Bd-indent\">") {
            append(output, "\n", maximum)?;
        }
        render_html_node(child, limits, state, output, maximum)?;
    }
    render_html_man_indent_inline(inline, limits, output, maximum)?;
    append(output, "</div>\n", maximum)
}

fn render_html_man_indent_inline(
    nodes: Vec<NodeRef<'_>>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if nodes.is_empty() {
        return Ok(());
    }
    if nodes.iter().any(|node| node.flags().no_fill) {
        append(output, "\n", maximum)?;
        return render_html_preformatted(nodes, limits, output, maximum);
    }
    let content = render_html_inline_nodes(nodes, limits);
    if content.is_empty() {
        return Ok(());
    }
    append(output, &content, maximum)?;
    append(output, "\n", maximum)
}

/// Render a man synopsis as the two-column semantic device table.  A no-fill
/// argument lives in an inner preformatted field, preserving `SY`'s distinct
/// continuation geometry.
fn render_html_man_synopsis(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(head) = node.children().find(|child| child.kind() == NodeKind::Head) else {
        return Ok(());
    };
    let body = node.children().find(|child| child.kind() == NodeKind::Body);
    let command = render_html_inline_nodes(head.children().collect::<Vec<_>>(), limits);
    append(
        output,
        "<table class=\"Nm\">\n  <tr>\n    <td><code class=\"Nm\">",
        maximum,
    )?;
    append(output, &command, maximum)?;
    append(output, "</code></td>\n", maximum)?;
    if let Some(body) = body {
        let body_nodes = body.children().collect::<Vec<_>>();
        if body_nodes.iter().any(|child| child.flags().no_fill) {
            append(output, "    <td>\n    ", maximum)?;
            render_html_preformatted(body_nodes, limits, output, maximum)?;
            append(output, "    </td>\n", maximum)?;
        } else {
            append(output, "    <td>", maximum)?;
            append(
                output,
                &render_html_inline_nodes(body_nodes, limits),
                maximum,
            )?;
            append(output, "</td>\n", maximum)?;
        }
    }
    append(output, "  </tr>\n</table>\n", maximum)
}

fn render_html_mdoc_marker_list(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let class = match node.list_marker() {
        Some(MdocListMarker::Dash) => "Bl-dash",
        Some(MdocListMarker::Hyphen) => "Bl-hyphen",
        Some(MdocListMarker::Enum) => "Bl-enum",
        _ => "Bl-bullet",
    };
    append(output, &format!("<ul class=\"{class}\""), maximum)?;
    if let Some(tag) = html_list_direct_target_tag(node) {
        append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
    }
    append(output, ">\n", maximum)?;
    for item in body
        .children()
        .filter(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
    {
        let item_body = item.children().find(|child| child.kind() == NodeKind::Body);
        let mut content = item_body.map_or_else(String::new, |body| {
            render_html_inline_nodes(body.children().collect::<Vec<_>>(), limits)
        });
        let tag = html_node_target_tag(item);
        if let Some(tag) = &tag
            && !content.contains("class=\"permalink\"")
        {
            let escaped = escape_html(tag);
            content = format!("<a class=\"permalink\" href=\"#{escaped}\">{content}</a>");
        }
        append(output, "  <li", maximum)?;
        if let Some(tag) = tag {
            append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
        }
        append(output, ">", maximum)?;
        append(output, &content, maximum)?;
        append(output, "</li>\n", maximum)?;
    }
    append(output, "</ul>\n", maximum)
}

fn render_html_mdoc_column_list(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    append(output, "<table class=\"Bl-column\"", maximum)?;
    if let Some(tag) = html_list_direct_target_tag(node) {
        append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
    }
    append(output, ">\n", maximum)?;
    for item in body
        .children()
        .filter(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
    {
        append(output, "  <tr", maximum)?;
        if let Some(tag) = html_node_target_tag(item) {
            append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
        }
        append(output, ">\n", maximum)?;
        for cell in item
            .children()
            .filter(|child| child.kind() == NodeKind::Body)
        {
            append(output, "    <td>", maximum)?;
            append(
                output,
                &render_html_inline_nodes(cell.children().collect::<Vec<_>>(), limits),
                maximum,
            )?;
            append(output, "</td>\n", maximum)?;
        }
        append(output, "  </tr>\n", maximum)?;
    }
    append(output, "</table>\n", maximum)
}

fn html_node_target_tag(node: NodeRef<'_>) -> Option<String> {
    if (node.flags().deep_link_target || node.flags().permalink)
        && let Some(tag) = node.tag().filter(|tag| !tag.is_empty())
    {
        return Some(tag.to_owned());
    }
    let mut pending = node.children().collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        if node.flags().deep_link_target || node.flags().permalink {
            if let Some(tag) = node.tag().filter(|tag| !tag.is_empty()) {
                return Some(tag.to_owned());
            }
            if let Some(text) = html_first_visible_text(node) {
                let text = text.strip_prefix('-').unwrap_or(text);
                let end = text.find(char::is_whitespace).unwrap_or(text.len());
                if end > 0 {
                    return Some(text[..end].to_owned());
                }
            }
        }
        pending.extend(node.children());
    }
    None
}

fn html_list_direct_target_tag(node: NodeRef<'_>) -> Option<String> {
    [
        Some(node),
        node.children().find(|child| child.kind() == NodeKind::Body),
    ]
    .into_iter()
    .flatten()
    .find_map(|node| {
        (node.flags().deep_link_target || node.flags().permalink)
            .then(|| node.tag().filter(|tag| !tag.is_empty()).map(str::to_owned))
            .flatten()
    })
}

/// Render the common mdoc `Bl -tag` shape.  The terminal-only selectors
/// (`-hang`, `-diag`, and friends) intentionally keep their distinct terminal
/// path; this DOM form is for the normalized definition-list contract.
fn render_html_mdoc_tag_list(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    append(output, "<dl class=\"Bl-tag\"", maximum)?;
    if let Some(tag) = html_list_direct_target_tag(node) {
        append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
    }
    append(output, ">\n", maximum)?;
    for item in body
        .children()
        .filter(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
    {
        let head = item.children().find(|child| child.kind() == NodeKind::Head);
        let item_body = item.children().find(|child| child.kind() == NodeKind::Body);
        let mut head_content = if let Some(head) = head {
            let content = render_html_inline_nodes(head.children().collect::<Vec<_>>(), limits);
            let first_macro = head.children().find_map(NodeRef::macro_name);
            if matches!(first_macro, Some("Fl" | "Em" | "Sy")) {
                content
            } else {
                content.replace(" |\n    ", "\n    |\n    ")
            }
        } else {
            String::new()
        };
        let tag = head
            .filter(|head| head.flags().deep_link_target)
            .and_then(|head| html_unique_definition_target(head, state));
        if let Some(tag) = &tag {
            head_content = html_retarget_permalink(head_content, tag);
        }
        append(output, "  <dt", maximum)?;
        if let Some(tag) = tag {
            append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
        }
        append(output, ">", maximum)?;
        append(output, &head_content, maximum)?;
        append(output, "</dt>\n", maximum)?;
        if let Some(item_body) = item_body {
            append(output, "  <dd>", maximum)?;
            if item_body
                .children()
                .any(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("Bd"))
            {
                render_html_mdoc_definition_body(item_body, limits, state, output, maximum)?;
            } else {
                let body_content =
                    render_html_inline_nodes(item_body.children().collect::<Vec<_>>(), limits);
                append(output, &body_content, maximum)?;
            }
            append(output, "</dd>\n", maximum)?;
        }
    }
    append(output, "</dl>\n", maximum)
}

/// A definition item's body may contain a nested display.  Keep direct prose
/// inside its `dd`, but indent the embedded block exactly as mandoc's HTML
/// device does instead of flattening the display into the definition text.
fn render_html_mdoc_definition_body(
    body: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut inline: Vec<NodeRef<'_>> = Vec::new();
    let mut emitted_block = false;
    for child in body.children() {
        if child.kind() != NodeKind::Block || child.macro_name() != Some("Bd") {
            inline.push(child);
            continue;
        }
        if !inline.is_empty() {
            if emitted_block {
                append(output, "\n    ", maximum)?;
            }
            append(
                output,
                &render_html_inline_nodes(std::mem::take(&mut inline), limits),
                maximum,
            )?;
        }
        append(output, "\n", maximum)?;
        let mut rendered = String::new();
        render_html_mdoc_display(child, limits, state, &mut rendered, maximum)?;
        let rendered = rendered.trim_end();
        for (index, line) in rendered.lines().enumerate() {
            if index > 0 {
                append(output, "\n", maximum)?;
            }
            append(output, "    ", maximum)?;
            append(output, line, maximum)?;
        }
        emitted_block = true;
    }
    if !inline.is_empty() {
        if emitted_block {
            append(output, "\n    ", maximum)?;
        }
        append(output, &render_html_inline_nodes(inline, limits), maximum)?;
    }
    Ok(())
}

fn html_retarget_permalink(mut content: String, tag: &str) -> String {
    if !content.contains("class=\"permalink\"") {
        let escaped = escape_html(tag);
        return format!("<a class=\"permalink\" href=\"#{escaped}\">{content}</a>");
    }
    let Some(prefix) = content.find("href=\"#") else {
        return content;
    };
    let start = prefix + "href=\"#".len();
    let Some(relative_end) = content[start..].find('"') else {
        return content;
    };
    content.replace_range(start..start + relative_end, &escape_html(tag));
    content
}

fn html_definition_head_tag(head: NodeRef<'_>) -> Option<String> {
    head.tag()
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            (head.flags().deep_link_target || head.flags().permalink)
                .then(|| html_first_visible_text_in_source_order(head))
                .flatten()
                .and_then(html_automatic_target)
        })
        .or_else(|| {
            let mut pending = head.children().collect::<Vec<_>>();
            while let Some(node) = pending.pop() {
                if node.flags().deep_link_target || node.flags().permalink {
                    if let Some(tag) = node.tag().filter(|tag| !tag.is_empty()) {
                        return Some(tag.to_owned());
                    }
                    if let Some(text) = html_first_visible_text(node) {
                        let text = text.strip_prefix('-').unwrap_or(text);
                        let end = text.find(char::is_whitespace).unwrap_or(text.len());
                        if end > 0 {
                            return Some(text[..end].to_owned());
                        }
                    }
                }
                pending.extend(node.children());
            }
            None
        })
}

fn html_first_visible_text_in_source_order(node: NodeRef<'_>) -> Option<&str> {
    if node.flags().no_print {
        return None;
    }
    if let Some(text) = node.text().filter(|text| !text.is_empty()) {
        return Some(text);
    }
    node.children()
        .find_map(html_first_visible_text_in_source_order)
}

fn html_unique_man_target(head: NodeRef<'_>, state: &mut HtmlState) -> Option<String> {
    let target = html_definition_head_tag(head)?;
    if target.contains('~') {
        return Some(target);
    }
    let count = state.man_targets.entry(target.clone()).or_insert(0);
    *count += 1;
    (*count > 1)
        .then(|| format!("{target}~{count}"))
        .or(Some(target))
}

fn html_unique_definition_target(head: NodeRef<'_>, state: &mut HtmlState) -> Option<String> {
    let target = html_definition_head_tag(head)?;
    if target.contains('~') {
        return Some(target);
    }
    let count = state.definition_targets.entry(target.clone()).or_insert(0);
    *count += 1;
    (*count > 1)
        .then(|| format!("{target}~{count}"))
        .or(Some(target))
}

fn html_unique_display_target(target: String, state: &mut HtmlState) -> String {
    if target.contains('~') {
        return target;
    }
    let count = state.display_targets.entry(target.clone()).or_insert(0);
    *count += 1;
    if *count > 1 {
        format!("{target}~{count}")
    } else {
        target
    }
}

fn html_automatic_target(text: &str) -> Option<String> {
    let text = text.strip_prefix('-').unwrap_or(text);
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    (end > 0).then(|| text[..end].to_owned())
}

fn html_first_visible_text(node: NodeRef<'_>) -> Option<&str> {
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if !node.flags().no_print {
            if let Some(text) = node.text().filter(|text| !text.is_empty()) {
                return Some(text);
            }
            pending.extend(node.children());
        }
    }
    None
}

/// Retain the historical fragment behavior for raw roff text outside a
/// semantic Body. Structured section paths use paragraph ownership instead.
fn render_html_flat_node(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if node.kind() == NodeKind::Text {
        if node.flags().line_start
            && !output.is_empty()
            && !output.ends_with('>')
            && !output.ends_with('\n')
        {
            if output.ends_with(' ') {
                let _ = output.pop();
            }
            append(output, "\n", maximum)?;
        }
        if let Some(text) = node.text() {
            append(
                output,
                &render_html_visible_text_with_font(
                    text,
                    limits,
                    html_request_font_before(node).current,
                ),
                maximum,
            )?;
            append(output, " ", maximum)?;
        }
        return Ok(());
    }
    if let Some(value) = node.equation() {
        let mathml = node.equation_terminal().map_or_else(
            || escape_html(value),
            |equation| render_html_equation(equation, limits),
        );
        append(output, "<math class=\"eqn\">", maximum)?;
        append(output, &mathml, maximum)?;
        append(output, "</math>", maximum)?;
    }
    Ok(())
}

fn render_html_section(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let subsection = matches!(node.macro_name(), Some("SS" | "Ss"));
    let class = if subsection { "Ss" } else { "Sh" };
    let level = if subsection { "h2" } else { "h1" };
    let head = node.children().find(|child| child.kind() == NodeKind::Head);
    let body = node.children().find(|child| child.kind() == NodeKind::Body);
    append(output, &format!("<section class=\"{class}\">\n"), maximum)?;
    if let Some(head) = head {
        let title = render_html_inline_nodes(head.children().collect::<Vec<_>>(), limits);
        if !title.is_empty() {
            let tag = head
                .tag()
                .filter(|tag| !tag.is_empty())
                .map_or_else(|| html_heading_identifier(&title), str::to_owned);
            let empty_heading = title == "&#x00A0;";
            let title = if title.starts_with(char::is_whitespace) || subsection {
                title.replacen(' ', "\n  ", 1)
            } else {
                title
            };
            if empty_heading {
                append(
                    output,
                    &format!("<{level} class=\"{class}\">{title}</{level}>\n"),
                    maximum,
                )?;
            } else {
                let tag = state.unique_heading_tag(tag);
                let escaped_tag = escape_html(&tag);
                let opening = format!(
                    "<{level} class=\"{class}\" id=\"{escaped_tag}\"><a class=\"permalink\" href=\"#{escaped_tag}\">"
                );
                // `Rs` switches SEE ALSO into standalone citation flow.  The
                // upstream HTML writer consequently folds this particular
                // heading in its device field; ordinary headings retain the
                // source-compatible layout path below.
                let title = if title == "SEE ALSO"
                    && body.is_some_and(|body| {
                        body.children().any(|child| {
                            child.kind() == NodeKind::Block && child.macro_name() == Some("Rs")
                        })
                    }) {
                    wrap_html_heading(&title, opening.len())
                } else {
                    title
                };
                append(
                    output,
                    &format!("{opening}{title}</a></{level}>\n"),
                    maximum,
                )?;
            }
        }
    }
    if let Some(body) = body {
        render_html_body(body, limits, state, output, maximum)?;
    }
    append(output, "</section>\n", maximum)
}

impl HtmlState {
    fn unique_heading_tag(&mut self, tag: String) -> String {
        let count = self.headings.entry(tag.clone()).or_default();
        *count += 1;
        if *count == 1 {
            tag
        } else {
            format!("{tag}~{count}")
        }
    }
}

fn html_heading_identifier(title: &str) -> String {
    title
        .chars()
        .map(|character| {
            if character.is_whitespace() {
                '_'
            } else {
                character
            }
        })
        .collect()
}

fn render_html_body(
    body: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut inline: Vec<NodeRef<'_>> = Vec::new();
    let mut paragraph_tag = None;
    let mut direct_semantic_count = 0_usize;
    // `D1` and `Dl` terminate a paragraph, but their immediately following
    // ordinary phrase is device-level flow rather than a fresh HTML Pp.
    // Keep that narrow exception until a later structural request consumes it.
    let mut raw_after_one_line_display = false;
    for child in body.children() {
        if matches!(child.kind(), NodeKind::Text | NodeKind::Equation) {
            if inline
                .last()
                .is_some_and(|previous| previous.flags().no_fill != child.flags().no_fill)
            {
                render_html_inline_flow(
                    std::mem::take(&mut inline),
                    limits,
                    paragraph_tag.take(),
                    raw_after_one_line_display,
                    output,
                    maximum,
                )?;
                raw_after_one_line_display = false;
            }
            inline.push(child);
            direct_semantic_count = 0;
            continue;
        }
        if child.macro_name() == Some("br") {
            inline.push(child);
            continue;
        }
        if child.macro_name() == Some("sp") && child.flags().no_fill {
            inline.push(child);
            continue;
        }
        if child.macro_name() == Some("Tg") {
            if child.flags().deep_link_target {
                inline.push(child);
            }
            continue;
        }
        if child.macro_name() == Some("ft") {
            inline.push(child);
            continue;
        }
        if child.kind() == NodeKind::Block && child.macro_name() == Some("Rs") {
            // Reference blocks are normally an inline bibliography phrase.
            // SEE ALSO is the one mdoc section where the HTML device closes
            // the preceding paragraph and gives the citation its own Pp.
            if terminal_mdoc_section_named(body, "SEE ALSO") {
                render_html_inline_flow(
                    std::mem::take(&mut inline),
                    limits,
                    paragraph_tag.take(),
                    raw_after_one_line_display,
                    output,
                    maximum,
                )?;
                raw_after_one_line_display = false;
                render_html_reference_paragraph(child, limits, output, maximum)?;
            } else {
                inline.push(child);
            }
            direct_semantic_count = 0;
            continue;
        }
        if child.macro_name() == Some("Fo") && !inline.is_empty() {
            inline.push(child);
            direct_semantic_count = 0;
            continue;
        }
        if child.macro_name() == Some("Fn") {
            inline.push(child);
            direct_semantic_count = 0;
            continue;
        }
        if html_is_semantic_inline_macro(child) && !inline.is_empty() {
            inline.push(child);
            direct_semantic_count = 0;
            continue;
        }
        // `YS` only closes the already-rendered synopsis block.  It must not
        // consume the raw outer-flow boundary that the block owns.
        if child.macro_name() == Some("YS") {
            continue;
        }
        let standalone_semantic = inline.is_empty()
            && child.kind() == NodeKind::Element
            && html_is_semantic_inline_macro(child);
        render_html_inline_flow(
            std::mem::take(&mut inline),
            limits,
            paragraph_tag.take(),
            raw_after_one_line_display,
            output,
            maximum,
        )?;
        raw_after_one_line_display = false;
        if child.macro_name() == Some("Pp") {
            paragraph_tag = child
                .flags()
                .deep_link_target
                .then(|| child.tag().map(str::to_owned))
                .flatten();
            direct_semantic_count = 0;
            continue;
        }
        if child.macro_name() == Some("sp") {
            direct_semantic_count = 0;
            continue;
        }
        if standalone_semantic {
            if direct_semantic_count > 0 {
                append(output, "  ", maximum)?;
            }
            append(
                output,
                &render_html_inline_nodes(vec![child], limits),
                maximum,
            )?;
            append(output, "\n", maximum)?;
            direct_semantic_count += 1;
            continue;
        }
        direct_semantic_count = 0;
        render_html_node(child, limits, state, output, maximum)?;
        raw_after_one_line_display =
            matches!(child.macro_name(), Some("Bd" | "D1" | "Dl" | "RS" | "SY"));
    }
    render_html_inline_flow(
        inline,
        limits,
        paragraph_tag,
        raw_after_one_line_display,
        output,
        maximum,
    )
}

fn render_html_inline_flow(
    nodes: Vec<NodeRef<'_>>,
    limits: &Limits,
    tag: Option<String>,
    raw: bool,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if nodes.iter().any(|node| node.flags().no_fill) {
        return render_html_preformatted(nodes, limits, output, maximum);
    }
    if !raw {
        return render_html_paragraph(nodes, limits, tag, output, maximum);
    }
    let content = render_html_inline_nodes(nodes, limits)
        // `br` is indented when it belongs to a Pp.  This narrow raw-flow
        // path has no paragraph envelope, so keep the device line flush.
        .replace("\n  <br/>\n  ", "\n<br/>\n");
    if content.is_empty() {
        return Ok(());
    }
    append(output, &content, maximum)?;
    append(output, "\n", maximum)
}

fn render_html_preformatted(
    nodes: Vec<NodeRef<'_>>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let content = render_html_inline_nodes(nodes, limits);
    if content.is_empty() {
        return Ok(());
    }
    append(output, "<pre>", maximum)?;
    append(output, &content, maximum)?;
    append(output, "</pre>\n", maximum)
}

fn render_html_paragraph(
    nodes: Vec<NodeRef<'_>>,
    limits: &Limits,
    tag: Option<String>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let has_font_request = nodes
        .iter()
        .any(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("ft"));
    let mut content = render_html_inline_nodes(nodes, limits);
    if content.is_empty() {
        return Ok(());
    }
    if content.starts_with("<a class=\"permalink\"") && content.contains("<code class=\"Fn\"") {
        content = content.replacen("</a>() and", "</a>()\n    and", 1);
        content = content.replacen("and\n    <code class=\"Fn\">", "and <code class=\"Fn\">", 1);
    }
    let opening = if let Some(tag) = tag {
        let tag = escape_html(&tag);
        format!("<p class=\"Pp\" id=\"{tag}\">")
    } else {
        "<p class=\"Pp\">".to_owned()
    };
    append(output, &opening, maximum)?;
    let content = if content.contains("class=\"Rs\"") || has_font_request {
        wrap_html_reference_paragraph(&content, opening.len())
    } else {
        wrap_html_plain_paragraph(&content, opening.len())
    };
    append(output, &content, maximum)?;
    append(output, "</p>\n", maximum)
}

/// mandoc's HTML writer folds ordinary ASCII paragraph prose at its 80-column
/// output field.  Semantic markup and non-ASCII/device-escaped content keep
/// their dedicated paths: this narrow helper only owns plain text, where
/// splitting at source-independent word boundaries is lossless.
fn wrap_html_plain_paragraph(content: &str, opening_width: usize) -> String {
    if content.contains('<') || !content.is_ascii() || opening_width >= 80 {
        return content.to_owned();
    }
    let mut output = String::with_capacity(content.len());
    let mut column = opening_width;
    for word in content.split(' ') {
        if word.is_empty() {
            continue;
        }
        let separator = usize::from(!output.is_empty());
        if column.saturating_add(separator).saturating_add(word.len()) > 80 {
            output.push_str("\n    ");
            output.push_str(word);
            column = 4 + word.len();
        } else {
            if separator != 0 {
                output.push(' ');
                column += 1;
            }
            output.push_str(word);
            column += word.len();
        }
    }
    output
}

/// Headings share the HTML device's narrow output field, but use its
/// two-column continuation indentation rather than paragraph indentation.
fn wrap_html_heading(content: &str, opening_width: usize) -> String {
    const WIDTH: usize = 72;
    if content.contains('<') || !content.is_ascii() || opening_width >= WIDTH {
        return content.to_owned();
    }
    let mut output = String::with_capacity(content.len());
    let mut column = opening_width;
    for word in content.split(' ') {
        if word.is_empty() {
            continue;
        }
        let separator = usize::from(!output.is_empty());
        if column.saturating_add(separator).saturating_add(word.len()) > WIDTH {
            output.push_str("\n  ");
            output.push_str(word);
            column = 2 + word.len();
        } else {
            if separator != 0 {
                output.push(' ');
                column += 1;
            }
            output.push_str(word);
            column += word.len();
        }
    }
    output
}

/// The historical HTML writer formats mdoc bibliography markup as device
/// output, not DOM pretty-printing: it folds markup tokens at column 78 and
/// uses a four-column continuation.  Keep that narrow behavior local to
/// `Rs`; ordinary semantic HTML intentionally retains its authored flow.
fn wrap_html_reference_paragraph(content: &str, opening_width: usize) -> String {
    const WIDTH: usize = 78;
    let mut output = String::with_capacity(content.len());
    let mut column = opening_width;
    let mut pending_space = false;
    let mut cursor = 0_usize;

    while cursor < content.len() {
        let remainder = &content[cursor..];
        if let Some(character) = remainder.chars().next()
            && character.is_whitespace()
        {
            let whitespace_end = cursor
                + remainder
                    .char_indices()
                    .take_while(|(_, character)| character.is_whitespace())
                    .last()
                    .map_or(character.len_utf8(), |(index, character)| {
                        index + character.len_utf8()
                    });
            let whitespace = &content[cursor..whitespace_end];
            if whitespace.contains('\n') {
                output.push_str(whitespace);
                column = whitespace
                    .rsplit_once('\n')
                    .map_or(column + whitespace.len(), |(_, tail)| tail.len());
                pending_space = false;
            } else {
                pending_space = true;
            }
            cursor = whitespace_end;
            continue;
        }

        let token_end = if remainder.starts_with('<') {
            remainder
                .find('>')
                .map_or(content.len(), |index| cursor + index + 1)
        } else {
            cursor
                + remainder
                    .char_indices()
                    .take_while(|(_, character)| !character.is_whitespace() && *character != '<')
                    .last()
                    .map_or_else(
                        || remainder.chars().next().map_or(0, char::len_utf8),
                        |(index, character)| index + character.len_utf8(),
                    )
        };
        let token_end = html_compact_element_end(content, cursor).unwrap_or(token_end);
        let token = &content[cursor..token_end];
        let separator = usize::from(pending_space && !output.is_empty());
        if separator != 0 && column.saturating_add(separator + token.len()) > WIDTH {
            output.push_str("\n    ");
            column = 4;
        } else if separator != 0 {
            output.push(' ');
            column += 1;
        }
        output.push_str(token);
        column += token.len();
        pending_space = false;
        cursor = token_end;
    }
    output
}

/// Keep a no-space semantic HTML element together while applying the device
/// output-field fold.  The C writer opens and closes these wrappers around
/// one word atomically; treating their opening tag separately would allow a
/// long literal wrapper to overflow before its visible word is considered.
fn html_compact_element_end(content: &str, start: usize) -> Option<usize> {
    let remainder = content.get(start..)?;
    let opening_end = remainder.find('>')?;
    let opening = &remainder[..=opening_end];
    if !opening.starts_with('<') || opening.starts_with("</") || opening.ends_with("/>") {
        return None;
    }
    let name_end =
        opening[1..].find(|character: char| character.is_whitespace() || character == '>')? + 1;
    let name = &opening[1..name_end];
    let closing = format!("</{name}>");
    let content_start = start + opening_end + 1;
    let closing_start = content.get(content_start..)?.find(&closing)? + content_start;
    (!content[content_start..closing_start]
        .chars()
        .any(char::is_whitespace))
    .then_some(closing_start + closing.len())
}

/// Render an `Rs` block as the HTML device's inline citation.  The parser
/// has already imposed libmandoc's field order; presentation supplies field
/// classes, author conjunctions, field separators, and its final period.
fn render_html_reference_block(node: NodeRef<'_>, limits: &Limits) -> String {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return String::new();
    };
    let fields = body
        .children()
        .filter(|child| !child.flags().no_print)
        .collect::<Vec<_>>();
    let mut phrases = Vec::new();
    let mut index = 0_usize;
    while index < fields.len() {
        if fields[index].macro_name() == Some("%A") {
            let mut authors = Vec::new();
            while index < fields.len() && fields[index].macro_name() == Some("%A") {
                if let Some(author) = render_html_reference_field(fields[index], limits) {
                    authors.push(author);
                }
                index += 1;
            }
            let authors = match authors.as_slice() {
                [] => String::new(),
                [author] => author.clone(),
                [first, second] => format!("{first} and {second}"),
                _ => {
                    let mut value = authors[..authors.len() - 1].join(", ");
                    value.push_str(", and ");
                    value.push_str(authors.last().expect("authors is nonempty"));
                    value
                }
            };
            if !authors.is_empty() {
                phrases.push(authors);
            }
            continue;
        }
        if let Some(phrase) = render_html_reference_field(fields[index], limits) {
            phrases.push(phrase);
        }
        index += 1;
    }
    if phrases.is_empty() {
        return String::new();
    }
    format!("<cite class=\"Rs\">{}.</cite>", phrases.join(", "))
}

fn render_html_reference_field(field: NodeRef<'_>, limits: &Limits) -> Option<String> {
    let name = field.macro_name()?;
    if !matches!(
        name,
        "%A" | "%B"
            | "%C"
            | "%D"
            | "%I"
            | "%J"
            | "%N"
            | "%O"
            | "%P"
            | "%Q"
            | "%R"
            | "%T"
            | "%U"
            | "%V"
    ) {
        return None;
    }
    let value = render_html_inline_nodes(field.children().collect::<Vec<_>>(), limits);
    if value.is_empty() {
        return None;
    }
    let class = &name[1..];
    if name == "%U" {
        let href = html_first_visible_text_in_source_order(field)?;
        return Some(format!(
            "<a class=\"Rs{class}\" href=\"{}\">{value}</a>",
            escape_html(href)
        ));
    }
    let element = if matches!(name, "%B" | "%I" | "%J") {
        "i"
    } else {
        "span"
    };
    Some(format!(
        "<{element} class=\"Rs{class}\">{value}</{element}>"
    ))
}

fn render_html_reference_paragraph(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let citation = render_html_reference_block(node, limits);
    if citation.is_empty() {
        return Ok(());
    }
    let opening = "<p class=\"Pp\">";
    append(output, opening, maximum)?;
    append(
        output,
        &wrap_html_reference_paragraph(&citation, opening.len()),
        maximum,
    )?;
    append(output, "</p>\n", maximum)
}

fn render_html_inline_nodes(nodes: Vec<NodeRef<'_>>, limits: &Limits) -> String {
    let mut output = String::new();
    let mut previous: Option<NodeRef<'_>> = None;
    for node in nodes {
        if node.flags().no_print || node.ancestors().any(|ancestor| ancestor.flags().no_print) {
            continue;
        }
        let content = match node.kind() {
            NodeKind::Text => node.text().map(|text| {
                render_html_visible_text_with_font(
                    text,
                    limits,
                    html_request_font_before(node).current,
                )
            }),
            NodeKind::Equation => node.equation().map(|value| {
                // Keep the device's equation envelope even when an eqn block
                // appears in a semantic paragraph.  In particular, the
                // upstream regression harness locates MathML through this
                // exact marker rather than through surrounding block layout.
                let mathml = node.equation_terminal().map_or_else(
                    || escape_html(value),
                    |equation| render_html_equation(equation, limits),
                );
                format!("<math class=\"eqn\">{mathml}</math>")
            }),
            NodeKind::Block if node.macro_name() == Some("Fo") => {
                Some(render_html_function_declaration(node, limits))
            }
            NodeKind::Block if node.macro_name() == Some("Rs") => {
                Some(render_html_reference_block(node, limits))
            }
            NodeKind::Element if node.macro_name() == Some("ft") => None,
            _ => {
                let nested = render_html_inline_nodes(node.children().collect::<Vec<_>>(), limits);
                match node.macro_name() {
                    Some("br") => Some("\n  <br/>\n  ".to_owned()),
                    Some("sp") if node.flags().no_fill => Some("\n".to_owned()),
                    Some("Pp" | "sp") => None,
                    Some("Tg") if node.flags().deep_link_target && !nested.is_empty() => {
                        Some(format!("<mark id=\"{}\"></mark>", escape_html(&nested)))
                    }
                    _ if nested.is_empty() => None,
                    Some("Pq") if node.kind() == NodeKind::Block => Some(format!("({nested})")),
                    Some("Bq") if nested.starts_with('[') && nested.ends_with(']') => Some(nested),
                    Some("Bq") => Some(format!("[{nested}]")),
                    _ => {
                        if let Some(enclosure) = node.enclosure() {
                            let closing = enclosure.closing.as_deref().unwrap_or_default();
                            Some(format!(
                                "{}{}{}",
                                escape_html(&enclosure.opening),
                                nested,
                                escape_html(closing)
                            ))
                        } else {
                            Some(nested)
                        }
                    }
                }
            }
        };
        let Some(content) = content.filter(|content| !content.is_empty()) else {
            continue;
        };
        let tag = html_inline_tag(node, &content);
        let content = render_html_inline_semantics(
            node,
            &content,
            node.flags().deep_link_target,
            tag.as_deref(),
        );
        if let Some(previous) = previous {
            if node.flags().line_start && node.flags().no_fill && node.macro_name() != Some("sp") {
                output.push('\n');
            } else if previous.flags().permalink
                && node.flags().line_start
                && node.macro_name() != Some("br")
            {
                if previous.macro_name() == Some("Fn") {
                    output.push(' ');
                } else if previous.flags().deep_link_target {
                    output.push_str("\n    ");
                } else {
                    output.push_str("\n  ");
                }
            } else if matches!(node.macro_name(), Some("Fn" | "Fo")) && node.flags().line_start {
                output.push_str("\n    ");
            } else if node.macro_name() == Some("Tg") && node.flags().deep_link_target {
                output.push(' ');
            } else if node.flags().deep_link_target {
                output.push_str("\n    ");
            } else if previous.macro_name() != Some("br")
                && node.macro_name() != Some("br")
                && node.macro_name() != Some("sp")
                && !previous.flags().delimiter_open
                && !node.flags().delimiter_close
            {
                output.push(' ');
            }
        }
        if node.flags().permalink {
            if let Some(tag) = tag {
                let tag = escape_html(&tag);
                output.push_str("<a class=\"permalink\" href=\"#");
                output.push_str(&tag);
                output.push_str("\">");
                output.push_str(&content);
                output.push_str("</a>");
                if node.macro_name() == Some("Fn") {
                    output.push_str("()");
                }
            } else {
                output.push_str(&content);
            }
        } else {
            output.push_str(&content);
            if node.macro_name() == Some("Fn") {
                output.push_str("()");
            }
        }
        previous = Some(node);
    }
    output
}

/// Render an mdoc `Fo` declaration as one callable function phrase.  Its
/// Head owns the function destination and its Body owns the parenthesized
/// argument sequence; the terminating `Fc` contributes the declaration's
/// semicolon without surviving as a public AST node.
fn render_html_function_declaration(node: NodeRef<'_>, limits: &Limits) -> String {
    let Some(head) = node.children().find(|child| child.kind() == NodeKind::Head) else {
        return String::new();
    };
    let name = render_html_inline_nodes(head.children().collect::<Vec<_>>(), limits);
    if name.is_empty() {
        return String::new();
    }
    let tag = head
        .tag()
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .or_else(|| html_automatic_target(&name));
    let id = head
        .flags()
        .deep_link_target
        .then(|| tag.as_deref().map(escape_html))
        .flatten()
        .map_or_else(String::new, |tag| format!(" id=\"{tag}\""));
    let code = format!("<code class=\"Fn\"{id}>{name}</code>");
    let name = if head.flags().permalink {
        tag.map_or(code.clone(), |tag| {
            let tag = escape_html(&tag);
            format!("<a class=\"permalink\" href=\"#{tag}\">{code}</a>")
        })
    } else {
        code
    };
    let arguments = node
        .children()
        .find(|child| child.kind() == NodeKind::Body)
        .map(|body| render_html_inline_nodes(body.children().collect::<Vec<_>>(), limits))
        .unwrap_or_default();
    format!("{name}({arguments});")
}

/// Return the destination spelling that mandoc derives for an inline target.
/// Explicit `.Tg` values win; automatic mdoc names omit one leading option
/// dash and end at the first source-space boundary.
fn html_inline_tag(node: NodeRef<'_>, content: &str) -> Option<String> {
    if node.macro_name() == Some("Fn") {
        return node
            .tag()
            .filter(|tag| !tag.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                html_first_visible_text_in_source_order(node).and_then(html_automatic_target)
            });
    }
    node.tag()
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            (node.flags().deep_link_target || node.flags().permalink).then(|| {
                let content = content.strip_prefix('-').unwrap_or(content);
                let end = content.find(char::is_whitespace).unwrap_or(content.len());
                content[..end].to_owned()
            })
        })
        .filter(|tag| !tag.is_empty())
}

/// Map the normalized mdoc inline families to their stable HTML device tags.
/// The public arena intentionally retains the generic macro spelling, so this
/// remains a renderer-only mapping rather than an AST widening.
fn render_html_inline_semantics(
    node: NodeRef<'_>,
    content: &str,
    id: bool,
    tag: Option<&str>,
) -> String {
    let (element, class, prefix) = match node.macro_name() {
        Some("B") => return format!("<b>{content}</b>"),
        Some("I") => return format!("<i>{content}</i>"),
        Some("Fl") => ("code", "Fl", (!content.starts_with("--")).then_some("-")),
        Some("Cm" | "Dv" | "Er" | "Ev" | "Ic" | "Li") => {
            ("code", node.macro_name().unwrap_or_default(), None)
        }
        Some("Em") => ("i", "Em", None),
        Some("Sy") => ("b", "Sy", None),
        Some("Fa") => ("var", "Fa", None),
        Some("Fn") => ("code", "Fn", None),
        Some("No" | "Ms") => ("span", node.macro_name().unwrap_or_default(), None),
        _ => return content.to_owned(),
    };
    let id = id
        .then(|| tag.map(escape_html))
        .flatten()
        .map_or_else(String::new, |tag| format!(" id=\"{tag}\""));
    format!(
        "<{element} class=\"{class}\"{id}>{prefix}{content}</{element}>",
        prefix = prefix.unwrap_or_default()
    )
}

fn html_is_semantic_inline_macro(node: NodeRef<'_>) -> bool {
    matches!(
        node.macro_name(),
        Some(
            "Fl" | "Cm"
                | "Dv"
                | "Er"
                | "Ev"
                | "Ic"
                | "Li"
                | "Em"
                | "Sy"
                | "Fa"
                | "Fn"
                | "No"
                | "Ms"
        )
    )
}

/// Render terminal formats from semantic section blocks rather than a flat
/// preorder stream. This retains a section's Head/Body boundary even though a
/// Head's text often has no independent line-start flag.
fn render_terminal_document(
    document: &Document,
    format: RenderFormat,
    width: usize,
    maximum: usize,
    limits: &Limits,
) -> Result<String, RenderError> {
    let mut output = String::new();
    let protected_header_lines =
        append_terminal_header(document, format, width, limits, &mut output, maximum)?;
    let Some(root) = document.node(document.root()) else {
        return Ok(output);
    };
    for node in root.children() {
        // A malformed, argumentless `.SH`/`.SS` is deliberately absent from
        // the compatible tree: subsequent man nodes are therefore attached
        // directly to Root.  term.c keeps the current man body field in that
        // recovery shape, though, rather than resetting it to column zero.
        // Root-level section blocks still own their distinct heading/body
        // geometry; every other direct man child resumes in the ordinary
        // seven-column body field.
        let indentation = if document.macro_set() == MacroSet::Man && !is_section_block(node) {
            7
        } else {
            0
        };
        render_terminal_node(node, format, limits, indentation, &mut output, maximum)?;
    }
    let protected_footer_lines =
        append_terminal_footer(document, format, width, limits, &mut output, maximum)?;
    let mut rendered = wrap_terminal_output(
        output.trim_end(),
        width,
        maximum,
        protected_header_lines,
        protected_footer_lines,
    )?;
    if !rendered.is_empty() {
        append(&mut rendered, "\n", maximum)?;
    }
    Ok(rendered)
}

/// Emit the shared terminal-page heading from normalized metadata.
///
/// The stable terminal device reserves the first line for the manual
/// identifier at both margins and the collection name in the centre.  This is
/// deliberately independent from the package-specific body walkers: man and
/// mdoc produce the same three-field geometry once parsing has normalised
/// their control macros into [`crate::Metadata`].  Pages without a title or a
/// section (for example a raw roff fragment) remain headerless.
fn append_terminal_header(
    document: &Document,
    format: RenderFormat,
    width: usize,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<usize, RenderError> {
    let metadata = document.metadata();
    let Some(title) = metadata.title.as_deref() else {
        return Ok(0);
    };
    let section = metadata.section.as_deref();
    if document.macro_set() == MacroSet::Man && section.is_none() {
        return Ok(0);
    }
    let identifier =
        section.map_or_else(|| title.to_owned(), |section| format!("{title}({section})"));
    let mut volume = metadata.volume.as_deref().map_or_else(
        || {
            if document.macro_set() == MacroSet::Mdoc && section.is_none() {
                "LOCAL".to_owned()
            } else {
                terminal_default_volume(section.unwrap_or_default()).to_owned()
            }
        },
        str::to_owned,
    );
    if let Some(architecture) = metadata.arch.as_deref()
        && !architecture.is_empty()
    {
        volume.push_str(" (");
        volume.push_str(architecture);
        volume.push(')');
    }
    let identifier = render_visible_text(&identifier, format, limits);
    let volume = render_visible_text(&volume, format, limits);
    let identifier_width = display_width(&identifier);
    let volume_width = display_width(&volume);
    // `print_mdoc_head()`/`print_man_head()` first reserve the middle field.
    // The strict comparison is intentional: when exactly full, the C device
    // uses the wide middle-volume field and omits the duplicated right title
    // instead of concatenating all three header words.
    let centre = if identifier_width
        .saturating_add(1)
        .saturating_mul(2)
        .saturating_add(volume_width)
        < width
    {
        width.saturating_sub(volume_width).saturating_add(1) / 2
    } else if volume_width < width {
        width.saturating_sub(volume_width)
    } else {
        0
    };
    // An identifier wider than its initially reserved field makes the C
    // terminal flush it on a line of its own before rendering the volume.
    // Preserve that overflow path for intentionally tiny custom widths too.
    if identifier_width > centre {
        append(output, &identifier, maximum)?;
        append(output, "\n", maximum)?;
        // A title wider than the device line owns its line outright.  The
        // terminal device still right-justifies the otherwise fitting manual
        // volume on the following physical line; treating both fields as
        // unpositioned fallbacks loses that stable page-heading geometry.
        if volume_width <= width {
            append(
                output,
                &" ".repeat(width.saturating_sub(volume_width)),
                maximum,
            )?;
        }
        append(output, &volume, maximum)?;
        append(output, "\n\n", maximum)?;
        return Ok(2);
    }
    let left_padding = centre.saturating_sub(identifier_width);
    let right_start = if centre
        .saturating_add(volume_width)
        .saturating_add(identifier_width)
        < width
    {
        width.saturating_sub(identifier_width)
    } else {
        width
    };
    let right_padding = right_start.saturating_sub(centre.saturating_add(volume_width));
    append(output, &identifier, maximum)?;
    append(output, &" ".repeat(left_padding), maximum)?;
    append(output, &volume, maximum)?;
    append(output, &" ".repeat(right_padding), maximum)?;
    if right_start.saturating_add(identifier_width) <= width {
        append(output, &identifier, maximum)?;
    }
    append(output, "\n\n", maximum)?;
    Ok(1)
}

fn terminal_default_volume(section: &str) -> &'static str {
    // `msec.in` is part of the pinned terminal-device contract. Match the
    // whole section rather than its first character: `3p`, for example, has
    // a distinct Perl volume rather than section 3's library heading.
    match section {
        "1" => "General Commands Manual",
        "2" => "System Calls Manual",
        "3" => "Library Functions Manual",
        "3p" => "Perl Library Manual",
        "4" => "Device Drivers Manual",
        "5" => "File Formats Manual",
        "6" => "Games Manual",
        "7" => "Miscellaneous Information Manual",
        "8" => "System Manager's Manual",
        "9" => "Kernel Developer's Manual",
        _ => "",
    }
}

/// Emit the metadata footer using the terminal device's fixed three-column
/// layout. Man pages end with `system / date / identifier`; mdoc pages use the
/// declared system at both margins. Like the header, this is metadata-only and
/// never depends on the host locale or clock.
fn append_terminal_footer(
    document: &Document,
    format: RenderFormat,
    width: usize,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<usize, RenderError> {
    let metadata = document.metadata();
    let Some(title) = metadata.title.as_deref() else {
        return Ok(0);
    };
    let section = metadata.section.as_deref();
    if document.macro_set() == MacroSet::Man && section.is_none() {
        return Ok(0);
    }
    // A syntactically present man `.TH` always opens the terminal page. Its
    // date and system fields may both be empty or recovered independently;
    // they still produce the three-column footer, just like an argument-less
    // `.TH`. Documents without a title request returned above remain
    // footerless.
    let date = metadata.date.as_deref().unwrap_or("");
    let system = metadata.os.as_deref().unwrap_or("OpenBSD");
    let right = if document.macro_set() == MacroSet::Man {
        format!("{title}({})", section.unwrap_or_default())
    } else {
        system.to_owned()
    };
    let system = render_visible_text(system, format, limits);
    let date = render_visible_text(date, format, limits);
    let right = render_visible_text(&right, format, limits);
    if document_ends_with_terminal_spacing(document) {
        append_terminal_footer_space(output, maximum)?;
    } else {
        append_blank_line(output, maximum)?;
    }
    append_terminal_three_column_line(output, &system, &date, &right, width, maximum)
}

/// Reserve the terminal device's final vertical slot before its page footer.
///
/// This intentionally differs from [`append_blank_line`]: after a document's
/// final `.sp`, `term_vspace()` used by libmandoc's footer is cumulative.  The
/// request has already completed one empty line, but the footer still requests
/// another one.  Boxed tables and negative spacing retain their private skip
/// markers, which are consumed by that same request instead of manufacturing a
/// blank line.
fn append_terminal_footer_space(output: &mut String, maximum: usize) -> Result<(), RenderError> {
    if output.ends_with(TERMINAL_SENTENCE_PENDING_MARKER) {
        let _ = output.pop();
    }
    if take_terminal_vertical_skip(output) || take_terminal_table_vertical_skip(output) {
        return Ok(());
    }
    if output.is_empty() {
        return Ok(());
    }
    if output.ends_with('\n') {
        append(output, "\n", maximum)
    } else {
        append(output, "\n\n", maximum)
    }
}

/// Whether the last terminal-affecting request in the document is `.sp`.
///
/// The terminal backend only exposes its current field state to the footer,
/// while this renderer deliberately keeps that state local to each semantic
/// node.  Recover the one observable cross-boundary fact by following the
/// normal recursive rendering order.  Text nested below `.sp` is its numeric
/// argument, not trailing prose; otherwise the last text/table node resets the
/// marker.  Structural wrapper nodes have no terminal effect of their own.
fn document_ends_with_terminal_spacing(document: &Document) -> bool {
    fn visit(node: NodeRef<'_>, inside_spacing: bool, last_is_spacing: &mut bool) {
        if node.flags().no_print {
            return;
        }
        let is_spacing = node.macro_name() == Some("sp");
        if matches!(node.kind(), NodeKind::Text | NodeKind::Table) && !inside_spacing {
            *last_is_spacing = false;
        }
        for child in node.children() {
            visit(child, inside_spacing || is_spacing, last_is_spacing);
        }
        if is_spacing {
            *last_is_spacing = true;
        }
    }

    let Some(root) = document.node(document.root()) else {
        return false;
    };
    let mut last_is_spacing = false;
    for child in root.children() {
        visit(child, false, &mut last_is_spacing);
    }
    last_is_spacing
}

fn append_terminal_three_column_line(
    output: &mut String,
    left: &str,
    centre: &str,
    right: &str,
    width: usize,
    maximum: usize,
) -> Result<usize, RenderError> {
    let left_width = display_width(left);
    let centre_width = display_width(centre);
    let right_width = display_width(right);
    if left_width
        .saturating_add(centre_width)
        .saturating_add(right_width)
        > width
    {
        append(output, left, maximum)?;
        // Keep the left and centre fields together whenever that pair fits:
        // `term_end()` uses the ordinary centre column even if the right
        // identifier must spill to its own line.  Conversely an oversized
        // centre field gets its own line, while a fitting right field remains
        // right-justified on the final one.
        let left_and_centre_fit = left_width.saturating_add(centre_width) <= width;
        if left_and_centre_fit {
            let centre_start = width.saturating_sub(centre_width).saturating_add(1) / 2;
            append(
                output,
                &" ".repeat(centre_start.saturating_sub(left_width)),
                maximum,
            )?;
            append(output, centre, maximum)?;
        }
        append(output, "\n", maximum)?;
        if !left_and_centre_fit {
            let centre_start = width.saturating_sub(centre_width).saturating_add(1) / 2;
            append(output, &" ".repeat(centre_start), maximum)?;
            append(output, centre, maximum)?;
            append(output, "\n", maximum)?;
        }
        if right_width <= width {
            append(
                output,
                &" ".repeat(width.saturating_sub(right_width)),
                maximum,
            )?;
        }
        append(output, right, maximum)?;
        return Ok(if left_and_centre_fit { 2 } else { 3 });
    }
    let centre_start = width.saturating_sub(centre_width).saturating_add(1) / 2;
    let left_padding = centre_start.saturating_sub(left_width);
    let right_start = width.saturating_sub(right_width);
    let right_padding = right_start.saturating_sub(centre_start.saturating_add(centre_width));
    append(output, left, maximum)?;
    append(output, &" ".repeat(left_padding), maximum)?;
    append(output, centre, maximum)?;
    append(output, &" ".repeat(right_padding), maximum)?;
    append(output, right, maximum)?;
    Ok(1)
}

#[allow(clippy::too_many_lines)] // Terminal macro presentation remains an explicit ordered dispatcher.
fn render_terminal_node(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if node.flags().no_print || node.ancestors().any(|ancestor| ancestor.flags().no_print) {
        return Ok(());
    }
    // PD is a stateful man formatter request. Depending on recovery shape it
    // may be represented as an Element or a partial Block.  A partial Block
    // owns a Body containing the following next-line scope, which remains
    // visible; its Head is the private spacing argument.
    if node.macro_name() == Some("PD") {
        if node.kind() == NodeKind::Block {
            for body in node
                .children()
                .filter(|child| child.kind() == NodeKind::Body)
            {
                for child in body.children() {
                    render_terminal_node(child, format, limits, indentation, output, maximum)?;
                }
            }
        }
        return Ok(());
    }
    // `Tg` establishes navigation metadata only. It never contributes a
    // terminal glyph, including when recovery leaves its tag spelling in an
    // otherwise visible compatible-AST element.
    if node.macro_name() == Some("Es") {
        // `Es` is terminal-invisible, but it consumes the same-line slot
        // that a preceding empty `Fl` would otherwise use for attachment.
        // Its next visible sibling therefore resumes with a normal space.
        if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
            mark_terminal_force_separator(output, maximum)?;
        }
        return Ok(());
    }
    if node.macro_name() == Some("Sm") {
        terminal_apply_mdoc_spacing(node, output, maximum)?;
        return Ok(());
    }
    if node.macro_name() == Some("ta") {
        // `.ta` owns terminal formatter state only.  Keep its arguments out
        // of visible flow and defer the state transition to the final width
        // pass, where source tabs are expanded.
        append_terminal_tab_stops_request(node, output, maximum)?;
        return Ok(());
    }
    if node.macro_name() == Some("Tg") {
        return Ok(());
    }
    if let Some(closing) = terminal_embedded_quote_closing(node, format) {
        let font = if node
            .ancestors()
            .any(|ancestor| ancestor.macro_name() == Some("Bf"))
        {
            TerminalFont::Roman
        } else {
            terminal_inherited_font(node)
        };
        append(output, &render_terminal_font(closing, font), maximum)?;
        append(
            output,
            &TERMINAL_CONTINUE_SOURCE_LINE_MARKER.to_string(),
            maximum,
        )?;
        return Ok(());
    }
    if terminal_mdoc_sm_relinked_invalid_argument(node)
        && terminal_mdoc_sm_relinked_argument_precedes(node)
        && terminal_has_visible_output(output)
    {
        // Recovery turns the remaining words of `.Sm bad ...` into ordinary
        // sibling flow. They keep their normal internal spacing even when
        // the preceding valid request had disabled global mdoc spacing.
        mark_terminal_force_separator(output, maximum)?;
    }
    if matches!(node.macro_name(), Some("ce" | "rj")) {
        render_terminal_adjusted_input_lines(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if terminal_spacing_disabled(output)
        && terminal_has_visible_output(output)
        && terminal_mdoc_sm_starts_new_source_phrase(node)
        && !terminal_mdoc_sm_relinked_valid_argument(node)
        && !terminal_mdoc_sm_relinked_argument_precedes(node)
    {
        // `.Sm off` suppresses in-line argument separation, but a new
        // physical macro/text line still begins an ordinary filled phrase.
        mark_terminal_force_separator_after_sentence(output, maximum)?;
    }
    if is_mdoc_description_block(node) {
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            let children = body.children().collect::<Vec<_>>();
            let paragraph = children
                .iter()
                .position(|child| child.macro_name() == Some("Pp"));
            // A recovered description can contain more than one physical
            // source line. Its body remains description flow until a Pp
            // restores ordinary structural rendering.
            let description_end = paragraph.unwrap_or(children.len());
            let mut description = String::new();
            for child in &children[..description_end] {
                collect_terminal_text(*child, format, limits, &mut description);
            }
            let prefix = if matches!(format, RenderFormat::Utf8) {
                "–"
            } else {
                "-"
            };
            let phrase = if description.is_empty() {
                prefix.to_owned()
            } else {
                format!("{prefix} {description}")
            };
            append_terminal_text(
                output,
                &phrase,
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
            for child in &children[description_end..] {
                render_terminal_node(*child, format, limits, indentation, output, maximum)?;
            }
        }
        return Ok(());
    }
    if is_section_block(node) {
        let mut heading = String::new();
        let mut body = None;
        let mdoc_heading = matches!(node.macro_name(), Some("Sh" | "Ss"));
        for child in node.children() {
            match child.kind() {
                NodeKind::Head if mdoc_heading => {
                    collect_terminal_mdoc_heading(child, format, limits, &mut heading);
                }
                NodeKind::Head => collect_terminal_text(child, format, limits, &mut heading),
                NodeKind::Body => body = Some(child),
                _ => {}
            }
        }
        if heading.chars().all(|character| {
            character.is_whitespace() || character == TERMINAL_NONBREAKING_SPACE_MARKER
        }) {
            // An mdoc section title consisting only of escaped horizontal
            // space is a recovered empty heading. It owns no device glyph;
            // retaining it as one literal blank would leave a visible-space
            // line between the surrounding paragraphs.
            heading.clear();
        }
        let empty_mdoc_heading = mdoc_heading && heading.is_empty();
        if !heading.is_empty() {
            if !is_first_nested_section(node) {
                // A section heading owns its normal separator below a table.
                // Do not weaken genuine negative `.sp` recovery here.
                let _ = take_terminal_table_vertical_skip(output);
                if terminal_previous_empty_section(node, format, limits) {
                    if !output.is_empty() && !output.ends_with('\n') {
                        append(output, "\n", maximum)?;
                    }
                } else if matches!(node.macro_name(), Some("SH" | "SS")) {
                    // `PD` is terminal presentation state, including at a
                    // following man section boundary.  A zero request merely
                    // completes the preceding line, while larger values add
                    // that many vertical slots before the next heading.
                    let density = terminal_man_paragraph_density(node).unwrap_or(1);
                    if density == 0 {
                        if !output.is_empty() && !output.ends_with('\n') {
                            append(output, "\n", maximum)?;
                        }
                    } else {
                        append_blank_line(output, maximum)?;
                        for _ in 1..density {
                            append(output, "\n", maximum)?;
                        }
                    }
                } else {
                    append_blank_line(output, maximum)?;
                }
            }
            if matches!(node.macro_name(), Some("SH" | "SS")) {
                // A long man heading begins at the section's heading column,
                // while each wrapped terminal continuation enters the Body
                // field. Keep this device-only hanging geometry out of the
                // compatible AST.
                append_terminal_hanging_indent(
                    output,
                    terminal_section_body_indent(node),
                    maximum,
                )?;
            }
            append(
                output,
                &" ".repeat(terminal_section_heading_indent(node)),
                maximum,
            )?;
            if mdoc_heading {
                append(output, &heading, maximum)?;
            } else {
                append(output, &render_terminal_bold(&heading, format), maximum)?;
            }
        } else if empty_mdoc_heading && !output.is_empty() {
            // A visibly empty mdoc section still transitions through the
            // heading field: its absent title leaves the normal section gap
            // plus the heading's own empty device line before Body prose.
            append_blank_line(output, maximum)?;
            append(output, "\n", maximum)?;
        }
        if let Some(body) = body {
            if heading.is_empty()
                && terminal_empty_man_section_starts_plain_flow(node, body)
                && !output.is_empty()
            {
                // An argumentless man section retains its Body after
                // validation. term.c treats that otherwise invisible section
                // opener as the ordinary paragraph boundary before prose or
                // a fill-mode transition. Structural paragraph/list blocks
                // own their own gap, so restrict this to plain body flow.
                append_blank_line(output, maximum)?;
            } else if !heading.is_empty()
                && (terminal_has_visible_text(body, format, limits)
                    || terminal_has_pd_control(body))
            {
                append(output, "\n", maximum)?;
            }
            let body_indentation = terminal_section_body_indent(node);
            for child in body.children() {
                render_terminal_node(child, format, limits, body_indentation, output, maximum)?;
            }
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Rs") {
        render_terminal_reference_block(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("PP") {
        let density = terminal_man_paragraph_density(node);
        // A PD immediately after a section heading changes the following PP
        // before it emits any visible material, so it must not manufacture a
        // blank line before that first Body phrase.  Later paragraphs retain
        // the normal blank plus PD's additional vertical slots.
        if terminal_has_visible_predecessor(node) {
            append_terminal_following_vertical_slot(node, output, maximum)?;
            if density == Some(0) {
                if !output.is_empty() && !output.ends_with('\n') {
                    append(output, "\n", maximum)?;
                }
            } else {
                append_blank_line(output, maximum)?;
                for _ in 1..density.unwrap_or(1) {
                    append(output, "\n", maximum)?;
                }
            }
        }
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            for child in body.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("TP") {
        render_terminal_man_tagged_paragraph(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("HP") {
        render_terminal_man_hanging_paragraph(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Fo") {
        render_terminal_mdoc_function_block(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Bf") {
        // `Bf`'s Head is validation/configuration input only; its retained
        // extra arguments remain observable in the public AST but the
        // terminal device skips that Head and applies the normalized font to
        // Body flow alone.
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            for child in body.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Bl")
        && terminal_mdoc_list_is_empty(node)
    {
        // An empty mdoc list is presentation-transparent except that its
        // block boundary completes the preceding physical source phrase.
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Bl")
        && node.list_kind() == Some(NormalizedListKind::Plain)
    {
        render_terminal_plain_list(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Bl")
        && node.list_kind() == Some(NormalizedListKind::Column)
    {
        return render_terminal_column_list(node, format, limits, indentation, output, maximum);
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Bl")
        && matches!(
            node.list_kind(),
            Some(NormalizedListKind::Bullet | NormalizedListKind::Ordered)
        )
    {
        render_terminal_marked_list(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Bl")
        && node.list_kind() == Some(NormalizedListKind::Definition)
    {
        render_terminal_definition_list(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Eo") {
        render_terminal_explicit_enclosure(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Op")
        && terminal_mdoc_synopsis(node)
    {
        // In SYNOPSIS each optional form is one keepable declaration field.
        // Collect its nested brackets and typography first, then protect its
        // internal separators so the width pass moves the whole option to
        // the continuation line rather than splitting after its opener.
        let mut optional = String::new();
        collect_terminal_text(node, format, limits, &mut optional);
        if !optional.is_empty() {
            // A kept optional form is one terminal word.  In particular, a
            // short option such as `-s` must not become the final hyphen of
            // one device line plus its letter on the next line.
            let optional = optional.replace(' ', &TERMINAL_NONBREAKING_SPACE_MARKER.to_string());
            let optional = optional.replace('-', &format!("-{TERMINAL_NO_HYPHEN_BREAK_MARKER}"));
            append_terminal_text(
                output,
                &optional,
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("En")
        && node.enclosure().is_some()
    {
        let mut contents = String::new();
        collect_terminal_text(node, format, limits, &mut contents);
        if !contents.is_empty() {
            append_terminal_text(
                output,
                &contents,
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node
            .children()
            .find(|child| child.kind() == NodeKind::Body)
            .is_some_and(terminal_quote_body_contains_display)
        && terminal_quote_delimiters(node, None, format).is_some()
    {
        return render_terminal_quote_with_display(
            node,
            format,
            limits,
            indentation,
            output,
            maximum,
        );
    }
    if node.kind() == NodeKind::Block && terminal_quote_delimiters(node, None, format).is_some() {
        let mut leading = String::new();
        let mut contents = String::new();
        let mut trailing = String::new();
        for head in node
            .children()
            .filter(|child| child.kind() == NodeKind::Head || child.flags().delimiter_open)
        {
            collect_terminal_text(head, format, limits, &mut leading);
        }
        let body = node.children().find(|child| child.kind() == NodeKind::Body);
        if let Some(body) = body {
            collect_terminal_quote_contents(body, format, limits, indentation, &mut contents);
        }
        for tail in node
            .children()
            .filter(|child| child.kind() == NodeKind::Tail || child.flags().delimiter_close)
        {
            collect_terminal_text(tail, format, limits, &mut trailing);
        }
        if let Some((opening, closing)) = terminal_quote_delimiters(node, body, format) {
            // Delimiters are generated presentation text rather than AST
            // words.  Give them the same inherited font as their opening
            // scope, then account for an empty Bf Body inserted by mdoc
            // recovery when `.Ef` closes inside this still-open enclosure.
            let opening = render_terminal_font(opening, terminal_inherited_font(node));
            let closing_font = if body.is_some_and(terminal_contains_closed_bf_scope) {
                TerminalFont::Roman
            } else {
                terminal_inherited_font(node)
            };
            let closing = if body
                .is_some_and(|body| terminal_quote_has_embedded_closer(body, node.macro_name()))
            {
                String::new()
            } else {
                render_terminal_font(closing, closing_font)
            };
            append_terminal_text(
                output,
                &format!("{leading}{opening}{contents}{closing}{trailing}"),
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && matches!(node.macro_name(), Some("D1" | "Dl")) {
        // The one-line mdoc displays are independent terminal fields. They
        // always complete the preceding device line, use one extra display
        // indent, and leave the next ordinary phrase on its own line.
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        let mut contents = String::new();
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            collect_terminal_text(body, format, limits, &mut contents);
        }
        if !contents.is_empty() {
            append_terminal_text(
                output,
                &contents,
                TerminalTextLayout {
                    line_start: true,
                    ..TerminalTextLayout::default()
                },
                indentation.saturating_add(6),
                maximum,
            )?;
        }
        if !contents.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Bd") {
        // A display is an independent vertical region. Its optional mdoc
        // offset applies in addition to the enclosing section indentation.
        // An unoffset unfilled display directly below a section heading
        // starts in the heading's normal body field; it does not insert a
        // phantom vertical gap. The literal/unfilled distinction only changes
        // tab stops; it does not add a vertical slot before the first display.
        // Offsets and all displays following visible flow retain their
        // independent device boundary.
        if terminal_has_visible_predecessor(node) {
            if node.compact() {
                if !output.is_empty() && !output.ends_with('\n') {
                    append(output, "\n", maximum)?;
                }
            } else {
                append_blank_line(output, maximum)?;
            }
        } else if !node.compact()
            && !node.literal_display()
            && (node.display_kind() != Some(DisplayKind::Literal) || node.offset().is_some())
        {
            // A first filled display, or an offset `-unfilled` display,
            // owns a device gap below a section heading. An unoffset
            // `-unfilled` display begins in that heading field like a
            // literal display; the public normalized kind alone cannot
            // distinguish its tab-stop behavior from `-literal`.
            append_blank_line(output, maximum)?;
        }
        if node.literal_display() {
            // `termp_bd_pre()` resets the device tabs to the literal
            // display's eight-column periodic field.  This state survives
            // the display until a later roff `.ta` request changes it.
            append_terminal_tab_stops_control(output, "T\u{1f}8n", maximum)?;
        }
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            let display_indentation = terminal_mdoc_display_indentation(node, indentation);
            if node.centered_display() {
                let mut centered = String::new();
                for child in body.children() {
                    render_terminal_node(
                        child,
                        format,
                        limits,
                        display_indentation,
                        &mut centered,
                        maximum,
                    )?;
                }
                append_terminal_centered_lines(output, &centered, maximum)?;
            } else {
                for child in body.children() {
                    render_terminal_node(
                        child,
                        format,
                        limits,
                        display_indentation,
                        output,
                        maximum,
                    )?;
                }
            }
        }
        // A following display or paragraph introduces its own vertical
        // boundary. Do not manufacture another one here: ordinary prose that
        // follows `.Ed` remains in its source paragraph.
        if terminal_contains_embedded_display_quote_close(node) {
            append(
                output,
                &TERMINAL_CONTINUE_SOURCE_LINE_MARKER.to_string(),
                maximum,
            )?;
        } else if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("IP") {
        // The first IP argument is its tag; an optional final `n` width
        // belongs to the following body indentation rather than visible
        // terminal content.
        // An empty recovered paragraph directly under a section heading is
        // retained as the first Body child of the field, but term.c consumes
        // it before placing that field. A field directly after the heading
        // still owns its ordinary paragraph boundary.
        let density = terminal_man_paragraph_density(node);
        if !terminal_follows_empty_section_paragraph(node)
            && (density.is_none() || terminal_has_visible_predecessor(node))
        {
            if density == Some(0) {
                if !output.is_empty() && !output.ends_with('\n') {
                    append(output, "\n", maximum)?;
                }
            } else {
                append_blank_line(output, maximum)?;
                for _ in 1..density.unwrap_or(1) {
                    append(output, "\n", maximum)?;
                }
            }
        }
        let mut body = None;
        let mut tag_nodes = Vec::new();
        // Man's IP device field is seven `n` units by default.  A short
        // tag shares that physical line with the first body phrase; a tag
        // that reaches the field width leaves the body on the next line at
        // the same field boundary.
        let tag_field_width = terminal_man_field_width(node);
        for child in node.children() {
            match child.kind() {
                NodeKind::Head => tag_nodes.extend(child.children()),
                NodeKind::Body => body = Some(child),
                _ => {}
            }
        }
        // man(7) accepts exactly one tag argument and one optional width.
        // The compatible AST retains later malformed arguments for source
        // diagnostics, but the terminal device neither prints nor interprets
        // them as tag words.
        if tag_nodes.len() > 1 {
            tag_nodes.truncate(1);
        }
        let body_indentation = if tag_field_width.is_negative() {
            indentation.saturating_sub(tag_field_width.unsigned_abs())
        } else {
            indentation.saturating_add(tag_field_width.unsigned_abs())
        };
        let mut tag = String::new();
        for child in tag_nodes {
            collect_terminal_text(child, format, limits, &mut tag);
        }
        // Man IP tags are a field, not literal display text: trailing input
        // blanks consume no extra field width and must not force the body
        // onto a continuation line.
        let tag = tag.trim_end().to_owned();
        if !tag.is_empty() {
            append_terminal_text(
                output,
                &tag,
                TerminalTextLayout {
                    line_start: true,
                    // A tag normally remains ordinary wrappable terminal
                    // prose. Preserve only authored internal spacing; field
                    // padding is protected independently below so a long tag
                    // can still wrap at the device margin. A field that
                    // itself begins beyond the standard right margin is a
                    // terminal overflow field, for which term.c suppresses
                    // normal reflow entirely.
                    keep_spacing: tag.contains('\t')
                        || tag.contains("  ")
                        || body_indentation > DEFAULT_RENDER_WIDTH,
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
        }
        if let Some(body) = body {
            // A tagged IP whose Body was closed immediately by another
            // field is a visible tag-only line. `term.c` does not materialise
            // its unused field padding; doing so would leak trailing blanks
            // into the public ASCII stream.
            let body_has_visible_text = terminal_has_visible_text(body, format, limits);
            let body_starts_with_terminal_break = terminal_body_starts_with_break(body);
            if !tag.is_empty()
                && body_has_visible_text
                && !body_starts_with_terminal_break
                && tag_field_width > 0
                && display_width(&tag) < tag_field_width.unsigned_abs()
            {
                let gap = tag_field_width
                    .unsigned_abs()
                    .saturating_sub(display_width(&tag));
                append(
                    output,
                    &TERMINAL_NONBREAKING_SPACE_MARKER
                        .to_string()
                        .repeat(gap.saturating_sub(1)),
                    maximum,
                )?;
            } else if !tag.is_empty() && body_has_visible_text && !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
            let mut inline_first_no_fill_text = !tag.is_empty()
                && body_has_visible_text
                && !body_starts_with_terminal_break
                && tag_field_width > 0
                && display_width(&tag) < tag_field_width.unsigned_abs();
            for child in body.children() {
                if inline_first_no_fill_text
                    && child.kind() == NodeKind::Text
                    && child.flags().no_fill
                    && child.flags().line_start
                {
                    render_terminal_text_node(
                        child,
                        format,
                        limits,
                        body_indentation,
                        output,
                        maximum,
                        true,
                    )?;
                    inline_first_no_fill_text = false;
                } else {
                    render_terminal_node(child, format, limits, body_indentation, output, maximum)?;
                    if !child.flags().no_print {
                        inline_first_no_fill_text = false;
                    }
                }
            }
        }
        // `post_IP()` closes the field with only one physical line. Outside
        // an explicit RS Body, the following paragraph/block owns the usual
        // vertical separation. Inside RS, adding a second line here leaks a
        // blank between the indented field and immediately resumed prose.
        if terminal_man_ip_is_in_rs_body(node) {
            if !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
        } else if density == Some(0) {
            // A zero PD keeps the following field or paragraph adjacent to
            // this IP. `post_IP()` still completes its physical line, but
            // must not manufacture the default vertical slot.
            if !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
        } else {
            append_blank_line(output, maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && matches!(node.macro_name(), Some("UR" | "MT")) {
        // The terminal device presents URI and mailto blocks' visible Body
        // first and places their Head resource in angle brackets after it.
        // MT's Tail (the optional `.ME` arguments) attaches immediately to
        // that closing resource. The semantic tree keeps all three regions
        // separate for navigation and diagnostics.
        let mut resource = String::new();
        let mut contents = String::new();
        let mut trailing = String::new();
        for child in node.children() {
            match child.kind() {
                NodeKind::Head => collect_terminal_text(child, format, limits, &mut resource),
                NodeKind::Body => collect_terminal_text(child, format, limits, &mut contents),
                NodeKind::Tail => collect_terminal_text(child, format, limits, &mut trailing),
                _ => {}
            }
        }
        if !contents.is_empty() {
            append_terminal_text(
                output,
                &contents,
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        // An empty URI or mailto request is still an explicit link boundary:
        // term.c emits `<>` after any Body text.
        append_terminal_text(
            output,
            &format!("<{resource}>"),
            TerminalTextLayout::default(),
            indentation,
            maximum,
        )?;
        if !trailing.is_empty() {
            append_terminal_text(
                output,
                &trailing,
                TerminalTextLayout {
                    join: TerminalJoin::Attach,
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("SY") {
        // A man(7) synopsis block is a device field rather than ordinary
        // nested prose.  Its command head is bold and owns a terminal line;
        // a body inside `.nf` starts in the indented synopsis continuation
        // field, while a filled body stays beside the command.
        append_blank_line(output, maximum)?;
        let head = node.children().find(|child| child.kind() == NodeKind::Head);
        let body = node.children().find(|child| child.kind() == NodeKind::Body);
        if let Some(head) = head {
            let mut command = String::new();
            collect_terminal_semantic_text(head, format, limits, TerminalFont::Bold, &mut command);
            if !command.is_empty() {
                append_terminal_text(
                    output,
                    &command,
                    TerminalTextLayout::default(),
                    indentation,
                    maximum,
                )?;
            }
        }
        let body_is_no_fill =
            body.is_some_and(|body| body.children().any(|child| child.flags().no_fill));
        if body_is_no_fill && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        if let Some(body) = body {
            let body_indentation = if body_is_no_fill {
                indentation.saturating_add(8)
            } else {
                indentation
            };
            for child in body.children() {
                render_terminal_node(child, format, limits, body_indentation, output, maximum)?;
            }
        }
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("RS") {
        let explicit_width = node
            .children()
            .find(|child| child.kind() == NodeKind::Head)
            .and_then(|head| head.children().find_map(NodeRef::text))
            .and_then(|value| {
                terminal_signed_layout_units(value).or_else(|| {
                    // RS accepts an unsuffixed roff number as a terminal
                    // field width. The device truncates its fractional part
                    // to whole cells (`3.5` therefore contributes three).
                    terminal_plain_field_width(value)
                })
            });
        // A widthless RS restores the most recent TP/IP/HP field margin in
        // its current man body, even when ordinary prose intervenes. A PP
        // resets that register; a nested RS body has no such sibling field
        // and therefore resumes the ordinary seven-cell default.
        let saved_field_width = explicit_width
            .is_none()
            .then(|| {
                let parent = node.parent()?;
                parent
                    .children()
                    .take_while(|sibling| sibling.id() != node.id())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .take_while(|sibling| sibling.macro_name() != Some("PP"))
                    .any(|sibling| matches!(sibling.macro_name(), Some("TP" | "IP" | "HP")))
                    .then(|| terminal_man_field_width(node))
            })
            .flatten();
        let restores_field_margin = saved_field_width.is_some()
            || (explicit_width.is_none()
                && node.parent().is_some_and(|parent| {
                    parent.kind() == NodeKind::Body
                        && parent.parent().is_some_and(|field| {
                            matches!(field.macro_name(), Some("TP" | "IP" | "HP"))
                        })
                }));
        let width = explicit_width.unwrap_or(7);
        let body_indentation = if let Some(saved) = saved_field_width {
            if saved.is_negative() {
                indentation.saturating_sub(saved.unsigned_abs())
            } else {
                indentation.saturating_add(saved.unsigned_abs())
            }
        } else if restores_field_margin {
            indentation
        } else if width.is_negative() {
            indentation.saturating_sub(width.unsigned_abs())
        } else {
            indentation.saturating_add(width.unsigned_abs())
        };
        if restores_field_margin
            && !terminal_man_rs_follows_empty_hanging_paragraph(node)
            && output.ends_with("\n\n")
        {
            output.pop();
        }
        if terminal_man_rs_follows_empty_hanging_paragraph(node) && output.ends_with('\n') {
            // A zero-body HP is still a completed field boundary.  Its
            // following sibling RS starts a fresh region rather than
            // attaching to the preceding prose line.
            append(output, "\n", maximum)?;
        }
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            for child in body.children() {
                render_terminal_node(child, format, limits, body_indentation, output, maximum)?;
            }
        }
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Element && node.macro_name() == Some("ti") {
        let target = node
            .children()
            .find_map(NodeRef::text)
            .and_then(|value| terminal_temporary_indent_target(value, indentation));
        if let Some(target) = target {
            append_terminal_temporary_indent(output, target, maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Element && matches!(node.macro_name(), Some("EX" | "EE")) {
        // `pre_literal()` always starts a terminal line and never prints the
        // request's recovered arguments. The surrounding no-fill text nodes
        // retain their own physical line boundaries.
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Element && matches!(node.macro_name(), Some("nf" | "fi")) {
        // `nf` and `fi` are terminal line controls even though their public
        // AST elements contain no printable payload. Consecutive controls do
        // not create a blank line, but a transition after visible flow flushes
        // that flow before the following fill mode writes its first word.
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Element
        && matches!(node.macro_name(), Some("ft" | "po" | "ll" | "in"))
    {
        // `.ft` changes the terminal device's current font and `.po` changes
        // its page offset, `.ll` changes its line length, and `.in` changes
        // its physical field. Their
        // compatible AST children are request arguments, not printable prose;
        // subsequent text reconstructs each state from prior requests.
        // The terminal device also completes the preceding physical field
        // before a standalone indentation update; otherwise the new absolute
        // column could not take effect until a later paragraph boundary.
        if node.macro_name() == Some("in") && !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    match node.kind() {
        NodeKind::Comment => {}
        NodeKind::Text => {
            render_terminal_text_node(node, format, limits, indentation, output, maximum, false)?;
        }
        NodeKind::Element if node.macro_name() == Some("Nm") => {
            let mut name = String::new();
            // Nm establishes a bold base font, but its children can switch
            // to italic/roman with `\\f` and later restore the base. Applying
            // bold after generic collection would overstrike an already
            // styled fragment a second time.
            collect_terminal_semantic_text(node, format, limits, TerminalFont::Bold, &mut name);
            append_terminal_text(
                output,
                &name,
                TerminalTextLayout {
                    // Like other mdoc inline macros, Nm's physical request
                    // line remains ordinary filled prose.
                    line_start: false,
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
        }
        NodeKind::Element if node.macro_name() == Some("Xr") => {
            if let Some(reference) = terminal_cross_reference(node, format, limits) {
                append_terminal_text(
                    output,
                    &reference,
                    TerminalTextLayout::default(),
                    indentation,
                    maximum,
                )?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Lk") => {
            if let Some(link) = terminal_link(node, format, limits) {
                append_terminal_text(
                    output,
                    &link,
                    TerminalTextLayout::default(),
                    indentation,
                    maximum,
                )?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Db") => {
            // `Db` is an obsolete debugging request.  Its syntax remains in
            // the compatible AST (and emits a parser diagnostic), but the
            // terminal device's `termp_skip_pre()` suppresses both it and
            // its recovered arguments.
        }
        NodeKind::Element if node.macro_name() == Some("Lb") => {
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
            // Library macros are ordinary inline content outside LIBRARY.
            // Inside that conventional section, a request that begins a
            // physical source line completes its device line after rendering.
            if node.flags().line_start
                && terminal_mdoc_section_named(node, "LIBRARY")
                && !output.ends_with('\n')
            {
                append(output, "\n", maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Fn") => {
            render_terminal_mdoc_function_element(
                node,
                format,
                limits,
                indentation,
                output,
                maximum,
            )?;
        }
        NodeKind::Element if node.macro_name() == Some("Fd") => {
            render_terminal_mdoc_include_declaration(
                node,
                format,
                limits,
                indentation,
                output,
                maximum,
            )?;
        }
        NodeKind::Element if node.macro_name() == Some("In") => {
            render_terminal_mdoc_include_file(node, format, limits, indentation, output, maximum)?;
        }
        NodeKind::Element if node.macro_name() == Some("Ns") => {
            // `Ns` only removes a separator when it occurs in the middle of
            // a physical macro line. At its own line start, term.c leaves
            // the following phrase's ordinary separation intact.
            if !node.flags().line_start {
                append(output, &TERMINAL_ATTACH_NEXT_MARKER.to_string(), maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("No") => {
            // A line-start normal-text macro follows a preceding source-line
            // delimiter, not the delimiter's syntactic argument. Restore
            // the ordinary device separator before descending into its
            // Roman/no-hyphen text children.
            if node.flags().line_start
                && !node
                    .ancestors()
                    .any(|ancestor| ancestor.macro_name() == Some("Eo"))
                && output.ends_with([TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
            {
                mark_terminal_force_separator(output, maximum)?;
            }
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        NodeKind::Element if terminal_mdoc_system_macro(node.macro_name()) => {
            // A system-name macro and its optional version form one device
            // word.  Keeping the generated name and following version
            // together lets the width pass break before `OpenBSD 6.1`, not
            // between those two source arguments.
            let mut system = String::new();
            collect_terminal_text(node, format, limits, &mut system);
            append_terminal_text(
                output,
                &system.replace(' ', &TERMINAL_NONBREAKING_SPACE_MARKER.to_string()),
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        NodeKind::Block if node.macro_name() == Some("Bk") => {
            if let Some(phrase) = terminal_mdoc_system_word_keep(node, format, limits) {
                append_terminal_text(
                    output,
                    &phrase,
                    TerminalTextLayout::default(),
                    indentation,
                    maximum,
                )?;
            } else if let Some(phrase) = terminal_mdoc_word_keep(node, format, limits) {
                // A synopsis Bk leaves the first line in its enclosing field
                // but gives a wrapped kept phrase the device's ten-cell
                // continuation field. Prose keeps retain the paragraph's
                // ordinary wrap field.
                if terminal_mdoc_synopsis(node) {
                    mark_terminal_hanging_indent(
                        output,
                        terminal_mdoc_bk_continuation_indent(node, format, limits, indentation),
                    );
                }
                append_terminal_text(
                    output,
                    &phrase,
                    TerminalTextLayout::default(),
                    indentation,
                    maximum,
                )?;
            } else {
                for child in node.children() {
                    render_terminal_node(child, format, limits, indentation, output, maximum)?;
                }
            }
        }
        NodeKind::Block if node.macro_name() == Some("Nm") && terminal_mdoc_synopsis(node) => {
            // `termp_nm_pre()` enters synopsis layout from the Nm block's
            // Head.  Consequently consecutive name declarations are
            // distinct device lines even though their source blocks contain
            // only otherwise-inline text.
            terminal_mdoc_synopsis_spacing(node, output, maximum)?;
            for child in node.children() {
                if child.kind() == NodeKind::Head {
                    let mut name = String::new();
                    collect_terminal_mdoc_synopsis_name_head(child, format, limits, &mut name);
                    append_terminal_text(
                        output,
                        &name,
                        TerminalTextLayout::default(),
                        indentation,
                        maximum,
                    )?;
                    // A long implicit synopsis name establishes its Body's
                    // field one cell past the complete name, even when the
                    // name itself has already wrapped at the device margin.
                    // Short names keep the ordinary synopsis field; their
                    // option blocks have distinct mdoc layout semantics.
                    let name_width = display_width(&name);
                    if name_width > 70
                        && node.children().any(|part| {
                            part.kind() == NodeKind::Body
                                && part.children().any(|nested| !nested.flags().no_print)
                        })
                    {
                        mark_terminal_hanging_indent(
                            output,
                            indentation.saturating_add(name_width).saturating_add(1),
                        );
                    }
                } else {
                    // A synopsis name followed directly by optional forms
                    // owns the conventional nine-column continuation field.
                    // The field is independent of the name's visible width:
                    // the terminal moves a whole later option there when the
                    // current declaration line is full.
                    if child.kind() == NodeKind::Body && terminal_mdoc_synopsis_option_body(child) {
                        mark_terminal_hanging_indent(output, indentation.saturating_add(4));
                    }
                    render_terminal_node(child, format, limits, indentation, output, maximum)?;
                }
            }
        }
        NodeKind::Block if node.macro_name() == Some("Vt") && terminal_mdoc_synopsis(node) => {
            // In SYNOPSIS each variable declaration owns one device line;
            // the same macro remains an inline italic phrase in prose.
            if !output.is_empty() && !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Ap") => {
            // `Ap` is an apostrophe punctuation macro.  Its optional child
            // belongs to the preceding word (for example `Ingo Ap s`), so
            // retain the same next-token attachment state used by `Ns`.
            append_terminal_text(
                output,
                "'",
                TerminalTextLayout {
                    join: TerminalJoin::Attach,
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
            append(output, &TERMINAL_ATTACH_NEXT_MARKER.to_string(), maximum)?;
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Pf") => {
            // `Pf` presents its one literal argument as a prefix for the
            // next visible token on the same source line.  In particular,
            // the prefix need not itself be parsed punctuation: `.Pf pre
            // fixed` becomes `prefixed`, while `.Pf . right` becomes
            // `.right`.  An incomplete prefix must not capture a later
            // physical source line.
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
            if terminal_mdoc_prefix_attaches_to_following_token(node) {
                mark_terminal_attach_next(output, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("OP") => {
            append_terminal_text(
                output,
                &terminal_man_option(node, format, limits),
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        NodeKind::Element if terminal_man_alternating_fonts(node.macro_name()).is_some() => {
            let fonts = terminal_man_alternating_fonts(node.macro_name()).expect("guarded above");
            let mut contents = String::new();
            for (index, child) in node.children().enumerate() {
                let mut fragment = String::new();
                collect_terminal_semantic_text(
                    child,
                    format,
                    limits,
                    fonts[index % fonts.len()],
                    &mut fragment,
                );
                contents.push_str(&fragment);
            }
            let no_fill = node.flags().no_fill;
            append_terminal_text(
                output,
                &contents,
                TerminalTextLayout {
                    // man(7)'s alternating font requests deliberately join
                    // consecutive arguments without an inter-word device
                    // space; the styled child fragments retain their own
                    // formatter escapes.
                    line_start: no_fill && node.flags().line_start,
                    no_fill,
                    keep_spacing: contents.contains('\t'),
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
        }
        NodeKind::Element if node.macro_name() == Some("B") => {
            let mut bold = String::new();
            collect_terminal_inline_text(node, format, limits, &mut bold);
            let no_fill = node.flags().no_fill;
            append_terminal_text(
                output,
                &render_terminal_bold(&bold, format),
                TerminalTextLayout {
                    // Font macros are inline even when their request begins
                    // a new source line; paragraph and display requests own
                    // terminal physical boundaries.
                    line_start: no_fill && node.flags().line_start,
                    no_fill,
                    keep_spacing: bold.contains('\t'),
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
        }
        NodeKind::Element if node.macro_name() == Some("I") => {
            let mut italic = String::new();
            collect_terminal_semantic_text(node, format, limits, TerminalFont::Italic, &mut italic);
            let no_fill = node.flags().no_fill;
            append_terminal_text(
                output,
                &italic,
                TerminalTextLayout {
                    // Font macros remain inline in filled prose, but a
                    // literal source-line request retains its field start.
                    line_start: no_fill && node.flags().line_start,
                    no_fill,
                    keep_spacing: italic.contains('\t'),
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
        }
        NodeKind::Element if node.macro_name() == Some("An") => {
            // `An -split` and `An -nosplit` are terminal-device state: the
            // directive itself (including validator-retained excess words)
            // does not print.  A following ordinary `An` begins its own
            // physical line in split mode.  The state is scoped to the
            // current mdoc body, where the parser publishes the resolved
            // option on its directive node.
            if node.author_mode().is_some() {
                return Ok(());
            }
            if terminal_author_mode(node) == AuthorMode::Split
                && terminal_author_starts_line(node)
                && !output.is_empty()
                && !output.ends_with('\n')
            {
                append(output, "\n", maximum)?;
            }
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Ft") && terminal_mdoc_synopsis(node) => {
            terminal_mdoc_synopsis_spacing(node, output, maximum)?;
            let mut contents = String::new();
            collect_terminal_semantic_text(
                node,
                format,
                limits,
                TerminalFont::Italic,
                &mut contents,
            );
            append_terminal_text(
                output,
                &contents,
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        NodeKind::Element if terminal_mdoc_element_font(node).is_some() => {
            let mut contents = String::new();
            let font = terminal_mdoc_element_font(node).expect("guarded above");
            let trailing_open_delimiter = node
                .children()
                .next_back()
                .is_some_and(|child| child.flags().delimiter_open);
            collect_terminal_semantic_text(node, format, limits, font, &mut contents);
            let empty_flag = node.macro_name() == Some("Fl") && node.children().next().is_none();
            if node.macro_name() == Some("Fl")
                && (contents.is_empty() || node.children().next().is_some())
            {
                // `.Fl` owns its leading dash. An authored escaped hyphen
                // is its argument, so `Fl \\-long` intentionally renders as
                // the GNU-style `--long` rather than suppressing the macro's
                // own prefix after escape expansion.
                contents.insert_str(0, &render_terminal_font("-", font));
            }
            if terminal_mdoc_long_name_field(node, format, limits) {
                contents = contents.replace(' ', &TERMINAL_NONBREAKING_SPACE_MARKER.to_string());
            }
            if node.flags().line_start
                && output.ends_with([TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
            {
                mark_terminal_force_separator(output, maximum)?;
            }
            append_terminal_text(
                output,
                &contents,
                TerminalTextLayout {
                    // mdoc inline macros do not turn their physical source
                    // line into a terminal boundary. Structural requests
                    // (sections, displays, `br`, and paragraphs) have
                    // already produced one when required.
                    line_start: false,
                    literal_punctuation: terminal_mdoc_inline_punctuation_is_literal(node),
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
            if empty_flag && terminal_mdoc_empty_fl_attaches_to_following_macro(node) {
                mark_terminal_attach_next(output, maximum)?;
            }
            if trailing_open_delimiter {
                // A delimiter at the end of one semantic macro does not
                // pull the first argument of a later macro across that
                // macro boundary. Preserve the following ordinary space
                // without leaking layout state into the public AST.
                mark_terminal_force_separator(output, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Nd") => {
            let mut description = String::new();
            collect_terminal_text(node, format, limits, &mut description);
            if !description.is_empty() {
                if !output.is_empty() && !output.ends_with([' ', '\n']) {
                    append(output, " ", maximum)?;
                }
                append(output, "- ", maximum)?;
                append(output, &description, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("PD") => {
            // Paragraph density is a presentation request. Its scoped value
            // is queried by following PP blocks, not emitted as prose.
        }
        NodeKind::Element if matches!(node.macro_name(), Some("Ex" | "Rv")) => {
            // The standard exit/return-value expansions begin a fresh device
            // line below a preceding label such as `one argument:`. Their
            // generated phrases remain ordinary wrapped prose afterwards.
            if !output.is_empty() && !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Pp") => {
            append_terminal_following_vertical_slot(node, output, maximum)?;
            append_blank_line(output, maximum)?;
            if terminal_mdoc_synopsis_name_paragraph(node)
                && terminal_next_visible_sibling(node)
                    .is_none_or(|next| next.macro_name() != Some("Nm"))
            {
                // In a synopsis-pretty mdoc scope, `Pp` starts the next
                // declaration phrase below the preceding `Nm` field. The
                // public node only carries the pretty flag; preserve the
                // device's twelve-column continuation privately until the
                // final width pass.
                append_terminal_temporary_indent(output, indentation.saturating_add(7), maximum)?;
            }
        }
        NodeKind::Element if matches!(node.macro_name(), Some("PP" | "LP")) => {
            append_blank_line(output, maximum)?;
        }
        NodeKind::Element if node.macro_name() == Some("sp") => {
            // A boxed tbl's trailing device border already occupies one
            // vertical slot (two for `doublebox`).  Its first following
            // positive `.sp` consumes exactly those border slots before
            // requesting any additional blank lines.  Borderless tables do
            // not manufacture a slot, so a following `.sp` remains visible.
            // Negative requests keep their independent deferred semantics.
            let table_slots = take_terminal_table_vertical_skips(output);
            let span = node
                .children()
                .find_map(NodeRef::text)
                .and_then(terminal_vertical_span)
                .unwrap_or(1);
            let span = if span.is_positive() {
                span.saturating_sub(isize::try_from(table_slots).unwrap_or(isize::MAX))
            } else {
                span
            };
            append_terminal_vertical_space(output, span, maximum)?;
        }
        NodeKind::Element if node.macro_name() == Some("br") => {
            // A stray man `.RE` is recovered as a line-breaking `br` beside
            // the field it tried to close.  `post_IP()` has already left its
            // ordinary paragraph slot in the output, whereas term.c lets the
            // recovered close resume directly on the following device line.
            // Real `.br` requests remain below the active field Body, so the
            // sibling relationship is the required narrow discriminator.
            if terminal_man_field_sibling_break(node) && output.ends_with("\n\n") {
                output.pop();
            }
            if !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
        }
        NodeKind::Equation => {
            if let Some(value) = node.equation() {
                // Equation lowering uses the same portable special-character
                // spellings as text nodes (for example `\\[*a]`).  They are
                // deliberately retained in the public AST, but the terminal
                // devices resolve them to their glyph (or ASCII fallback)
                // before the normal line-wrapping pass.
                let rendered = node
                    .equation_terminal()
                    .map(|equation| render_terminal_equation(equation, format, limits))
                    .filter(|rendered| !rendered.is_empty())
                    .unwrap_or_else(|| render_terminal_equation_text(value, format, limits));
                append_terminal_text(
                    output,
                    &rendered,
                    TerminalTextLayout::default(),
                    indentation,
                    maximum,
                )?;
            }
        }
        NodeKind::Table => {
            render_terminal_table(node, format, limits, indentation, output, maximum)?;
        }
        _ => {
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
    }
    Ok(())
}

/// Render a text node, optionally retaining an IP field's no-fill first line
/// after the field padding instead of treating its source line as a new
/// terminal line. This is a device-layout override; the public node flags
/// remain untouched.
#[allow(clippy::too_many_arguments)]
fn render_terminal_text_node(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
    inline_no_fill_line_start: bool,
) -> Result<(), RenderError> {
    let Some(text) = node.text() else {
        return Ok(());
    };
    let mut rendered =
        render_terminal_visible_text_with_font(text, format, limits, terminal_text_font(node));
    if node
        .ancestors()
        .any(|ancestor| ancestor.macro_name() == Some("No"))
    {
        rendered = rendered.replace('-', "-\u{19}");
    }
    let source_no_fill = node.flags().no_fill;
    let no_fill = source_no_fill && node.flags().line_start;
    let inline_conditional_body = node.terminal_inline_conditional();
    if inline_conditional_body && rendered.starts_with(' ') {
        // The ordinary fill separator belongs to the preceding body node;
        // this leading blank is an additional authored cell after `\}`. Keep
        // it with its suffix while still allowing later prose to wrap.
        rendered.replace_range(
            ..' '.len_utf8(),
            &TERMINAL_NONBREAKING_SPACE_MARKER.to_string(),
        );
    }
    if !no_fill && rendered.contains("  ") {
        rendered = terminal_internal_spaces_to_nonbreaking(&rendered);
    }
    let indentation = terminal_text_indentation(node, indentation);
    // The terminal device preserves a no-fill line's word and tab layout,
    // but still discards trailing source whitespace. In particular, an empty
    // macro argument must not leave one visible blank after a final colon.
    let rendered = if no_fill {
        rendered.trim_end()
    } else {
        rendered.as_str()
    };
    // The public AST intentionally normalizes ordinary argument separation,
    // but the arena retains its width for package restructuring. The terminal
    // device observes a run of adjacent spaces, including `\\ ` escapes
    // normalized inside one visible text node, so preserve it before the
    // final prose reflow would collapse it.
    let separator_width = node.separator_width() as usize;
    let preserve_spacing = separator_width > 1;
    let keep_spacing = rendered.contains('\t') || preserve_spacing;
    let literal_tabs = no_fill && node.ancestors().any(NodeRef::literal_display);
    // A detached mdoc punctuation token can be syntactically adjacent to the
    // following inline macro. Only the parser's sentence flag distinguishes
    // `Cd . z` from an actual prose sentence boundary.
    let detached_mdoc_punctuation = matches!(text, "." | "!" | "?")
        && !node.flags().sentence_end
        && node
            .ancestors()
            .any(|ancestor| matches!(ancestor.macro_name(), Some("Sh" | "Ss")));
    mark_terminal_line_length(output, terminal_line_length_before(node), maximum)?;
    append_terminal_text(
        output,
        rendered,
        TerminalTextLayout {
            // mdoc source newlines normally remain fillable whitespace. Man
            // likewise fills ordinary source lines; only no-fill text and a
            // leading tab field/source space retain a physical boundary.
            line_start: !inline_no_fill_line_start
                && node.flags().line_start
                && !inline_conditional_body
                && (no_fill || rendered.starts_with(['\t', ' '])),
            join: if node.flags().delimiter_close {
                TerminalJoin::Attach
            } else {
                TerminalJoin::Separate
            },
            no_fill: no_fill && !rendered.trim().is_empty(),
            no_fill_continuation: source_no_fill && !node.flags().line_start,
            keep_spacing,
            // A plain mdoc text node retains the same terminal sentence
            // boundary as prose in a man paragraph. Semantic mdoc macros
            // retain their distinct delimiter and inline-spacing rules.
            sentence_end: node.flags().sentence_end
                && terminal_sentence_terminator(rendered)
                && !no_fill
                && (node
                    .ancestors()
                    .any(|ancestor| matches!(ancestor.macro_name(), Some("SH" | "SS")))
                    || terminal_mdoc_plain_text_sentence(node)),
            literal_punctuation: node
                .ancestors()
                .any(terminal_mdoc_inline_punctuation_is_literal)
                || detached_mdoc_punctuation,
            tabs: if literal_tabs {
                TerminalTabLayout::PhysicalLiteral
            } else {
                TerminalTabLayout::Relative
            },
        },
        indentation,
        maximum,
    )?;
    if separator_width > 1 {
        append(output, &" ".repeat(separator_width - 1), maximum)?;
    }
    if node.flags().delimiter_open {
        append(output, &TERMINAL_ATTACH_NEXT_MARKER.to_string(), maximum)?;
    }
    if node.flags().line_continuation && !text.ends_with("\\z\\c") {
        // `\c` is already normalized out of the public text while its
        // scanner flag records that the next physical source phrase loses
        // the usual fill/no-fill boundary and separator. A trailing `\c`
        // *inside* `\z` remains the zero-width operand rather than a
        // physical-line continuation.
        append(output, &TERMINAL_ATTACH_NEXT_MARKER.to_string(), maximum)?;
    }
    Ok(())
}

fn terminal_quote_delimiters(
    node: NodeRef<'_>,
    body: Option<NodeRef<'_>>,
    format: RenderFormat,
) -> Option<(&'static str, &'static str)> {
    match node.macro_name() {
        Some("Ao" | "Aq") if body.is_some_and(terminal_quote_is_mail_target) => Some(("<", ">")),
        Some("Ao" | "Aq") if matches!(format, RenderFormat::Utf8) => Some(("⟨", "⟩")),
        Some("Ao" | "Aq") => Some(("<", ">")),
        Some("Bo" | "Bq" | "Oo" | "Op") => Some(("[", "]")),
        Some("Bro" | "Brq") => Some(("{", "}")),
        Some("Do" | "Dq") if matches!(format, RenderFormat::Utf8) => Some(("“", "”")),
        Some("Do" | "Dq" | "Qo" | "Qq") => Some(("\"", "\"")),
        Some("Po" | "Pq") => Some(("(", ")")),
        Some("Ql" | "So" | "Sq") if matches!(format, RenderFormat::Utf8) => Some(("‘", "’")),
        Some("Ql" | "So" | "Sq") => Some(("`", "'")),
        _ => None,
    }
}

fn terminal_quote_is_mail_target(body: NodeRef<'_>) -> bool {
    let mut children = body.children();
    children
        .next()
        .is_some_and(|child| child.macro_name() == Some("Mt"))
        && children.next().is_none()
}

/// A recovered explicit closer is represented by an empty Body bearing the
/// opening macro's name.  It must be emitted where it appears in source order
/// rather than deferred to the outer Block's normal terminal post-hook.
fn terminal_embedded_quote_closing(
    node: NodeRef<'_>,
    format: RenderFormat,
) -> Option<&'static str> {
    (node.kind() == NodeKind::Body && node.children().next().is_none())
        .then(|| terminal_quote_delimiters(node, None, format))
        .flatten()
        .map(|(_, closing)| closing)
}

fn terminal_quote_has_embedded_closer(body: NodeRef<'_>, macro_name: Option<&str>) -> bool {
    body.children().any(|child| {
        (child.kind() == NodeKind::Body
            && child.macro_name() == macro_name
            && child.children().next().is_none())
            || terminal_quote_has_embedded_closer(child, macro_name)
    })
}

fn terminal_quote_body_contains_display(body: NodeRef<'_>) -> bool {
    body.children().any(|child| {
        child.kind() == NodeKind::Block && matches!(child.macro_name(), Some("Bd" | "Bl"))
    })
}

/// Render an explicit enclosure whose Body contains a vertical layout block.
/// Flattening a display or list would erase its terminal field boundaries;
/// walking it structurally retains them while a recovered empty quote Body
/// still closes at its authored source position.
fn render_terminal_quote_with_display(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let Some((opening, closing)) = terminal_quote_delimiters(node, Some(body), format) else {
        return Ok(());
    };
    let mut leading = String::new();
    for head in node
        .children()
        .filter(|child| child.kind() == NodeKind::Head || child.flags().delimiter_open)
    {
        collect_terminal_text(head, format, limits, &mut leading);
    }
    let opening = render_terminal_font(opening, terminal_inherited_font(node));
    append_terminal_text(
        output,
        &format!("{leading}{opening}"),
        TerminalTextLayout::default(),
        indentation,
        maximum,
    )?;
    append(output, &TERMINAL_ATTACH_NEXT_MARKER.to_string(), maximum)?;
    for child in body.children() {
        render_terminal_node(child, format, limits, indentation, output, maximum)?;
    }
    if !terminal_quote_has_embedded_closer(body, node.macro_name()) {
        append_terminal_text(
            output,
            &render_terminal_font(closing, terminal_inherited_font(node)),
            TerminalTextLayout {
                join: TerminalJoin::Attach,
                ..TerminalTextLayout::default()
            },
            indentation,
            maximum,
        )?;
    }
    for tail in node
        .children()
        .filter(|child| child.kind() == NodeKind::Tail || child.flags().delimiter_close)
    {
        render_terminal_node(tail, format, limits, indentation, output, maximum)?;
    }
    Ok(())
}

/// An `Ed` that terminates while an explicit quote is still open is retained
/// as an empty `Bd` Body below that quote.  The next phrase resumes at the
/// display's enclosing field, not its display offset.
fn terminal_embedded_display_closing_indentation(
    node: NodeRef<'_>,
    current_indentation: usize,
) -> Option<usize> {
    if !terminal_is_embedded_display_closer(node) {
        return None;
    }
    let display = node.ancestors().find(|ancestor| {
        ancestor.kind() == NodeKind::Block && ancestor.macro_name() == Some("Bd")
    })?;
    let offset = terminal_mdoc_display_offset(display);
    Some(if offset.is_negative() {
        current_indentation.saturating_add(offset.unsigned_abs())
    } else {
        current_indentation.saturating_sub(offset.unsigned_abs())
    })
}

fn terminal_is_embedded_display_closer(node: NodeRef<'_>) -> bool {
    node.kind() == NodeKind::Body
        && node.macro_name() == Some("Bd")
        && node.children().next().is_none()
}

fn terminal_embedded_display_closes_quote(node: NodeRef<'_>) -> bool {
    terminal_is_embedded_display_closer(node)
        && node.parent().is_some_and(|body| {
            body.kind() == NodeKind::Body
                && body.parent().is_some_and(|block| {
                    block.kind() == NodeKind::Block
                        && terminal_quote_delimiters(block, None, RenderFormat::Ascii).is_some()
                })
        })
}

fn terminal_contains_embedded_display_quote_close(node: NodeRef<'_>) -> bool {
    terminal_embedded_display_closes_quote(node)
        || node
            .children()
            .any(terminal_contains_embedded_display_quote_close)
}

/// Collect an explicit quote Body without flattening a synopsis-pretty `Pp`
/// boundary.  The public AST deliberately exposes the paragraph as an inline
/// mdoc element, while the terminal device starts its next phrase in the
/// name-field continuation column even when an `nS` reset occurs inside the
/// still-open optional enclosure.
fn collect_terminal_quote_contents(
    body: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
) {
    for child in body.children() {
        if child.kind() == NodeKind::Block
            && child.macro_name() == Some("Nm")
            && terminal_mdoc_synopsis(child)
        {
            // `Nm` remains a declaration field even below an open optional
            // enclosure. The generic quote collector would otherwise flatten
            // it into the opener's preceding source line and lose both its
            // bold font and SYNOPSIS column.
            output.push('\n');
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
            output.push_str(&indentation.to_string());
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
            if let Some(head) = child.children().find(|part| part.kind() == NodeKind::Head) {
                collect_terminal_mdoc_synopsis_name_head(head, format, limits, output);
            }
            // Validation can nest the remaining source tail below the Nm
            // Body (for example a `Bk` before an `Oc`). It is still part of
            // the same synopsis declaration field after the bold name.
            for nested_body in child
                .children()
                .filter(|part| part.kind() == NodeKind::Body)
            {
                collect_terminal_text(nested_body, format, limits, output);
            }
        } else if child.macro_name() == Some("Pp") && terminal_mdoc_synopsis_paragraph(child) {
            output.push('\n');
            output.push('\n');
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
            output.push_str(&indentation.saturating_add(7).to_string());
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
        } else if child.macro_name() == Some("br") {
            // A recovered list closer can survive inside an otherwise-open
            // quote Body as a terminal `br` (for example `Bo … El` followed
            // by a stray `It`).  It resets to the enclosing list Body field;
            // flattening it loses that boundary and joins the stray item to
            // the bracket phrase.
            output.push('\n');
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
            output.push_str(&indentation.to_string());
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
        } else if let Some(target) =
            terminal_embedded_display_closing_indentation(child, indentation)
        {
            output.push('\n');
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
            output.push_str(&target.to_string());
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
        } else {
            collect_terminal_text(child, format, limits, output);
        }
    }
}

fn is_section_block(node: NodeRef<'_>) -> bool {
    node.kind() == NodeKind::Block && matches!(node.macro_name(), Some("SH" | "SS" | "Sh" | "Ss"))
}

fn is_mdoc_description_block(node: NodeRef<'_>) -> bool {
    node.kind() == NodeKind::Block && node.macro_name() == Some("Nd")
}

/// Collect an mdoc section heading with its ordinary words bold, while
/// preserving any explicit semantic font macro as an independent device
/// fragment. Rendering the whole collected phrase bold would apply a second
/// overstrike to an `Em`/`Li`/`Sy` child.
fn collect_terminal_mdoc_heading(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    output: &mut String,
) {
    if node.flags().no_print || node.ancestors().any(|ancestor| ancestor.flags().no_print) {
        return;
    }
    if node.kind() == NodeKind::Element
        && let Some(font) = terminal_mdoc_element_font(node)
    {
        let mut phrase = String::new();
        collect_terminal_semantic_text(node, format, limits, font, &mut phrase);
        if !phrase.is_empty() {
            terminal_append_heading_separator(output, &phrase);
            output.push_str(&phrase);
        }
        return;
    }
    if let Some(text) = node.text() {
        let phrase = render_terminal_bold(
            &render_terminal_visible_text_with_font(text, format, limits, TerminalFont::Roman),
            format,
        );
        terminal_append_heading_separator(output, &phrase);
        output.push_str(&phrase);
    }
    for child in node.children() {
        collect_terminal_mdoc_heading(child, format, limits, output);
    }
}

fn terminal_append_heading_separator(output: &mut String, phrase: &str) {
    if !output.is_empty()
        && !output.ends_with([' ', '(', '[', '{', '<'])
        && !phrase.starts_with([')', ']', '}', '>', ',', '.', ';', ':', '!', '?'])
    {
        output.push(' ');
    }
}

/// Empty mdoc sections are a terminal heading transition rather than a full
/// vertical region.  The next heading follows on the immediately next line;
/// its ordinary section gap would incorrectly introduce a blank line.
fn terminal_previous_empty_section(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let previous = parent
        .children()
        .take_while(|child| child.id() != node.id())
        .last();
    previous.is_some_and(|previous| {
        is_section_block(previous)
            && previous
                .children()
                .find(|child| child.kind() == NodeKind::Body)
                .is_some_and(|body| {
                    !terminal_has_visible_text(body, format, limits)
                        && !terminal_has_visible_table(body)
                })
    })
}

fn terminal_has_visible_table(node: NodeRef<'_>) -> bool {
    (node.kind() == NodeKind::Table && !node.table_cells().is_empty())
        || node.children().any(terminal_has_visible_table)
}

fn terminal_man_paragraph_density(node: NodeRef<'_>) -> Option<usize> {
    let mut density = None;
    let mut root = node;
    while let Some(parent) = root.parent() {
        root = parent;
    }
    terminal_last_pd_before(root, node.id(), &mut density);
    density
}

/// Visit source-ordered syntax up to `target`, retaining man(7)'s most recent
/// paragraph-distance request.  The structural pass may attach `PD` to the
/// preceding paragraph Body, a pending next-line Head, or the surrounding
/// Body, so direct-sibling lookup is not sufficient.
fn terminal_last_pd_before(
    node: NodeRef<'_>,
    target: crate::NodeId,
    density: &mut Option<usize>,
) -> bool {
    if node.id() == target {
        return true;
    }
    if node.macro_name() == Some("PD") {
        match terminal_first_text(node) {
            None => *density = Some(1),
            Some(value) => {
                if let Some(value) = terminal_vertical_span(value) {
                    *density = Some(value.max(0).unsigned_abs());
                }
            }
        }
    }
    node.children()
        .any(|child| terminal_last_pd_before(child, target, density))
}

/// Return the first textual scanner argument below a stateful man request.
///
/// Recoverable blocks such as `PD` retain their argument below a Head rather
/// than directly on the Block, while their well-formed Element counterpart
/// can expose text one level sooner.  The terminal state machine consumes the
/// first argument in either shape.
fn terminal_first_text(node: NodeRef<'_>) -> Option<&str> {
    node.text()
        .or_else(|| node.children().find_map(terminal_first_text))
}

fn terminal_has_visible_predecessor(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        .any(|sibling| {
            // `PD` selects future paragraph density and an initial `.sp`
            // has no device effect before any visible field. Neither is a
            // predecessor capable of making a following section-leading
            // `PP` manufacture a vertical gap.
            !matches!(sibling.macro_name(), Some("PD" | "sp")) && !sibling.flags().no_print
        })
}

/// `term_vspace()` is additive, even when a transparent anchor separates the
/// two source requests.  Parser structure can put the preceding `.sp` either
/// beside the next request or at the end of the previous paragraph Body, so
/// recover that device-level predecessor before an mdoc `Pp` or man `PP`
/// asks for its own vertical slot.
fn append_terminal_following_vertical_slot(
    node: NodeRef<'_>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if terminal_follows_vertical_space(node) && output.ends_with("\n\n") {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

fn terminal_follows_vertical_space(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        // `Tg` creates an anchor but has no terminal-device presentation, so
        // it cannot consume an adjacent vertical slot.
        .filter(|sibling| sibling.macro_name() != Some("Tg") && !sibling.flags().no_print)
        .last()
        .is_some_and(terminal_ends_with_vertical_space)
}

fn terminal_ends_with_vertical_space(node: NodeRef<'_>) -> bool {
    if node.macro_name() == Some("sp") {
        return true;
    }
    node.children()
        .rfind(|child| !child.flags().no_print)
        .is_some_and(terminal_ends_with_vertical_space)
}

/// Identify the source blank which man validation consumes before an initial
/// field block below a section heading. The parser retains it only as private
/// terminal provenance so public canonical ASTs stay legacy-compatible.
fn terminal_follows_empty_section_paragraph(node: NodeRef<'_>) -> bool {
    node.terminal_suppressed_leading_blank()
}

fn terminal_man_ip_is_in_rs_body(node: NodeRef<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == NodeKind::Body && parent.macro_name() == Some("RS"))
}

fn terminal_man_rs_follows_empty_hanging_paragraph(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        .last()
        .is_some_and(|previous| {
            previous.kind() == NodeKind::Block
                && previous.macro_name() == Some("HP")
                && previous
                    .children()
                    .find(|child| child.kind() == NodeKind::Body)
                    .is_some_and(|body| body.children().all(|child| child.flags().no_print))
        })
}

/// Whether a recovered line break immediately follows a completed man field.
///
/// Valid `.br` requests occurring inside an `IP`/`TP`/`HP` remain children of
/// that field's Body.  In contrast, a `.RE` with no open `RS` closes the
/// field at the structural layer and becomes this direct sibling.  The latter
/// consumes the field's trailing paragraph slot in the terminal device.
fn terminal_man_field_sibling_break(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if !matches!(parent.macro_name(), Some("SH" | "SS")) {
        return false;
    }
    parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        .last()
        .is_some_and(|previous| {
            previous.kind() == NodeKind::Block
                && matches!(previous.macro_name(), Some("IP" | "TP" | "HP"))
        })
}

/// Recover man(7)'s shared `lmargin` register for field macros.
///
/// `IP`, `TP`, and `HP` all read and (when supplied a valid dimensional
/// argument) update the same terminal register.  A `PP` block and section
/// boundaries reset it to the device default; the latter naturally receives a
/// different Body parent, so this source-order sibling walk only needs the
/// former explicit reset marker.
fn terminal_man_field_width(node: NodeRef<'_>) -> isize {
    if let Some(width) = terminal_man_explicit_field_width(node) {
        return width;
    }
    let Some(parent) = node.parent() else {
        return 7;
    };
    let preceding = parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        .collect::<Vec<_>>();
    for sibling in preceding.into_iter().rev() {
        if sibling.macro_name() == Some("PP") {
            break;
        }
        if matches!(sibling.macro_name(), Some("IP" | "TP" | "HP"))
            && let Some(width) = terminal_man_explicit_field_width(sibling)
        {
            return width;
        }
    }
    7
}

fn terminal_man_explicit_field_width(node: NodeRef<'_>) -> Option<isize> {
    let head = node
        .children()
        .find(|child| child.kind() == NodeKind::Head)?;
    match node.macro_name() {
        // IP has a visible tag before its optional width.
        Some("IP") => head
            .children()
            .nth(1)
            .and_then(NodeRef::text)
            .and_then(terminal_signed_roff_en_prefix),
        // TP and HP take their layout width as the Head's first same-line
        // scanner argument.  A next-line TP term such as `20n` is visible
        // text, not an update to the field register.
        Some("TP" | "HP") => head
            .children()
            .next()
            .filter(|argument| !argument.flags().line_start)
            .and_then(NodeRef::text)
            .and_then(terminal_signed_layout_units),
        _ => None,
    }
}

/// Render mdoc's `Bl -item` form from its `It` bodies.  Unlike definition and
/// tagged lists, the `It` head is syntactic input rather than visible content.
/// Its compact flag controls only the boundary between sibling items.
fn render_terminal_plain_list(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let compact = node.compact();
    let list_indentation = terminal_mdoc_list_indentation(node, indentation);
    if terminal_has_visible_predecessor(node) && !compact {
        append_blank_line(output, maximum)?;
    } else if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    let mut first = true;
    for item in body
        .children()
        .filter(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
    {
        let Some(item_body) = item.children().find(|child| child.kind() == NodeKind::Body) else {
            continue;
        };
        if !first {
            if compact {
                if !output.is_empty() && !output.ends_with('\n') {
                    append(output, "\n", maximum)?;
                }
            } else {
                append_blank_line(output, maximum)?;
            }
        }
        for child in item_body.children() {
            render_terminal_node(child, format, limits, list_indentation, output, maximum)?;
        }
        first = false;
    }
    // A populated item list is a terminal field.  Its following outer-flow
    // sibling therefore begins a new device line even when the final item
    // consists solely of recovery-visible text after a bare `Ta`.
    if !first && !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Render mdoc's `Bl -column` rows as fixed terminal fields.
///
/// `Bl -column` is neither an ordinary hanging list nor a tbl node: each
/// `It` owns one Body per `Ta`-delimited cell, while the list declaration
/// phrases determine the field widths.  Those phrases are private arena
/// provenance because the legacy public AST discards them.  Mandoc leaves four
/// cells between declared fields and appends excess cells directly after the
/// final declared field; keeping the resulting line spacing protected avoids
/// ordinary prose wrapping collapsing the table geometry.
fn render_terminal_column_list(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let declared_widths = node
        .column_widths()
        .map(|declaration| {
            let rendered = render_terminal_visible_text_with_font(
                declaration,
                format,
                limits,
                terminal_inherited_font(node),
            );
            display_width(&rendered)
        })
        .collect::<Vec<_>>();
    let list_indentation = terminal_mdoc_list_indentation(node, indentation);
    if terminal_has_visible_predecessor(node) && !node.compact() {
        append_blank_line(output, maximum)?;
    } else if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    let mut table_precedes_next_item = false;
    for child in body.children() {
        if child.kind() == NodeKind::Table && !child.table_cells().is_empty() {
            // tbl rows are direct Body siblings when they occur between mdoc
            // column-list items.  They must stay structural: flattening them
            // through the column-cell collector erases every generated row.
            render_terminal_table(child, format, limits, list_indentation, output, maximum)?;
            table_precedes_next_item = true;
            continue;
        }
        if child.kind() != NodeKind::Block || child.macro_name() != Some("It") {
            continue;
        }
        if table_precedes_next_item {
            append_blank_line(output, maximum)?;
        }
        let table_rows = child
            .children()
            .filter(|cell| cell.kind() == NodeKind::Body)
            .flat_map(NodeRef::children)
            .filter(|row| row.kind() == NodeKind::Table && !row.table_cells().is_empty())
            .collect::<Vec<_>>();
        if !table_rows.is_empty() {
            // The mdoc parser wraps a tbl range in an otherwise empty `It`
            // when it occurs between ordinary column-list rows.  The public
            // compatible tree keeps that wrapper, but terminal layout must
            // render its Table children as a contiguous tbl range.
            for row in table_rows {
                render_terminal_table(row, format, limits, list_indentation, output, maximum)?;
            }
            table_precedes_next_item = true;
            continue;
        }
        let mut structural_tail = Vec::new();
        let cells = child
            .children()
            .filter(|cell| cell.kind() == NodeKind::Body)
            .map(|cell| {
                let children = cell.children().collect::<Vec<_>>();
                let structural_start = terminal_definition_body_structural_tail_start(&children)
                    .unwrap_or(children.len());
                if structural_tail.is_empty() && structural_start < children.len() {
                    structural_tail.extend_from_slice(&children[structural_start..]);
                }
                let mut text = String::new();
                for child in &children[..structural_start] {
                    collect_terminal_column_cell_text(*child, format, limits, &mut text);
                }
                text
            })
            .collect::<Vec<_>>();
        if cells.iter().all(String::is_empty) {
            continue;
        }
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        append(output, &TERMINAL_KEEP_SPACING_MARKER.to_string(), maximum)?;
        append(output, &" ".repeat(list_indentation), maximum)?;
        for (index, cell) in cells.iter().enumerate() {
            let visible = cell.trim_end();
            append(output, visible, maximum)?;
            if index + 1 < cells.len()
                && let Some(width) = declared_widths.get(index)
            {
                // `term.c` leaves four device cells between up to four
                // columns. Its five-column layout reserves one of those
                // cells for the extra field, leaving a three-cell gap.
                let column_gap = if declared_widths.len() >= 5 { 3 } else { 4 };
                // Compute against the complete next-field target rather
                // than saturating the declaration width first: a source
                // phrase one cell wider than its label still consumes one of
                // the four inter-column cells instead of shifting every
                // following column right.
                let padding = width
                    .saturating_add(column_gap)
                    .saturating_sub(display_width(visible));
                append(output, &" ".repeat(padding), maximum)?;
            }
        }
        append(output, "\n", maximum)?;
        // A column cell can recover into a nested display/list after its
        // visible field text.  The compatible AST deliberately keeps both
        // beneath the same It Body, but treating the structural tail as cell
        // prose flattens its vertical field and loses its display offset.
        // Render it only after committing the row, at the column-list field.
        for tail in &structural_tail {
            render_terminal_node(*tail, format, limits, list_indentation, output, maximum)?;
        }
        // Each empty `Body(Bl)` retained below the structural tail is a
        // scanner-recovered list closer.  The native device finishes that
        // recovered field after the enclosing display has emitted its own
        // source tail, rather than flattening the closer where it appears in
        // the compatibility tree.  Keep those vertical slots cumulative: a
        // pair of nested closers is observably two slots before the following
        // outer section.
        for _ in 0..terminal_recovered_list_closer_count(&structural_tail) {
            if !output.is_empty() {
                append(output, "\n", maximum)?;
            }
        }
        table_precedes_next_item = false;
    }
    Ok(())
}

/// Collect one column-list cell node without discarding a scanner-retained
/// empty phrase. A tab followed by source whitespace can become an empty Text
/// node before a semantic mdoc macro; the terminal still advances one cell
/// before that macro's visible expansion. Ordinary prose deliberately
/// suppresses such placeholders, so keep this narrowly within `Bl -column`
/// layout.
fn collect_terminal_column_cell_text(
    child: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    output: &mut String,
) {
    if child.kind() == NodeKind::Text && child.text() == Some("") {
        output.push(' ');
    } else if child.kind() == NodeKind::Text && child.text() == Some(r"\&") {
        // A zero-width no-break escape at the end of a tab-created cell
        // carries the following physical source phrase in the same cell.
        // `term.c` retains its one-cell field separation there even though
        // the escape itself has no glyph.
        output.push(' ');
    } else {
        collect_terminal_text(child, format, limits, output);
    }
}

fn terminal_recovered_list_closer_count(nodes: &[NodeRef<'_>]) -> usize {
    fn count(node: NodeRef<'_>) -> usize {
        usize::from(
            node.kind() == NodeKind::Body
                && node.macro_name() == Some("Bl")
                && node.children().next().is_none(),
        ) + node.children().map(count).sum::<usize>()
    }

    nodes.iter().copied().map(count).sum()
}

/// Render one contiguous tbl range from its normalized row nodes.
///
/// Preprocessing deliberately exposes each tbl row as a public `Table` node
/// because that is the legacy owned-AST contract.  Terminal layout must still
/// see all adjacent rows before it can choose a column width, so the first row
/// gathers its sibling range and later rows become no-ops.  This keeps the
/// public arena flat while making the renderer's table state local and
/// deterministic.
fn render_terminal_table(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    // tbl's ordinary terminal output leaves three cells between adjacent
    // calculated columns.  The public TableCell span records exactly which
    // columns one payload occupies; allocate any span deficit to its final
    // column, as tblcalc does after the simple single-column pass.
    const TABLE_COLUMN_GAP: usize = 3;
    if terminal_previous_sibling(node).is_some_and(|previous| {
        previous.kind() == NodeKind::Table
            && !node
                .table_terminal()
                .is_some_and(|terminal| terminal.starts_table)
    }) {
        return Ok(());
    }
    let Some(parent) = node.parent() else {
        return Ok(());
    };
    let rows = parent
        .children()
        .skip_while(|sibling| sibling.id() != node.id())
        .enumerate()
        .take_while(|(index, sibling)| {
            sibling.kind() == NodeKind::Table
                && (*index == 0
                    || !sibling
                        .table_terminal()
                        .is_some_and(|terminal| terminal.starts_table))
        })
        .map(|(_, sibling)| sibling)
        .collect::<Vec<_>>();
    if rows
        .iter()
        .filter_map(|row| row.table_terminal())
        .any(table_terminal_has_device_layout)
    {
        return render_terminal_styled_table(&rows, format, limits, indentation, output, maximum);
    }
    if rows.iter().all(|row| row.table_cells().is_empty()) {
        return Ok(());
    }

    let column_count = rows
        .iter()
        .map(|row| {
            row.table_cells()
                .iter()
                .map(|cell| usize::from(cell.column_span.max(1)))
                .sum::<usize>()
        })
        .max()
        .unwrap_or_default();
    if column_count == 0 {
        return Ok(());
    }
    let mut widths = vec![0_usize; column_count];
    for row in &rows {
        let mut column = 0_usize;
        for cell in row.table_cells() {
            let span = usize::from(cell.column_span.max(1));
            let text = cell.text.as_deref().map_or_else(String::new, |text| {
                render_terminal_visible_text(text, format, limits)
            });
            let rendered_width = display_width(text.trim_end());
            if span == 1 && column < widths.len() {
                widths[column] = widths[column].max(rendered_width);
            }
            column = column.saturating_add(span);
        }
    }
    for row in &rows {
        let mut column = 0_usize;
        for cell in row.table_cells() {
            let span = usize::from(cell.column_span.max(1));
            let text = cell.text.as_deref().map_or_else(String::new, |text| {
                render_terminal_visible_text(text, format, limits)
            });
            let rendered_width = display_width(text.trim_end());
            if span > 1 && column < widths.len() {
                let end = column.saturating_add(span).min(widths.len());
                let available = widths[column..end]
                    .iter()
                    .copied()
                    .sum::<usize>()
                    .saturating_add(
                        TABLE_COLUMN_GAP.saturating_mul(end.saturating_sub(column + 1)),
                    );
                if rendered_width > available {
                    let final_column = end.saturating_sub(1);
                    widths[final_column] = widths[final_column]
                        .saturating_add(rendered_width.saturating_sub(available));
                }
            }
            column = column.saturating_add(span);
        }
    }

    if !output.is_empty() {
        if terminal_previous_sibling(node)
            .is_some_and(|previous| previous.kind() == NodeKind::Table)
        {
            // A distinct `.TS` range consumes its predecessor's local
            // vertical-skip marker, then owns an ordinary paragraph gap.
            // Without this boundary, adjacent flat compatibility rows would
            // run together even though their source tables are separate.
            let _ = take_terminal_table_vertical_skip(output);
            append_blank_line(output, maximum)?;
        } else if terminal_table_follows_mdoc_prose(node) {
            // Keep the preceding mdoc phrase on its completed physical line
            // without introducing man-style paragraph vspace.  Any
            // keep-spacing marker remains immediately before this newline so
            // the final terminal width pass consumes it as line provenance.
            if !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
        } else {
            append_blank_line(output, maximum)?;
        }
    }
    for (row_index, row) in rows.iter().enumerate() {
        let mut line = String::new();
        let mut column = 0_usize;
        for cell in row.table_cells() {
            let span = usize::from(cell.column_span.max(1));
            let end = column.saturating_add(span).min(widths.len());
            if end <= column {
                break;
            }
            let field_width = widths[column..end]
                .iter()
                .copied()
                .sum::<usize>()
                .saturating_add(TABLE_COLUMN_GAP.saturating_mul(end.saturating_sub(column + 1)));
            if !cell.vertical_continuation {
                let text = cell.text.as_deref().map_or_else(String::new, |text| {
                    render_terminal_visible_text(text, format, limits)
                });
                let text = text.trim_end();
                let padding = field_width.saturating_sub(display_width(text));
                let leading = match cell.alignment {
                    TableAlignment::Left => 0,
                    TableAlignment::Center => padding / 2,
                    TableAlignment::Right => padding,
                };
                let target = widths[..column]
                    .iter()
                    .copied()
                    .sum::<usize>()
                    .saturating_add(TABLE_COLUMN_GAP.saturating_mul(column));
                if display_width(&line) < target {
                    line.push_str(&" ".repeat(target.saturating_sub(display_width(&line))));
                }
                line.push_str(&" ".repeat(leading));
                line.push_str(text);
            }
            column = column.saturating_add(span);
        }
        if line.trim().is_empty() {
            // A physical empty tbl data row is a device-level blank line only
            // when another row follows it.  tbl discards trailing empty rows;
            // emitting indentation here would leave a visible whitespace-only
            // line instead of the terminal's ordinary empty line.
            if rows
                .iter()
                .skip(row_index + 1)
                .any(|later| !later.table_cells().is_empty())
            {
                append(output, "\n", maximum)?;
            }
            continue;
        }
        append(output, &TERMINAL_KEEP_SPACING_MARKER.to_string(), maximum)?;
        append(output, &" ".repeat(indentation), maximum)?;
        append(output, line.trim_end(), maximum)?;
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Whether retained tbl layout affects terminal presentation beyond the
/// canonical `TableCell` payload.  Ordinary alignment-only rows still use the
/// compact compatibility path above, keeping its already-exact output stable.
fn table_terminal_has_device_layout(row: &TableTerminalRow) -> bool {
    row.outer_border != TableTerminalBorder::None
        || row.all_box
        || row.centered
        || row.horizontal_rule != TableTerminalBorder::None
        || row.cells.iter().any(|cell| {
            cell.before_vertical_rules != 0
                || cell.after_vertical_rules != 0
                || cell.horizontal_rule != TableTerminalBorder::None
                || cell.spacing.is_some()
                || cell.font != TableTerminalFont::Roman
                || cell.width_expanding
        })
}

/// Render tbl's device-only box, rule, font, and spacing metadata.
///
/// The parser keeps this small presentation layer separate from the public
/// owned AST.  It is enough for terminal geometry while allowing engine
/// lowering and canonical AST differential to continue consuming the stable
/// `TableCell` projection alone.
#[allow(clippy::too_many_lines)] // tbl geometry is inherently one stateful pass.
fn render_terminal_styled_table(
    rows: &[NodeRef<'_>],
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    const DEFAULT_GAP: usize = 3;
    let column_count = rows
        .iter()
        .map(|row| {
            row.table_terminal()
                .map_or(0, |terminal| terminal.cells.len())
                .max(
                    row.table_cells()
                        .iter()
                        .map(|cell| usize::from(cell.column_span.max(1)))
                        .sum(),
                )
        })
        .max()
        .unwrap_or_default();
    if column_count == 0 {
        return Ok(());
    }
    let table_right_margin =
        terminal_line_length_value(terminal_line_length_before(rows[0]), DEFAULT_RENDER_WIDTH);
    // tblcalc gives a `T{…T}` cell a bounded default field even before it
    // knows the final table width. An explicit `w` replaces that default.
    // Keep this private to terminal layout: public TableCell text remains one
    // normalized logical value for AST compatibility.
    let default_text_block_width = table_right_margin
        .saturating_add(column_count / 2)
        .checked_div(column_count.saturating_add(1))
        .unwrap_or(1)
        .max(1);

    let mut gaps = vec![DEFAULT_GAP; column_count.saturating_sub(1)];
    for row in rows {
        if let Some(terminal) = row.table_terminal() {
            for (index, cell) in terminal.cells.iter().enumerate().take(gaps.len()) {
                if let Some(spacing) = cell.spacing {
                    gaps[index] = usize::from(spacing);
                }
            }
        }
    }
    let mut widths = vec![0_usize; column_count];
    let mut expanding_columns = vec![false; column_count];
    let mut numeric_before = vec![0_usize; column_count];
    let mut numeric_after = vec![0_usize; column_count];
    let mut numeric_decimal = vec![false; column_count];
    for row in rows {
        let starts = table_terminal_cell_starts(row, column_count);
        let terminal = row.table_terminal();
        for (index, cell) in row.table_cells().iter().enumerate() {
            let Some(&column) = starts.get(index) else {
                break;
            };
            let span = usize::from(cell.column_span.max(1));
            let text = table_terminal_visible_cell_text(cell, terminal, column, format, limits);
            let horizontal_rule = terminal
                .and_then(|terminal| terminal.cells.get(column))
                .is_some_and(|cell| cell.horizontal_rule != TableTerminalBorder::None);
            if horizontal_rule {
                if span == 1 && column < widths.len() {
                    widths[column] = widths[column].max(1);
                }
                continue;
            }
            let width_ignored = terminal
                .and_then(|terminal| terminal.cells.get(column))
                .is_some_and(|cell| cell.width_ignored);
            if width_ignored {
                continue;
            }
            if span == 1 && column < widths.len() {
                if cell.text_block {
                    let field_width = terminal
                        .and_then(|terminal| terminal.cells.get(column))
                        .and_then(|cell| cell.minimum_width)
                        .map_or(default_text_block_width, usize::from)
                        .max(1);
                    let rendered_width = terminal_table_text_block_lines(&text, field_width)
                        .iter()
                        .map(|line| display_width(line))
                        .max()
                        .unwrap_or_default();
                    widths[column] = widths[column].max(rendered_width);
                } else if terminal
                    .and_then(|terminal| terminal.cells.get(column))
                    .is_some_and(|cell| cell.numeric)
                {
                    let (before, after, decimal) = table_terminal_numeric_metrics(text.trim_end());
                    numeric_before[column] = numeric_before[column].max(before);
                    numeric_after[column] = numeric_after[column].max(after);
                    numeric_decimal[column] |= decimal;
                } else {
                    let rendered_width = display_width(text.trim_end());
                    widths[column] = widths[column].max(rendered_width);
                }
            }
        }
    }
    for column in 0..widths.len() {
        if numeric_before[column] > 0 || numeric_decimal[column] {
            widths[column] = widths[column].max(
                numeric_before[column]
                    + usize::from(numeric_decimal[column])
                    + numeric_after[column],
            );
        }
    }
    // tbl's `w` modifier establishes a physical terminal field even when
    // the cell payload is shorter.  It applies before span deficits are
    // distributed, just as the device's column calculation does.
    for row in rows {
        let Some(terminal) = row.table_terminal() else {
            continue;
        };
        for (column, cell) in terminal.cells.iter().enumerate().take(widths.len()) {
            expanding_columns[column] |= cell.width_expanding;
            if let Some(width) = cell.minimum_width {
                widths[column] = widths[column].max(usize::from(width));
            }
        }
    }
    for row in rows {
        let starts = table_terminal_cell_starts(row, column_count);
        let terminal = row.table_terminal();
        for (index, cell) in row.table_cells().iter().enumerate() {
            let Some(&column) = starts.get(index) else {
                break;
            };
            let span = usize::from(cell.column_span.max(1));
            if span <= 1 || column >= widths.len() {
                continue;
            }
            let end = column.saturating_add(span).min(widths.len());
            let text = table_terminal_visible_cell_text(cell, terminal, column, format, limits);
            let available = widths[column..end].iter().copied().sum::<usize>()
                + gaps[column..end.saturating_sub(1)]
                    .iter()
                    .copied()
                    .sum::<usize>();
            let rendered_width = display_width(text.trim_end());
            if rendered_width > available {
                let final_column = end.saturating_sub(1);
                widths[final_column] =
                    widths[final_column].saturating_add(rendered_width.saturating_sub(available));
            }
        }
    }

    let outer = rows
        .iter()
        .filter_map(|row| row.table_terminal().map(|terminal| terminal.outer_border))
        .find(|border| *border != TableTerminalBorder::None)
        .unwrap_or(TableTerminalBorder::None);
    let all_box = rows
        .iter()
        .filter_map(|row| row.table_terminal())
        .any(|terminal| terminal.all_box);
    let centered = rows
        .iter()
        .filter_map(|row| row.table_terminal())
        .any(|terminal| terminal.centered);
    // A vertical rule occurring on any layout row reserves the outer device
    // column for the whole table.  Individual rows may leave that column
    // blank, but their data must not slide left into it.  `term_tbl()` sets
    // this up while calculating the shared tbl grid, before it knows which
    // physical rows will actually paint the rule.
    let has_left_vertical_frame = outer == TableTerminalBorder::None
        && rows
            .iter()
            .filter_map(|row| row.table_terminal())
            .any(|terminal| {
                terminal
                    .cells
                    .first()
                    .is_some_and(|cell| cell.before_vertical_rules != 0)
            });
    let has_right_vertical_frame = outer == TableTerminalBorder::None
        && rows
            .iter()
            .filter_map(|row| row.table_terminal())
            .any(|terminal| {
                terminal
                    .cells
                    .last()
                    .is_some_and(|cell| cell.after_vertical_rules != 0)
            });
    let boundary_layout = rows.iter().find_map(|row| row.table_terminal());
    // tbl centres the whole calculated grid once, not each physical row.  A
    // right or left layout frame contributes a single cell to that grid;
    // `box` and `doublebox` both contribute their two outer cells.  This
    // intentionally differs from the visual rule length, which may include
    // additional intersection glyphs.
    let center_offset = centered.then(|| {
        let right_margin =
            terminal_line_length_value(terminal_line_length_before(rows[0]), DEFAULT_RENDER_WIDTH);
        let outer_width = if outer == TableTerminalBorder::None {
            usize::from(has_left_vertical_frame) + usize::from(has_right_vertical_frame)
        } else {
            2
        };
        let table_width = widths
            .iter()
            .sum::<usize>()
            .saturating_add(gaps.iter().sum::<usize>())
            .saturating_add(outer_width);
        let centered_width = table_width.saturating_sub(usize::from(
            indentation.saturating_add(table_width) > right_margin,
        ));
        if indentation.saturating_add(right_margin) > centered_width {
            indentation
                .saturating_add(right_margin)
                .saturating_sub(centered_width)
                / 2
        } else {
            0
        }
    });
    let bottom_layout = rows
        .iter()
        .rev()
        .find_map(|row| row.table_terminal())
        .or(boundary_layout);
    if outer != TableTerminalBorder::None {
        for width in &mut widths {
            *width = (*width).max(1);
        }
    }
    // tblcalc treats `x` fields as an equal-width partition of the remaining
    // device width; their content-derived widths are only a lower-bound when
    // the fixed fields already overflow the right margin.  The 0.4995 bias is
    // intentional: it reproduces mandoc's historical, left-biased rounding
    // of indivisible cells (and its observable three-column geometry).
    // The geometry is renderer-private: the owned `TableCell` keeps only its
    // compatible text/alignment/span projection.
    let expanding_columns = expanding_columns
        .into_iter()
        .enumerate()
        .filter_map(|(column, expands)| expands.then_some(column))
        .collect::<Vec<_>>();
    if !expanding_columns.is_empty() {
        let frame_width = usize::from(outer != TableTerminalBorder::None) * 2;
        let fixed_width = widths
            .iter()
            .enumerate()
            .filter(|(column, _)| !expanding_columns.contains(column))
            .map(|(_, width)| *width)
            .sum::<usize>()
            .saturating_add(DEFAULT_GAP.saturating_mul(column_count.saturating_sub(1)))
            .saturating_add(frame_width);
        // Table geometry is calculated at its source position, not during
        // the later generic wrapping pass.  Reconstruct the preceding `.ll`
        // register here so `x` never expands a table that already exceeds a
        // temporarily narrowed terminal field.
        let target_width = table_right_margin.saturating_sub(indentation);
        if target_width > fixed_width {
            let available = target_width.saturating_sub(fixed_width);
            let count = expanding_columns.len();
            // Mandoc intentionally carries GNU tbl's five-column rounding
            // quirk.  The exception is observable in the upstream expand
            // fixture and also governs tables with six expandable fields.
            let quirk_position = if count == 5 {
                match available % count + 2 {
                    3 | 4 => Some(available % count + 2),
                    _ => None,
                }
            } else {
                None
            };
            let mut allocated = 0_usize;
            for (position, column) in expanding_columns.into_iter().enumerate() {
                // Equivalent to tblcalc's
                // `(double) available * position / count - allocated +
                // 0.4995`, but kept integral so even pathological source
                // dimensions cannot lose precision before the hard bounds
                // reject their rendered output.
                let numerator = available.saturating_mul(position + 1);
                let cumulative =
                    numerator / count + usize::from((numerator % count).saturating_mul(2) > count);
                let mut width = cumulative.saturating_sub(allocated);
                if quirk_position == Some(position + 1) {
                    width = width.saturating_sub(1);
                }
                widths[column] = width;
                allocated = allocated.saturating_add(width);
            }
        }
    }
    if !output.is_empty() {
        if terminal_previous_sibling(rows[0])
            .is_some_and(|previous| previous.kind() == NodeKind::Table)
        {
            let _ = take_terminal_table_vertical_skip(output);
            append_blank_line(output, maximum)?;
        } else if terminal_table_follows_mdoc_prose(rows[0]) {
            if !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
        } else {
            append_blank_line(output, maximum)?;
        }
    }
    if outer != TableTerminalBorder::None {
        // In ASCII `doublebox`, tbl emits the heavy outer rule first. That
        // first frame ignores the first data layout's internal crossings;
        // the following ordinary box rule carries them. Reusing the layout
        // for both lines incorrectly duplicates a top `+---+` intersection.
        if outer == TableTerminalBorder::Double {
            append_terminal_table_rule(
                &widths,
                &gaps,
                None,
                outer,
                false,
                center_offset,
                indentation,
                output,
                maximum,
            )?;
        }
        append_terminal_table_rule(
            &widths,
            &gaps,
            boundary_layout,
            outer,
            all_box,
            center_offset,
            indentation,
            output,
            maximum,
        )?;
    }
    let mut wrote_content = false;
    for (row_index, row) in rows.iter().enumerate() {
        let terminal = row.table_terminal().cloned().unwrap_or_default();
        if terminal.horizontal_rule != TableTerminalBorder::None
            || (row.table_cells().is_empty()
                && terminal
                    .cells
                    .iter()
                    .any(|cell| cell.horizontal_rule != TableTerminalBorder::None))
        {
            // A full `_`/`=` span sits between physical data rows. Its
            // intersections are selected from the preceding row's layout;
            // only the opening rule has no predecessor and falls back to
            // its own retained layout. This is the same left-hand span
            // context passed as `spp` to upstream `tbl_hrule()`.
            let rule_layout = row_index
                .checked_sub(1)
                .and_then(|previous| rows.get(previous))
                .and_then(|previous| previous.table_terminal())
                .filter(|previous| {
                    previous.cells.iter().any(|cell| {
                        cell.before_vertical_rules != 0 || cell.after_vertical_rules != 0
                    })
                })
                .unwrap_or(&terminal);
            let has_global_vertical_frame =
                rows.iter()
                    .filter_map(|row| row.table_terminal())
                    .any(|layout| {
                        layout.cells.iter().any(|cell| {
                            cell.before_vertical_rules != 0 || cell.after_vertical_rules != 0
                        })
                    });
            let needs_solid_global_rule =
                has_global_vertical_frame
                    && terminal.cells.iter().all(|cell| {
                        cell.before_vertical_rules == 0 && cell.after_vertical_rules == 0
                    })
                    && rule_layout.cells.iter().all(|cell| {
                        cell.before_vertical_rules == 0 && cell.after_vertical_rules == 0
                    });
            let rule_layout = if needs_solid_global_rule {
                boundary_layout.unwrap_or(rule_layout)
            } else {
                rule_layout
            };
            let output_start = output.len();
            append_terminal_table_rule(
                &widths,
                &gaps,
                Some(rule_layout),
                outer,
                all_box,
                center_offset,
                indentation,
                output,
                maximum,
            )?;
            if needs_solid_global_rule {
                let character = if terminal.horizontal_rule == TableTerminalBorder::Double {
                    '='
                } else {
                    '-'
                };
                let rendered = output[output_start..].replace('+', &character.to_string());
                output.replace_range(output_start.., &rendered);
            }
            continue;
        }
        if row.table_cells().is_empty()
            && outer == TableTerminalBorder::None
            && !all_box
            && terminal.horizontal_rule == TableTerminalBorder::None
            && terminal.cells.iter().all(|cell| {
                cell.before_vertical_rules == 0
                    && cell.after_vertical_rules == 0
                    && cell.horizontal_rule == TableTerminalBorder::None
            })
        {
            // A format-only empty data row still advances tbl's selected
            // layout (for example `lb`, `li`, `lb`), but font state has no
            // glyph to emit.  Keep a true blank device line only between
            // content rows; tbl discards one at the end of the table.
            if rows
                .iter()
                .skip(row_index + 1)
                .any(|later| !later.table_cells().is_empty())
            {
                append(output, "\n", maximum)?;
            }
            continue;
        }
        append_terminal_table_content(
            *row,
            &widths,
            &gaps,
            &numeric_before,
            &terminal,
            row_index
                .checked_sub(1)
                .and_then(|previous| rows.get(previous))
                .and_then(|previous| previous.table_terminal()),
            rows.get(row_index + 1)
                .and_then(|next| next.table_terminal()),
            outer,
            all_box,
            has_left_vertical_frame,
            has_right_vertical_frame,
            center_offset,
            format,
            limits,
            indentation,
            output,
            maximum,
        )?;
        wrote_content = true;
        // `allbox` contributes its own boundary before every later content
        // row.  An authored `_`/`=` layout row remains an additional device
        // rule between those rows, but a terminal layout rule already shares
        // the bottom frame and therefore does not need another allbox rule.
        let has_later_content = rows
            .iter()
            .skip(row_index + 1)
            .any(|later| !later.table_cells().is_empty());
        if all_box && has_later_content {
            let next_layout = rows
                .get(row_index + 1)
                .and_then(|next| next.table_terminal());
            let next_manual_rule = next_layout
                .filter(|terminal| terminal.horizontal_rule != TableTerminalBorder::None);
            let next_double_intersection = next_layout.filter(|next| {
                next.cells.windows(2).any(|cells| {
                    cells[0].after_vertical_rules >= 2 || cells[1].before_vertical_rules >= 2
                })
            });
            // `allbox` is drawn between data spans, so an internal double
            // vertical edge on the preceding span meets that rule. Preserve
            // the current-row intersection rather than replacing it with a
            // featureless allbox line.
            let current_double_intersection = terminal.cells.windows(2).any(|cells| {
                cells[0].after_vertical_rules >= 2 || cells[1].before_vertical_rules >= 2
            });
            append_terminal_table_rule(
                &widths,
                &gaps,
                current_double_intersection
                    .then_some(&terminal)
                    .or(next_double_intersection)
                    .or(next_manual_rule),
                outer,
                all_box,
                center_offset,
                indentation,
                output,
                maximum,
            )?;
        }
    }
    if wrote_content && outer != TableTerminalBorder::None {
        append_terminal_table_rule(
            &widths,
            &gaps,
            bottom_layout,
            outer,
            all_box,
            center_offset,
            indentation,
            output,
            maximum,
        )?;
        if outer == TableTerminalBorder::Double {
            append_terminal_table_rule(
                &widths,
                &gaps,
                None,
                outer,
                false,
                center_offset,
                indentation,
                output,
                maximum,
            )?;
        }
    }
    // Ordinary paragraph and footer spacing consumes this one table-local
    // slot. A standalone leading vertical layout line instead owns the
    // following field boundary, so its paragraph keeps the normal blank row.
    // Sections and explicit `.sp` clear the ordinary table-local marker
    // before their own handling.
    let carries_leading_vertical_layout = outer == TableTerminalBorder::None
        && rows.iter().any(|row| {
            row.table_terminal()
                .and_then(|terminal| terminal.cells.first())
                .is_some_and(|cell| cell.before_vertical_rules != 0)
        });
    let carries_layout_horizontal_rule = rows.iter().any(|row| {
        row.table_terminal().is_some_and(|terminal| {
            terminal
                .cells
                .iter()
                .any(|cell| cell.horizontal_rule != TableTerminalBorder::None)
        })
    });
    // `term_tbl()` always records the trailing device slot of a boxed table,
    // including when its final layout row contains a partial horizontal rule.
    // Only borderless layout-only rows have the special ownership rules
    // below; otherwise the following `.sp` would manufacture a second blank
    // after the visible box frame.
    if outer != TableTerminalBorder::None
        || (!carries_leading_vertical_layout && !carries_layout_horizontal_rule)
    {
        let trailing_slots = match outer {
            TableTerminalBorder::None => 0,
            TableTerminalBorder::Single => 1,
            TableTerminalBorder::Double => 2,
        };
        for _ in 0..trailing_slots {
            mark_terminal_table_vertical_skip(output);
        }
    }
    Ok(())
}

/// mdoc's table preprocessor keeps a table directly below the preceding
/// Body phrase.  The man device instead gives a table its ordinary paragraph
/// separator.  The generated table row has no public macro set of its own,
/// but its enclosing section retains the package's exact macro spelling.
fn terminal_table_follows_mdoc_prose(node: NodeRef<'_>) -> bool {
    node.ancestors()
        .any(|ancestor| matches!(ancestor.macro_name(), Some("Sh" | "Ss")))
}

fn table_terminal_cell_starts(row: &NodeRef<'_>, column_count: usize) -> Vec<usize> {
    let mut starts = row
        .table_terminal()
        .map(|terminal| {
            if terminal.data_columns.len() >= row.table_cells().len() {
                return terminal
                    .data_columns
                    .iter()
                    .take(row.table_cells().len())
                    .map(|column| usize::from(*column))
                    .collect();
            }
            terminal
                .cells
                .iter()
                .enumerate()
                .filter_map(|(index, cell)| {
                    (!cell.span && cell.horizontal_rule == TableTerminalBorder::None)
                        .then_some(index)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut next = starts.last().copied().map_or(0, |column| column + 1);
    while starts.len() < row.table_cells().len() && next < column_count {
        starts.push(next);
        next += 1;
    }
    starts
}

fn table_terminal_visible_cell_text(
    cell: &crate::TableCell,
    terminal: Option<&TableTerminalRow>,
    column: usize,
    format: RenderFormat,
    limits: &Limits,
) -> String {
    let text = cell.text.as_deref().map_or_else(String::new, |text| {
        render_terminal_visible_text(text, format, limits)
    });
    match terminal
        .and_then(|terminal| terminal.cells.get(column))
        .map_or(TableTerminalFont::Roman, |cell| cell.font)
    {
        TableTerminalFont::Roman => text,
        TableTerminalFont::Bold => render_terminal_font(&text, TerminalFont::Bold),
        TableTerminalFont::Italic => render_terminal_font(&text, TerminalFont::Italic),
    }
}

/// Wrap one normalized tbl `T{…T}` payload at the field selected by
/// `tblcalc_data()`. Text-block source lines have already been normalized to
/// ordinary spaces by preprocessing, and the C device likewise reflows them
/// at word boundaries without splitting an overwide word.
fn terminal_table_text_block_lines(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let separator = usize::from(!line.is_empty());
        if !line.is_empty()
            && display_width(&line)
                .saturating_add(separator)
                .saturating_add(display_width(word))
                > width
        {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

#[allow(clippy::too_many_arguments)] // A rule shares the table renderer's bounded output context.
fn append_terminal_table_rule(
    widths: &[usize],
    gaps: &[usize],
    terminal: Option<&TableTerminalRow>,
    outer: TableTerminalBorder,
    all_box: bool,
    center_offset: Option<usize>,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let rule = terminal.and_then(|row| {
        (row.horizontal_rule != TableTerminalBorder::None).then_some(row.horizontal_rule)
    });
    let line_character = if rule == Some(TableTerminalBorder::Double) {
        '='
    } else {
        '-'
    };
    let mut line = String::new();
    let leading_rules = if outer == TableTerminalBorder::None {
        terminal
            .and_then(|row| row.cells.first())
            // tbl retains at most one outer edge glyph even when the source
            // layout spells a double `||` boundary. Double rules remain
            // meaningful only between calculated columns.
            .map_or(0, |cell| usize::from(cell.before_vertical_rules.min(1)))
    } else {
        1
    };
    line.push_str(&"+".repeat(leading_rules));
    for column in 0..widths.len() {
        line.push_str(&line_character.to_string().repeat(widths[column]));
        if column + 1 == widths.len() {
            let trailing_rules = if outer == TableTerminalBorder::None {
                terminal
                    .and_then(|row| row.cells.last())
                    .map_or(0, |cell| usize::from(cell.after_vertical_rules.min(1)))
            } else {
                1
            };
            if trailing_rules > 0 {
                line.push(line_character);
                line.push_str(&"+".repeat(trailing_rules));
            }
            // A horizontal span crossing into a standalone leading vertical
            // layout row continues through that row's one-cell terminal
            // boundary.  It is not an outer box edge, hence one extra rule
            // glyph rather than a closing `+`.
            if rule.is_some()
                && outer == TableTerminalBorder::None
                && widths.len() == 1
                && leading_rules != 0
                && trailing_rules == 0
            {
                line.push(line_character);
            }
            continue;
        }
        let (after, before, rules) =
            table_terminal_boundary(terminal, None, None, column, gaps[column], all_box);
        line.push_str(&line_character.to_string().repeat(after));
        if rules == 0 {
            line.push_str(&line_character.to_string().repeat(before));
        } else {
            line.push_str(&"+".repeat(rules));
            line.push_str(&line_character.to_string().repeat(before));
        }
        // A standalone full-width tbl rule owns one final device cell at
        // each participating layout boundary. Partial horizontal layout
        // cells are handled by the data-row geometry instead; applying this
        // extension to the outer box frame would overrun that frame.
        if rule.is_some()
            && terminal.is_some_and(|row| {
                row.cells
                    .get(column)
                    .is_some_and(|cell| cell.horizontal_rule != TableTerminalBorder::None)
                    || row
                        .cells
                        .get(column + 1)
                        .is_some_and(|cell| cell.horizontal_rule != TableTerminalBorder::None)
            })
        {
            line.push(line_character);
        }
    }
    append_terminal_table_line_prefix(output, center_offset, indentation, maximum)?;
    append(output, &line, maximum)?;
    append(output, "\n", maximum)
}

#[allow(clippy::too_many_arguments)] // Content shares the table renderer's bounded output context.
fn append_terminal_table_content(
    row: NodeRef<'_>,
    widths: &[usize],
    gaps: &[usize],
    numeric_before: &[usize],
    terminal: &TableTerminalRow,
    previous_terminal: Option<&TableTerminalRow>,
    next_terminal: Option<&TableTerminalRow>,
    outer: TableTerminalBorder,
    all_box: bool,
    has_left_vertical_frame: bool,
    has_right_vertical_frame: bool,
    center_offset: Option<usize>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let starts = table_terminal_cell_starts(&row, widths.len());
    let mut cells = starts
        .iter()
        .copied()
        .zip(row.table_cells())
        .collect::<Vec<_>>();
    cells.sort_by_key(|(start, _)| *start);
    let text_block_lines = cells
        .iter()
        .filter(|(_, cell)| cell.text_block)
        .map(|(column, cell)| {
            let span = usize::from(cell.column_span.max(1)).min(widths.len() - *column);
            let end = column + span;
            let field_width = widths[*column..end].iter().copied().sum::<usize>()
                + gaps[*column..end.saturating_sub(1)]
                    .iter()
                    .copied()
                    .sum::<usize>();
            let text =
                table_terminal_visible_cell_text(cell, Some(terminal), *column, format, limits);
            terminal_table_text_block_lines(&text, field_width).len()
        })
        .max()
        .unwrap_or(1);
    let leading_horizontal = terminal
        .cells
        .first()
        .map_or(TableTerminalBorder::None, |cell| cell.horizontal_rule);
    let previous_leading_rules = if leading_horizontal == TableTerminalBorder::None {
        0
    } else {
        previous_terminal
            .and_then(|previous| previous.cells.first())
            .map_or(0, |cell| usize::from(cell.before_vertical_rules))
    };
    let leading_rules = if outer == TableTerminalBorder::None {
        terminal
            .cells
            .first()
            .map_or(0, |cell| usize::from(cell.before_vertical_rules))
            .max(
                next_terminal
                    .and_then(|next| next.cells.first())
                    .filter(|cell| !cell.leading_vertical_from_standalone)
                    .map_or(0, |cell| usize::from(cell.before_vertical_rules)),
            )
            .max(previous_leading_rules)
            .min(1)
    } else {
        1
    };
    for text_block_line in 0..text_block_lines {
        let mut line = String::new();
        if leading_rules != 0 {
            // An authored horizontal cell meets an outer vertical device
            // frame at a `+`; without it the frame is simply a `|`.
            line.push(if leading_horizontal == TableTerminalBorder::None {
                '|'
            } else {
                '+'
            });
        } else if has_left_vertical_frame {
            line.push(' ');
        }
        let mut column = 0_usize;
        let mut cell_index = 0_usize;
        while column < widths.len() {
            let (span, alignment, vertical, horizontal_rule, text, text_block) =
                if let Some((start, cell)) = cells.get(cell_index)
                    && *start == column
                {
                    cell_index += 1;
                    let span = usize::from(cell.column_span.max(1)).min(widths.len() - column);
                    (
                        span,
                        cell.alignment,
                        cell.vertical_continuation,
                        terminal
                            .cells
                            .get(column)
                            .map_or(TableTerminalBorder::None, |cell| cell.horizontal_rule),
                        table_terminal_visible_cell_text(
                            cell,
                            Some(terminal),
                            column,
                            format,
                            limits,
                        ),
                        cell.text_block,
                    )
                } else {
                    (
                        1,
                        TableAlignment::Left,
                        false,
                        terminal
                            .cells
                            .get(column)
                            .map_or(TableTerminalBorder::None, |cell| cell.horizontal_rule),
                        String::new(),
                        false,
                    )
                };
            let end = column + span;
            let field_width = widths[column..end].iter().copied().sum::<usize>()
                + gaps[column..end.saturating_sub(1)]
                    .iter()
                    .copied()
                    .sum::<usize>();
            let text = if text_block {
                terminal_table_text_block_lines(&text, field_width)
                    .into_iter()
                    .nth(text_block_line)
                    .unwrap_or_default()
            } else if text_block_line == 0 {
                text
            } else {
                String::new()
            };
            let text = text.trim_end();
            let numeric = !text_block
                && terminal
                    .cells
                    .get(column)
                    .is_some_and(|cell| cell.numeric && !cell.width_ignored);
            let padding = field_width.saturating_sub(display_width(text));
            let leading = if numeric {
                let (before, _, _) = table_terminal_numeric_metrics(text);
                numeric_before
                    .get(column)
                    .copied()
                    .unwrap_or_default()
                    .saturating_sub(before)
            } else {
                match alignment {
                    TableAlignment::Left => 0,
                    TableAlignment::Center => padding / 2,
                    TableAlignment::Right => padding,
                }
            };
            if horizontal_rule != TableTerminalBorder::None {
                let rule_character = if horizontal_rule == TableTerminalBorder::Double {
                    '='
                } else {
                    '-'
                };
                line.push_str(&rule_character.to_string().repeat(field_width));
            } else if vertical {
                line.push_str(&" ".repeat(field_width));
            } else {
                line.push_str(&" ".repeat(leading));
                line.push_str(text);
                line.push_str(&" ".repeat(padding.saturating_sub(leading)));
            }
            if end == widths.len() {
                let previous_trailing_rules = if horizontal_rule == TableTerminalBorder::None {
                    0
                } else {
                    previous_terminal
                        .and_then(|previous| previous.cells.last())
                        .map_or(0, |cell| usize::from(cell.after_vertical_rules))
                };
                let trailing_rules = if outer == TableTerminalBorder::None {
                    terminal
                        .cells
                        .last()
                        .map_or(0, |cell| usize::from(cell.after_vertical_rules))
                        .max(
                            next_terminal
                                .and_then(|next| next.cells.last())
                                .map_or(0, |cell| usize::from(cell.after_vertical_rules)),
                        )
                        .max(previous_trailing_rules)
                        .min(1)
                } else {
                    1
                };
                if horizontal_rule != TableTerminalBorder::None {
                    // The final horizontal layout cell reaches one device
                    // position past its calculated field.  If that position
                    // also carries the outer vertical frame it is the
                    // ordinary ASCII tbl intersection glyph.
                    line.push(
                        table_terminal_rule_character(horizontal_rule)
                            .expect("horizontal rule was checked above"),
                    );
                    if trailing_rules > 0 {
                        line.push('+');
                    }
                } else if trailing_rules > 0 {
                    line.push(' ');
                    line.push('|');
                } else if has_right_vertical_frame {
                    // Preserve the shared grid's right edge even on a row
                    // where no segment of that edge is currently painted.
                    // It is trailing whitespace and will intentionally be
                    // removed below, but makes this branch explicit beside
                    // the analogous leading-frame reservation.
                    line.push(' ');
                }
                break;
            }
            let (after, before, rules) = table_terminal_boundary(
                Some(terminal),
                previous_terminal,
                next_terminal,
                end - 1,
                gaps[end - 1],
                all_box,
            );
            let right_horizontal = terminal
                .cells
                .get(end)
                .map_or(TableTerminalBorder::None, |cell| cell.horizontal_rule);
            append_terminal_table_boundary(
                &mut line,
                after,
                before,
                rules,
                horizontal_rule,
                right_horizontal,
            );
            column = end;
        }
        append_terminal_table_line_prefix(output, center_offset, indentation, maximum)?;
        append(output, line.trim_end(), maximum)?;
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Begin one calculated tbl device line.  Ordinary tables render in their
/// surrounding text field; `center` tables instead use that field's right
/// edge as the centering width, so their source indentation must not become a
/// visible prefix before the final device pass.
fn append_terminal_table_line_prefix(
    output: &mut String,
    center_offset: Option<usize>,
    indentation: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    append(output, &TERMINAL_KEEP_SPACING_MARKER.to_string(), maximum)?;
    append(
        output,
        &" ".repeat(center_offset.unwrap_or(indentation)),
        maximum,
    )
}

fn table_terminal_numeric_metrics(value: &str) -> (usize, usize, bool) {
    let value = value.trim_end();
    let Some((before, after)) = value.rsplit_once('.') else {
        return (display_width(value), 0, false);
    };
    (display_width(before), display_width(after), true)
}

/// Draw one inter-column tbl device field.  A horizontal layout cell owns its
/// adjacent half of the spacing field; a vertical edge in the centre turns
/// the meeting point into `+` rather than replacing the horizontal rule with
/// a bare `|`.  Keeping this distinct from public `TableCell` state mirrors
/// the terminal-only layout graph used by upstream `tbl_term.c`.
fn append_terminal_table_boundary(
    line: &mut String,
    mut after: usize,
    mut before: usize,
    rules: usize,
    left_horizontal: TableTerminalBorder,
    right_horizontal: TableTerminalBorder,
) {
    let left = table_terminal_rule_character(left_horizontal);
    let right = table_terminal_rule_character(right_horizontal);
    // A rule entering from the right starts at the centre of the ordinary
    // three-cell tbl gap, not after it.  Shift that one device position from
    // the left cell's blank half to the rule-owning right cell.  This is the
    // asymmetric `tbl_direct_border()` placement used by the ASCII device.
    if rules == 0 && left.is_none() && right.is_some() && after != 0 {
        after -= 1;
        before += 1;
    }
    if rules == 0 {
        line.extend(std::iter::repeat_n(left.unwrap_or(' '), after));
        line.extend(std::iter::repeat_n(right.unwrap_or(' '), before));
        return;
    }
    line.extend(std::iter::repeat_n(left.unwrap_or(' '), after));
    if left.is_some() || right.is_some() {
        // For a double vertical boundary, the horizontal line arriving from
        // the right crosses both ASCII device columns (`++`).  A line ending
        // on the left crosses only the first one (`+|`).  This directional
        // asymmetry is inherited from groff tbl's two-cell border encoding.
        if right.is_some() {
            line.extend(std::iter::repeat_n('+', rules));
        } else {
            line.push('+');
            line.extend(std::iter::repeat_n('|', rules.saturating_sub(1)));
        }
    } else {
        line.extend(std::iter::repeat_n('|', rules));
    }
    line.extend(std::iter::repeat_n(right.unwrap_or(' '), before));
}

fn table_terminal_rule_character(border: TableTerminalBorder) -> Option<char> {
    match border {
        TableTerminalBorder::None => None,
        TableTerminalBorder::Single => Some('-'),
        TableTerminalBorder::Double => Some('='),
    }
}

fn table_terminal_boundary(
    terminal: Option<&TableTerminalRow>,
    previous_terminal: Option<&TableTerminalRow>,
    next_terminal: Option<&TableTerminalRow>,
    column: usize,
    gap: usize,
    all_box: bool,
) -> (usize, usize, usize) {
    if terminal.is_some_and(|row| row.cells.get(column + 1).is_some_and(|cell| cell.span)) {
        return (gap, 0, 0);
    }
    let current_left_horizontal = terminal
        .and_then(|row| row.cells.get(column))
        .map_or(TableTerminalBorder::None, |cell| cell.horizontal_rule);
    let current_right_horizontal = terminal
        .and_then(|row| row.cells.get(column + 1))
        .map_or(TableTerminalBorder::None, |cell| cell.horizontal_rule);
    let previous_after = if current_left_horizontal == TableTerminalBorder::None {
        0
    } else {
        previous_terminal
            .and_then(|previous| previous.cells.get(column))
            .filter(|cell| cell.horizontal_rule == TableTerminalBorder::None)
            .map_or(0, |cell| usize::from(cell.after_vertical_rules))
    };
    let previous_before = if current_right_horizontal == TableTerminalBorder::None {
        0
    } else {
        previous_terminal.map_or(0, |previous| {
            let after_left = previous
                .cells
                .get(column)
                .filter(|cell| cell.horizontal_rule == TableTerminalBorder::None)
                .map_or(0, |cell| usize::from(cell.after_vertical_rules));
            let before_right = previous
                .cells
                .get(column + 1)
                .filter(|cell| cell.horizontal_rule == TableTerminalBorder::None)
                .map_or(0, |cell| usize::from(cell.before_vertical_rules));
            after_left.max(before_right)
        })
    };
    let after = terminal
        .and_then(|row| row.cells.get(column))
        .map_or(0, |cell| usize::from(cell.after_vertical_rules))
        .max(
            next_terminal
                .and_then(|row| row.cells.get(column))
                .map_or(0, |cell| {
                    usize::from(cell.after_vertical_rules).min(if all_box { 1 } else { usize::MAX })
                }),
        )
        .max(previous_after);
    let before = terminal
        .and_then(|row| row.cells.get(column + 1))
        .map_or(0, |cell| usize::from(cell.before_vertical_rules))
        .max(
            next_terminal
                .and_then(|row| row.cells.get(column + 1))
                .map_or(0, |cell| {
                    usize::from(cell.before_vertical_rules).min(if all_box {
                        1
                    } else {
                        usize::MAX
                    })
                }),
        )
        .max(previous_before);
    let rules = after.max(before).max(usize::from(all_box));
    // ASCII tbl has only one drawable crossing cell in a one- or two-cell
    // inter-column field. A double downward frame gets its second glyph only
    // once the authored spacing supplies the extra device position.
    let rules = if rules == 2 && gap <= 2 { 1 } else { rules };
    let spaces = gap.saturating_sub(rules);
    (spaces.div_ceil(2), spaces / 2, rules)
}

/// Whether an mdoc list has no semantic item Blocks.
fn terminal_mdoc_list_is_empty(node: NodeRef<'_>) -> bool {
    node.children()
        .find(|child| child.kind() == NodeKind::Body)
        .is_none_or(|body| {
            !body
                .children()
                .any(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
        })
}

/// Render mdoc's marker-bearing `Bl` variants without widening the legacy
/// normalized list API. The parser retains their source spelling privately:
/// bullet, dash, and hyphen markers are bold, while enum counts from one.
/// All reserve the terminal device's five-cell marker field.
fn render_terminal_marked_list(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let Some(marker) = node.list_marker() else {
        // A recovery-created list can be normalized without a source
        // selector. Its legacy-compatible fallback is marker-free flow.
        return render_terminal_plain_list(node, format, limits, indentation, output, maximum);
    };
    let compact = node.compact();
    let marker_indentation = terminal_mdoc_list_indentation(node, indentation);
    // `termp_it_pre()` starts marker-list Bodies at the declared width plus
    // groff's two-cell buffer.  Negative and narrow widths still leave one
    // separator after the marker but make wrapped lines outdent accordingly.
    let explicit_body_field_width = node
        .width()
        .and_then(terminal_signed_layout_units)
        .map(|width| width.saturating_add(2));
    let body_field_width = explicit_body_field_width.unwrap_or(5);
    let body_indentation = if body_field_width.is_negative() {
        marker_indentation.saturating_sub(body_field_width.unsigned_abs())
    } else {
        marker_indentation.saturating_add(body_field_width.unsigned_abs())
    };
    if terminal_has_visible_predecessor(node) && !compact {
        append_blank_line(output, maximum)?;
    } else if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    let mut ordinal = 1_usize;
    let mut first = true;
    for item in body
        .children()
        .filter(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
    {
        if !first {
            if compact {
                if !output.is_empty() && !output.ends_with('\n') {
                    append(output, "\n", maximum)?;
                }
            } else {
                append_blank_line(output, maximum)?;
            }
        }
        let visible_marker = match marker {
            // The historical terminal device draws the bullet as a plus and
            // circle overstruck at the same column, not as two separately
            // bold glyphs.  Keep its byte-for-byte backspace sequence.
            MdocListMarker::Bullet => "+\u{8}+\u{8}o\u{8}o".to_owned(),
            MdocListMarker::Dash | MdocListMarker::Hyphen => render_terminal_bold("-", format),
            MdocListMarker::Enum => format!("{ordinal}."),
        };
        append_terminal_hanging_indent(output, body_indentation, maximum)?;
        append_terminal_text(
            output,
            &visible_marker,
            TerminalTextLayout {
                line_start: true,
                // An enum's dot is a terminal list marker, not prose that
                // should request the next word's double sentence spacing.
                literal_punctuation: matches!(marker, MdocListMarker::Enum),
                ..TerminalTextLayout::default()
            },
            marker_indentation,
            maximum,
        )?;
        if let Some(item_body) = item.children().find(|child| child.kind() == NodeKind::Body)
            && item_body.children().any(|child| !child.flags().no_print)
        {
            let leading_list = item_body
                .children()
                .find(|child| !child.flags().no_print)
                .filter(|child| child.macro_name() == Some("Bl"));
            if let Some(list) = leading_list {
                // A marker whose Body opens directly with another list owns
                // its own otherwise-empty device field.  Do not leave the
                // ordinary marker-to-prose padding behind it: a non-compact
                // nested list starts after the field's vertical slot, while
                // a compact one merely starts on the next physical line.
                if list.compact() {
                    append(output, "\n", maximum)?;
                } else {
                    append_blank_line(output, maximum)?;
                }
            } else {
                let field_gap = explicit_body_field_width.map_or(3, |width| {
                    width
                        .saturating_sub_unsigned(display_width(&visible_marker))
                        .max(1)
                        .unsigned_abs()
                });
                // Keep all but the final field separator non-breaking until the
                // width pass.  It can then wrap prose at the Body field without
                // collapsing the marker's explicitly padded first line.
                let protected_padding = TERMINAL_NONBREAKING_SPACE_MARKER
                    .to_string()
                    .repeat(field_gap.saturating_sub(1));
                append(output, &protected_padding, maximum)?;
            }
            for child in item_body.children() {
                render_terminal_node(child, format, limits, body_indentation, output, maximum)?;
            }
        }
        ordinal = ordinal.saturating_add(1);
        first = false;
    }
    if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Render man(7)'s `TP` as a tagged paragraph.  The leading `n`/`i` width is
/// scanner input kept below the public tag, while the following physical line
/// is the visible term.  The body position is relative to its containing
/// section and deliberately accepts negative widths, matching the terminal
/// device's leftward outdent behaviour.
fn render_terminal_man_tagged_paragraph(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(head) = node.children().find(|child| child.kind() == NodeKind::Head) else {
        return Ok(());
    };
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };

    // A bare TP starts at the terminal's seven-cell default. A width applies
    // only when it is on this TP request's source line, so it remains below
    // the public tag rather than a persistent semantic AST property.
    let mut width = terminal_man_field_width(node);
    let mut tag = String::new();
    let mut tag_indentation = indentation;
    let mut children = head.children();
    if let Some(first) = children.next() {
        if !first.flags().line_start
            && let Some(parsed_width) = first.text().and_then(terminal_signed_layout_units)
        {
            width = parsed_width;
        } else if first.flags().line_start {
            // With no same-line width argument, the first Head child is the
            // physical next-line term. Invalid same-line widths are not.
            collect_terminal_text(first, format, limits, &mut tag);
        }
    }
    // `pre_TP()` consumes one same-line width argument only.  Subsequent
    // malformed scanner arguments remain in the public recovery tree, but
    // term.c skips them while looking for the next physical-line tag.  An
    // `.in` request can appear while that Head is open; it changes only the
    // tag's left edge, not the Body field established by TP's width.
    for child in children.filter(|child| child.flags().line_start) {
        if child.macro_name() == Some("in")
            && let Some(value) = terminal_first_text(child)
            && let Some(next) = terminal_man_in_target(value, tag_indentation)
        {
            tag_indentation = next;
        } else {
            collect_terminal_text(child, format, limits, &mut tag);
        }
    }
    if tag.is_empty() {
        let Some(raw_tag) = head.tag() else {
            return Ok(());
        };
        tag = render_terminal_visible_text(raw_tag, format, limits);
    }
    // Like IP, TP's Head is a terminal field rather than literal display
    // text. Escaped trailing blanks reserve field cells (and may move the
    // Body below a long term) but must not themselves print at line end.
    let logical_tag_end = tag_indentation.saturating_add(display_width(&tag));
    tag = tag
        .trim_end_matches(|character: char| {
            character.is_whitespace() || character == TERMINAL_NONBREAKING_SPACE_MARKER
        })
        .to_owned();
    let visible_tag_end = tag_indentation.saturating_add(display_width(&tag));

    let body_indentation = if width.is_negative() {
        indentation.saturating_sub(width.unsigned_abs())
    } else {
        indentation.saturating_add(width.unsigned_abs())
    };
    let body_has_visible_text = terminal_has_visible_text(body, format, limits);
    let body_starts_with_terminal_break = terminal_body_starts_with_break(body);
    let first_body = body.children().find(|child| !child.flags().no_print);
    let first_body_is_no_fill =
        first_body.is_some_and(|child| child.flags().no_fill && child.flags().line_start);

    let density = terminal_man_paragraph_density(node);
    if !terminal_follows_empty_section_paragraph(node)
        && (density.is_none() || terminal_has_visible_predecessor(node))
    {
        if density == Some(0) {
            if !output.is_empty() && !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
        } else {
            append_blank_line(output, maximum)?;
            for _ in 1..density.unwrap_or(1) {
                append(output, "\n", maximum)?;
            }
        }
    }
    let inline_body = !tag.is_empty()
        && body_has_visible_text
        && !body_starts_with_terminal_break
        && body_indentation > logical_tag_end;
    if inline_body {
        // Once a short term shares its field with filled Body text, every
        // wrap continuation belongs to the Body column. A long term starts
        // its Body on a fresh field line instead, so it deliberately retains
        // the tag's own indentation while wrapping.
        append_terminal_hanging_indent(output, body_indentation, maximum)?;
    }
    append_terminal_text(
        output,
        &tag,
        TerminalTextLayout {
            line_start: true,
            // A TP tag is normal fill-mode text. Preserve authored internal
            // spacing only; forcing all tags to no-fill leaves long terms
            // beyond the terminal margin instead of wrapping them.
            keep_spacing: tag.contains('\t')
                || tag.contains("  ")
                || body_indentation > DEFAULT_RENDER_WIDTH,
            ..TerminalTextLayout::default()
        },
        tag_indentation,
        maximum,
    )?;

    // The first body line shares the term's field even when it is no-fill;
    // only *subsequent* no-fill source lines own new physical lines.  This is
    // why a `.TP` opened inside `.nf` displays `term     first line` rather
    // than an empty term line followed by an indented body.
    if inline_body {
        append(
            output,
            &TERMINAL_NONBREAKING_SPACE_MARKER
                .to_string()
                // `append_terminal_text()` contributes the field's ordinary
                // joining cell before the first Body phrase. Protect the
                // remaining padding so fill-mode wrapping cannot collapse
                // the TP column to a single blank.
                .repeat((body_indentation - visible_tag_end).saturating_sub(1)),
            maximum,
        )?;
    } else {
        append(output, "\n", maximum)?;
    }
    let mut consumed_first_no_fill = None;
    if first_body_is_no_fill
        && let Some(first) = first_body
        && let Some(text) = first.text()
    {
        let rendered = render_terminal_visible_text_with_font(
            text.trim_end(),
            format,
            limits,
            terminal_inherited_font(first),
        );
        append_terminal_text(
            output,
            &rendered,
            TerminalTextLayout {
                // The tagged field has already supplied the first line's
                // physical placement; retain no-fill only for wrapping and
                // for subsequent source-line boundaries.
                no_fill: !rendered.is_empty(),
                keep_spacing: first.separator_width() > 1 || rendered.contains("  "),
                ..TerminalTextLayout::default()
            },
            body_indentation,
            maximum,
        )?;
        consumed_first_no_fill = Some(first.id());
    }
    for child in body.children() {
        if Some(child.id()) == consumed_first_no_fill {
            continue;
        }
        render_terminal_node(child, format, limits, body_indentation, output, maximum)?;
    }
    Ok(())
}

/// Render man(7)'s `HP` as a hanging paragraph: its first terminal line keeps
/// the enclosing section field and all wraps/explicit body breaks use the
/// signed Head width. The Head is a layout request, never visible prose.
fn render_terminal_man_hanging_paragraph(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let width = terminal_man_field_width(node);
    let continuation_indentation = if width.is_negative() {
        indentation.saturating_sub(width.unsigned_abs())
    } else {
        indentation.saturating_add(width.unsigned_abs())
    };
    let density = terminal_man_paragraph_density(node);
    // A first HP immediately below a section Head owns no extra paragraph
    // gap. Once visible filled flow has begun, it follows normal man
    // paragraph-density spacing.
    if !output.is_empty() && !output.ends_with('\n') {
        if density == Some(0) {
            append(output, "\n", maximum)?;
        } else {
            append_blank_line(output, maximum)?;
            for _ in 1..density.unwrap_or(1) {
                append(output, "\n", maximum)?;
            }
        }
    }
    let mut children = body.children().filter(|child| !child.flags().no_print);
    let Some(first) = children.next() else {
        return Ok(());
    };
    append_terminal_hanging_indent(output, continuation_indentation, maximum)?;
    render_terminal_node(first, format, limits, indentation, output, maximum)?;
    for child in children {
        if child.macro_name() == Some("fi") && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        render_terminal_node(
            child,
            format,
            limits,
            continuation_indentation,
            output,
            maximum,
        )?;
    }
    Ok(())
}

/// Render mdoc's `Fo`/`Fc` declaration block without flattening the Head and
/// Body in the public tree.  The terminal device makes the Head bold, formats
/// each `Fa` argument in italic with an attached comma, and gives SYNOPSIS
/// declarations their own completed line and trailing semicolon.
fn render_terminal_mdoc_function_block(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut function = String::new();
    if let Some(head) = node.children().find(|child| child.kind() == NodeKind::Head) {
        collect_terminal_semantic_text(head, format, limits, TerminalFont::Bold, &mut function);
    }
    let mut arguments = Vec::new();
    if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
        for child in body.children().filter(|child| !child.flags().no_print) {
            // `Tg` contributes a navigation target to the AST but is
            // transparent to the terminal prototype.  Its text otherwise
            // duplicates the adjacent `Fa` argument.
            if child.macro_name() == Some("Tg") {
                continue;
            }
            if child.macro_name() == Some("Fa") {
                for argument in child.children() {
                    let mut rendered = String::new();
                    collect_terminal_semantic_text(
                        argument,
                        format,
                        limits,
                        TerminalFont::Italic,
                        &mut rendered,
                    );
                    if !rendered.is_empty() {
                        // `termp_fa_pre()` sets `TERMP_NBRWORD` for every
                        // `Fa` argument, not just in SYNOPSIS.  A multiword
                        // type/name phrase therefore moves as one field
                        // after its comma instead of splitting at its
                        // internal authored space.
                        arguments.push(
                            rendered.replace(' ', &TERMINAL_NONBREAKING_SPACE_MARKER.to_string()),
                        );
                    }
                }
            } else if child.macro_name() == Some("Nm") {
                // A recovered synopsis `Nm` can occur as the only argument
                // of a still-open `Fo`. It retains its normal bold device
                // presentation rather than becoming a generic italic
                // function argument.
                let mut rendered = String::new();
                collect_terminal_semantic_text(
                    child,
                    format,
                    limits,
                    TerminalFont::Bold,
                    &mut rendered,
                );
                if !rendered.is_empty() {
                    arguments.push(rendered);
                }
            } else {
                let mut rendered = String::new();
                collect_terminal_semantic_text(
                    child,
                    format,
                    limits,
                    TerminalFont::Italic,
                    &mut rendered,
                );
                if !rendered.is_empty() {
                    arguments.push(rendered);
                }
            }
        }
    }
    render_terminal_mdoc_function_signature(
        node,
        &function,
        &arguments,
        indentation,
        output,
        maximum,
    )
}

/// Render mdoc's one-request function form (`Fn`) using the same terminal
/// semantics as an `Fo` block: first argument is the bold function name and
/// the rest are italic comma-separated prototype arguments.
fn render_terminal_mdoc_function_element(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut children = node.children();
    let mut function = String::new();
    if let Some(name) = children.next() {
        collect_terminal_semantic_text(name, format, limits, TerminalFont::Bold, &mut function);
    }
    let mut arguments = Vec::new();
    for argument in children {
        let mut rendered = String::new();
        collect_terminal_semantic_text(
            argument,
            format,
            limits,
            TerminalFont::Italic,
            &mut rendered,
        );
        if !rendered.is_empty() {
            arguments.push(rendered);
        }
    }
    render_terminal_mdoc_function_signature(
        node,
        &function,
        &arguments,
        indentation,
        output,
        maximum,
    )
}

fn render_terminal_mdoc_function_signature(
    node: NodeRef<'_>,
    function: &str,
    arguments: &[String],
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let synopsis = terminal_mdoc_synopsis(node);
    if synopsis {
        terminal_mdoc_synopsis_spacing(node, output, maximum)?;
    }
    // A function argument is parsed as one mdoc argument phrase even when it
    // contains several visible words. The terminal device therefore moves a
    // whole phrase after a comma to its hanging field instead of splitting a
    // type from its parameter name.
    let nonbreaking_space = TERMINAL_NONBREAKING_SPACE_MARKER.to_string();
    let arguments = arguments
        .iter()
        .map(|argument| {
            if synopsis {
                argument.replace(' ', &nonbreaking_space)
            } else {
                argument.clone()
            }
        })
        .collect::<Vec<_>>();
    // Within a Bk body the terminal has entered `TERMP_KEEP` immediately
    // after emitting the function name.  Thus the separator from one
    // comma-terminated argument to the next is nonbreaking, while spaces
    // authored inside a plain Fn argument retain their ordinary break point.
    // This lets an overfull signature backtrack to the last authored space
    // instead of peeling a later argument onto its own line.
    let argument_separator = if terminal_mdoc_word_keep_scope(node) {
        format!(",{TERMINAL_NONBREAKING_SPACE_MARKER}")
    } else {
        ", ".to_owned()
    };
    let signature = format!(
        "{function}({}){}",
        arguments.join(&argument_separator),
        if synopsis { ";" } else { "" }
    );
    // `termp_fn_pre()` retains a four-cell continuation field below a
    // function starting a device line. Inline description prototypes retain
    // their surrounding field instead, so their marker cannot be injected
    // halfway through an existing output line.
    if synopsis && (output.is_empty() || output.ends_with('\n')) {
        append_terminal_hanging_indent(output, indentation.saturating_add(4), maximum)?;
    }
    append_terminal_text(
        output,
        &signature,
        TerminalTextLayout {
            join: if function.is_empty() {
                TerminalJoin::Attach
            } else {
                TerminalJoin::Separate
            },
            ..TerminalTextLayout::default()
        },
        indentation,
        maximum,
    )?;
    if synopsis && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Render the old-style mdoc header declaration.  Unlike the other bold
/// inline macros, `Fd` always completes a terminal line; in SYNOPSIS it also
/// participates in the declaration-group spacing rule shared with functions
/// and types.
fn render_terminal_mdoc_include_declaration(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if terminal_mdoc_synopsis(node) {
        terminal_mdoc_synopsis_spacing(node, output, maximum)?;
    }
    let mut contents = String::new();
    collect_terminal_semantic_text(node, format, limits, TerminalFont::Bold, &mut contents);
    append_terminal_text(
        output,
        &contents,
        TerminalTextLayout::default(),
        indentation,
        maximum,
    )?;
    if !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Render mdoc's semantic include-file macro.  It is a bold complete C
/// include phrase in SYNOPSIS, but a roman-bracketed italic file name in
/// prose.  Like the terminal device, only adjacent SYNOPSIS `In` elements
/// introduce a physical line boundary; the macro itself does not.
fn render_terminal_mdoc_include_file(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let synopsis = terminal_mdoc_synopsis(node);
    if synopsis {
        terminal_mdoc_synopsis_spacing(node, output, maximum)?;
    }
    let mut contents = String::new();
    let font = if synopsis {
        TerminalFont::Bold
    } else {
        TerminalFont::Italic
    };
    collect_terminal_semantic_text(node, format, limits, font, &mut contents);
    let rendered = if synopsis {
        format!(
            "{}{}{}",
            render_terminal_bold("#include <", format),
            contents,
            render_terminal_bold(">", format)
        )
    } else {
        format!("<{contents}>")
    };
    append_terminal_text(
        output,
        &rendered,
        TerminalTextLayout {
            join: TerminalJoin::Separate,
            ..TerminalTextLayout::default()
        },
        indentation,
        maximum,
    )
}

/// Render mdoc's exceptional explicit enclosure (`Eo`/`Ec`). Unlike the
/// other quote blocks it carries authored Head and Tail delimiters, and an
/// entirely empty pair still counts as a zero-width terminal word.
fn render_terminal_explicit_enclosure(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut tail = None;
    let mut has_head_or_body = false;
    for child in node.children() {
        match child.kind() {
            NodeKind::Head => {
                has_head_or_body |= terminal_has_visible_text(child, format, limits);
                for nested in child.children() {
                    render_terminal_node(nested, format, limits, indentation, output, maximum)?;
                }
                // The Head supplies Eo's opening delimiter.  It is an
                // explicit enclosure boundary, so attach the following Body
                // rather than allowing normal prose layout to insert a space.
                if terminal_has_visible_text(child, format, limits) {
                    mark_terminal_attach_next(output, maximum)?;
                }
            }
            NodeKind::Body => {
                has_head_or_body |= terminal_has_visible_text(child, format, limits);
                for nested in child.children() {
                    render_terminal_node(nested, format, limits, indentation, output, maximum)?;
                }
            }
            NodeKind::Tail => tail = Some(child),
            _ => {}
        }
    }
    let has_tail = tail.is_some_and(|tail| terminal_has_visible_text(tail, format, limits));
    if let Some(tail) = tail.filter(|_| has_tail) {
        if has_head_or_body {
            mark_terminal_attach_next(output, maximum)?;
        }
        for nested in tail.children() {
            render_terminal_node(nested, format, limits, indentation, output, maximum)?;
        }
    } else if has_head_or_body {
        // An opening-only Eo must not leak the opening delimiter's parser
        // attachment into the first normal sibling after the block.
        if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
            let _ = output.pop();
        }
        append(
            output,
            &TERMINAL_FORCE_SEPARATOR_MARKER.to_string(),
            maximum,
        )?;
    } else {
        append_terminal_empty_word(output, indentation, maximum)?;
    }
    Ok(())
}

/// Render an mdoc `Bl -tag` list from the semantic `It` Head/Body pairs.
///
/// Macro-name widths arrive normalized to fixed terminal `n` units, while an
/// authored roff scale is retained for the public AST.  The formatter turns
/// both forms into the terminal field geometry used by `a2width(3)`.
fn render_terminal_definition_list(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let list_indentation = terminal_mdoc_list_indentation(node, indentation);
    // `termp_it_pre()` uses the declared signed `-width` plus its two-cell
    // terminal buffer.  A negative field deliberately outdents the Body;
    // treating it as an unsigned fallback loses the first half of the
    // mdoc tag-list geometry.
    let field_width = node
        .width()
        .map_or(8, |width| terminal_mdoc_a2width(width).saturating_add(2));
    let hanging_list = node.terminal_hanging_list();
    let overhanging_list = node.terminal_overhanging_list();
    let inset_list = node.terminal_inset_list();
    let diagnostic_list = node.terminal_diagnostic_list();
    let body_indentation = if field_width.is_negative() {
        list_indentation.saturating_sub(field_width.unsigned_abs())
    } else {
        list_indentation.saturating_add(field_width.unsigned_abs())
    };
    if terminal_has_visible_predecessor(node) && !node.compact() {
        append_blank_line(output, maximum)?;
    } else if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    let mut first = true;
    for item in body
        .children()
        .filter(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
    {
        let mut tag = String::new();
        let mut contents = String::new();
        let mut structural_tail = Vec::new();
        for child in item.children() {
            match child.kind() {
                NodeKind::Head if diagnostic_list => collect_terminal_semantic_text(
                    child,
                    format,
                    limits,
                    TerminalFont::Bold,
                    &mut tag,
                ),
                NodeKind::Head => collect_terminal_text(child, format, limits, &mut tag),
                NodeKind::Body => {
                    let children = child.children().collect::<Vec<_>>();
                    if let Some(tail_start) =
                        terminal_definition_body_structural_tail_start(&children)
                    {
                        for child in &children[..tail_start] {
                            collect_terminal_text(*child, format, limits, &mut contents);
                        }
                        structural_tail = children[tail_start..].to_vec();
                    } else {
                        collect_terminal_text(child, format, limits, &mut contents);
                    }
                }
                _ => {}
            }
        }
        // A quoted trailing term blank participates in the `Bl -tag` width
        // threshold, but it is not rendered before the Body field.  Preserve
        // the original width for the inline-versus-next-line decision below,
        // then remove it from the emitted fixed-field term.  `-inset` has no
        // fixed field and deliberately retains authored spacing.
        let tag_field_width = display_width(&tag);
        if !inset_list {
            tag = tag
                .trim_end_matches(|character| {
                    character == ' ' || character == TERMINAL_NONBREAKING_SPACE_MARKER
                })
                .to_owned();
        }
        if !first {
            if node.compact() {
                if !output.is_empty() && !output.ends_with('\n') {
                    append(output, "\n", maximum)?;
                }
            } else {
                append_blank_line(output, maximum)?;
            }
        }
        if tag.is_empty() {
            if !contents.is_empty() {
                // Empty item heads do not use the normal fixed definition
                // field for the list forms whose term is itself a block:
                // `-ohang` and `-inset` restart at the list margin, while
                // `-diag` retains its two-cell diagnostic lead-in.  Hanging
                // and tag lists, in contrast, still align an empty term's
                // body with their normal definition field.
                let contents_indentation = if diagnostic_list {
                    list_indentation.saturating_add(2)
                } else if overhanging_list || inset_list {
                    list_indentation
                } else {
                    body_indentation
                };
                append_terminal_text(
                    output,
                    &contents,
                    TerminalTextLayout {
                        line_start: true,
                        ..TerminalTextLayout::default()
                    },
                    contents_indentation,
                    maximum,
                )?;
            }
            render_terminal_definition_tail(
                &structural_tail,
                format,
                limits,
                body_indentation,
                output,
                maximum,
            )?;
            first = false;
            continue;
        }
        if !overhanging_list && !inset_list && !diagnostic_list {
            append_terminal_hanging_indent(output, body_indentation, maximum)?;
        }
        append_terminal_text(
            output,
            &tag,
            TerminalTextLayout {
                line_start: true,
                // `Bl -inset` has no fixed field: quoted trailing term
                // whitespace remains observable before its one-cell Body
                // separator instead of being normalized by filled layout.
                keep_spacing: inset_list && tag.contains("  "),
                ..TerminalTextLayout::default()
            },
            list_indentation,
            maximum,
        )?;
        if overhanging_list {
            if !contents.is_empty() {
                append(output, "\n", maximum)?;
                append_terminal_text(
                    output,
                    &contents,
                    TerminalTextLayout {
                        line_start: true,
                        ..TerminalTextLayout::default()
                    },
                    list_indentation,
                    maximum,
                )?;
            }
            render_terminal_definition_tail(
                &structural_tail,
                format,
                limits,
                list_indentation,
                output,
                maximum,
            )?;
            first = false;
            continue;
        }
        if inset_list || diagnostic_list {
            if !contents.is_empty() {
                let trailing_term_space = tag.ends_with(' ');
                if diagnostic_list || trailing_term_space {
                    append(
                        output,
                        &TERMINAL_NONBREAKING_SPACE_MARKER.to_string(),
                        maximum,
                    )?;
                }
                append_terminal_text(
                    output,
                    &contents,
                    TerminalTextLayout {
                        join: if inset_list && trailing_term_space {
                            TerminalJoin::Attach
                        } else {
                            TerminalJoin::Separate
                        },
                        ..TerminalTextLayout::default()
                    },
                    list_indentation,
                    maximum,
                )?;
            }
            render_terminal_definition_tail(
                &structural_tail,
                format,
                limits,
                list_indentation,
                output,
                maximum,
            )?;
            first = false;
            continue;
        }
        if !contents.is_empty() {
            // `Bl -tag` uses the declared width as its term threshold, then
            // reserves two extra cells before an inline definition. A term
            // that reaches the declared width moves its body to the next
            // line at the wider body indentation. `Bl -hang` shares the
            // normalized definition topology, but it always keeps the first
            // Body phrase on the term line; its width controls continuations
            // only, including negative/zero values.
            if hanging_list
                || (field_width > 0
                    && tag_field_width.saturating_add(2) <= field_width.unsigned_abs())
            {
                // Hanging-list widths are an optional continuation field:
                // when it reaches past the term, align the first Body phrase
                // to that same field; otherwise retain the one ordinary
                // separator that keeps the phrase on the term line.
                let field_gap = field_width
                    .saturating_sub_unsigned(display_width(&tag))
                    .max(1)
                    .unsigned_abs();
                let protected_padding = TERMINAL_NONBREAKING_SPACE_MARKER
                    .to_string()
                    .repeat(field_gap.saturating_sub(1));
                append(output, &protected_padding, maximum)?;
                if body_indentation > DEFAULT_RENDER_WIDTH {
                    // An overflow tag field still accepts its first body
                    // word on the same device line.  Subsequent filled
                    // words resume at the (also overflow) body field;
                    // treating the protected padding as an ordinary break
                    // point would instead leave a padding-only line.
                    let (first_word, remaining) = contents
                        .split_once(' ')
                        .map_or((contents.as_str(), None), |(first, rest)| {
                            (first, Some(rest))
                        });
                    // The ordinary field path below adds its final visible
                    // separator in `append_terminal_text()`.  This overflow
                    // path attaches the first word instead, so retain that
                    // one cell as protected padding.
                    append(
                        output,
                        &TERMINAL_NONBREAKING_SPACE_MARKER.to_string(),
                        maximum,
                    )?;
                    append_terminal_text(
                        output,
                        first_word,
                        TerminalTextLayout {
                            join: TerminalJoin::Attach,
                            ..TerminalTextLayout::default()
                        },
                        body_indentation,
                        maximum,
                    )?;
                    if let Some(remaining) = remaining.filter(|remaining| !remaining.is_empty()) {
                        append(output, "\n", maximum)?;
                        append_terminal_text(
                            output,
                            remaining,
                            TerminalTextLayout {
                                line_start: true,
                                ..TerminalTextLayout::default()
                            },
                            body_indentation,
                            maximum,
                        )?;
                    }
                } else {
                    append_terminal_text(
                        output,
                        &contents,
                        TerminalTextLayout::default(),
                        body_indentation,
                        maximum,
                    )?;
                }
            } else {
                append(output, "\n", maximum)?;
                append_terminal_text(
                    output,
                    &contents,
                    TerminalTextLayout {
                        line_start: true,
                        ..TerminalTextLayout::default()
                    },
                    body_indentation,
                    maximum,
                )?;
            }
        }
        render_terminal_definition_tail(
            &structural_tail,
            format,
            limits,
            body_indentation,
            output,
            maximum,
        )?;
        first = false;
    }
    if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Find the first Body child which switches a definition item from its inline
/// term phrase to independent device flow.  The text collector deliberately
/// flattens ordinary inline macros, so letting it consume a vertical request
/// or nested display/list would discard the boundary and attach later text to
/// the tag field.
fn terminal_definition_body_structural_tail_start(children: &[NodeRef<'_>]) -> Option<usize> {
    children.iter().position(|child| {
        matches!(child.macro_name(), Some("Pp" | "PP" | "LP" | "sp" | "br"))
            || matches!(child.kind(), NodeKind::Table)
            || matches!(child.macro_name(), Some("Bd" | "Bl" | "D1" | "Dl"))
    })
}

fn render_terminal_definition_tail(
    tail: &[NodeRef<'_>],
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let first = tail.first().copied();
    if first.is_some_and(|node| matches!(node.macro_name(), Some("Bd" | "D1" | "Dl")))
        && !output.is_empty()
        && !output.ends_with('\n')
    {
        // A list term's inline field must complete before a compact display
        // begins.  Non-compact displays already own their blank slot, while
        // a compact `Bd` only owns this physical line break.
        append(output, "\n", maximum)?;
    }
    if first.is_some_and(|node| {
        node.macro_name() == Some("Bl") && !terminal_has_visible_predecessor(node)
    }) {
        // A nested list that is the only Body child starts a fresh device
        // field. Unlike a display, the list has no preceding prose of its
        // own to claim that vertical slot, so preserve it here.
        append_blank_line(output, maximum)?;
    }
    for child in tail {
        render_terminal_node(*child, format, limits, indentation, output, maximum)?;
    }
    Ok(())
}

fn is_first_nested_section(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != NodeKind::Body
        || !matches!(parent.macro_name(), Some("SH" | "SS" | "Sh" | "Ss"))
    {
        return false;
    }
    let predecessors = parent
        .children()
        .take_while(|child| child.id() != node.id())
        .collect::<Vec<_>>();
    if predecessors
        .iter()
        .all(|child| child.flags().no_print || child.macro_name() == Some("PD"))
    {
        return true;
    }
    // Consecutive man subsections with only a PD control in the first Body
    // do not make an empty vertical paragraph between their headings.
    node.macro_name() == Some("SS")
        && predecessors.last().is_some_and(|previous| {
            previous.kind() == NodeKind::Block
                && previous.macro_name() == Some("SS")
                && previous
                    .children()
                    .find(|child| child.kind() == NodeKind::Body)
                    .is_some_and(|body| {
                        body.children()
                            .all(|child| child.flags().no_print || child.macro_name() == Some("PD"))
                    })
        })
}

fn terminal_section_body_indent(node: NodeRef<'_>) -> usize {
    match node.macro_name() {
        Some("SH" | "SS") => 7,
        Some("Sh" | "Ss") => 5,
        _ => 0,
    }
}

fn terminal_empty_man_section_starts_plain_flow(node: NodeRef<'_>, body: NodeRef<'_>) -> bool {
    matches!(node.macro_name(), Some("SH" | "SS"))
        && body
            .children()
            .find(|child| !child.flags().no_print)
            .is_some_and(|child| {
                child.kind() == NodeKind::Text || matches!(child.macro_name(), Some("nf" | "fi"))
            })
}

fn terminal_section_heading_indent(node: NodeRef<'_>) -> usize {
    match node.macro_name() {
        Some("SS" | "Ss") => 3,
        _ => 0,
    }
}

fn terminal_mdoc_element_font(node: NodeRef<'_>) -> Option<TerminalFont> {
    match node.macro_name() {
        // The 1.14.6 terminal device presents these mdoc argument families
        // in bold, including their formatter-control escapes.
        Some("Cd" | "Cm" | "Fd" | "Fl" | "Ic" | "Ms" | "Sy") => Some(TerminalFont::Bold),
        Some("Ad" | "Ar" | "Em" | "Fa" | "Fr" | "Ft" | "Mt" | "Pa" | "Sx" | "Va") => {
            Some(TerminalFont::Italic)
        }
        // `Li` establishes an explicit literal/roman scope.  In particular,
        // it must override the surrounding `Vt` italic presentation rather
        // than inheriting that variable-type scope into its children.
        Some("Li") => Some(TerminalFont::Roman),
        _ => None,
    }
}

/// Mdoc inline semantic macros leave sentence separation to their enclosing
/// prose state. A terminal period in their rendered argument is not by itself
/// a request for the device's automatic double-sentence gap.
fn terminal_mdoc_inline_punctuation_is_literal(node: NodeRef<'_>) -> bool {
    match node.macro_name() {
        // Cd's punctuation can be either a direct argument or a separately
        // parsed sentence delimiter. Only the former suppresses automatic
        // sentence spacing (`Cd pciide?`); an outer `Cd options INSECURE .`
        // must leave the following sentence-ending delimiter observable.
        Some("Cd") => node
            .children()
            .filter_map(NodeRef::text)
            .next_back()
            .is_some_and(terminal_sentence_terminator),
        Some("Ad" | "Dv" | "Er" | "Ev" | "Ic" | "Ms" | "Va" | "Vt") => true,
        _ => false,
    }
}

/// A text node directly in mdoc's ordinary block flow can end a terminal
/// sentence. Text nested inside a semantic mdoc inline macro deliberately
/// does not: those macros have their own punctuation and spacing contracts.
fn terminal_mdoc_plain_text_sentence(node: NodeRef<'_>) -> bool {
    node.ancestors()
        .any(|ancestor| matches!(ancestor.macro_name(), Some("Sh" | "Ss")))
        && !node
            .ancestors()
            .any(|ancestor| terminal_mdoc_element_font(ancestor).is_some())
}

/// A childless mdoc `Fl` still prints its own dash.  When the next visible
/// same-line node is another macro, `termp_fl_pre()` keeps that macro attached
/// to the dash (`Fl Cm help` → `-help`); ordinary text deliberately retains a
/// separator.  Transparent nodes do not decide the boundary themselves.
fn terminal_mdoc_empty_fl_attaches_to_following_macro(node: NodeRef<'_>) -> bool {
    if node.macro_name() != Some("Fl") || node.children().next().is_some() {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .children()
        .skip_while(|sibling| sibling.id() != node.id())
        .skip(1)
        .find(|sibling| !sibling.flags().no_print)
        .is_some_and(|next| {
            next.macro_name().is_some()
                && node
                    .source_position()
                    .zip(next.source_position())
                    .is_some_and(|(current, following)| current.line == following.line)
        })
}

/// `.Pf` owns one literal prefix and attaches exactly to the next visible
/// same-line token.  Unlike an empty `.Fl`, the following token may be either
/// a macro or ordinary text.  Parser validation reports an incomplete prefix,
/// but rendering also checks this relationship so recovery cannot join it to
/// a later physical source line.
fn terminal_mdoc_prefix_attaches_to_following_token(node: NodeRef<'_>) -> bool {
    if node.macro_name() != Some("Pf") {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .children()
        .skip_while(|sibling| sibling.id() != node.id())
        .skip(1)
        .find(|sibling| !sibling.flags().no_print)
        .is_some_and(|next| {
            node.source_position()
                .zip(next.source_position())
                .is_some_and(|(current, following)| current.line == following.line)
        })
}

/// Render man-ext `OP` as its terminal option synopsis.
///
/// The parser keeps all recovered arguments for diagnostics, but the device
/// consumes at most two: the option in bold and its operand in italic.
fn terminal_man_option(node: NodeRef<'_>, format: RenderFormat, limits: &Limits) -> String {
    let mut arguments = node.children().filter(|child| !child.flags().no_print);
    let Some(option) = arguments.next() else {
        return "[]".to_owned();
    };
    let mut contents = String::from("[");
    let mut option_text = String::new();
    collect_terminal_semantic_text(option, format, limits, TerminalFont::Bold, &mut option_text);
    contents.push_str(&option_text);
    if let Some(argument) = arguments.next() {
        let mut value = String::new();
        collect_terminal_semantic_text(argument, format, limits, TerminalFont::Italic, &mut value);
        if !value.is_empty() {
            contents.push(' ');
            contents.push_str(&value);
        }
    }
    contents.push(']');
    contents
}

/// Terminal fonts for man(7)'s two-argument alternating requests.
///
/// `man_term.c:pre_alternate()` toggles the device font after every argument
/// and sets `TERMP_NOSPACE` between them.  Font-size-only `SB`/`SM` requests
/// are handled as ordinary bold/roman text elsewhere; these six names are the
/// complete terminal alternating family.
fn terminal_man_alternating_fonts(name: Option<&str>) -> Option<[TerminalFont; 2]> {
    match name {
        Some("BI") => Some([TerminalFont::Bold, TerminalFont::Italic]),
        Some("IB") => Some([TerminalFont::Italic, TerminalFont::Bold]),
        Some("BR") => Some([TerminalFont::Bold, TerminalFont::Roman]),
        Some("RB") => Some([TerminalFont::Roman, TerminalFont::Bold]),
        Some("IR") => Some([TerminalFont::Italic, TerminalFont::Roman]),
        Some("RI") => Some([TerminalFont::Roman, TerminalFont::Italic]),
        _ => None,
    }
}

fn terminal_inherited_font(node: NodeRef<'_>) -> TerminalFont {
    terminal_scope_font(node).unwrap_or_default()
}

/// Return a structural mdoc font when one owns this node.  A plain roff text
/// node has no such scope and consequently inherits the document-order `.ft`
/// state instead of being reset to Roman.
fn terminal_scope_font(node: NodeRef<'_>) -> Option<TerminalFont> {
    if terminal_bf_scope_closed_before(node) {
        return Some(TerminalFont::Roman);
    }
    node.ancestors().find_map(|ancestor| {
        // A `Bf` without a recognized font argument resets its nested
        // scope to Roman.  The normalized AST represents both missing
        // and unknown arguments with `font == None`, which is precisely
        // the terminal device's shared fallback behavior.
        if ancestor.kind() == NodeKind::Block
            && ancestor.macro_name() == Some("Bf")
            && ancestor.font().is_none()
        {
            return Some(TerminalFont::Roman);
        }
        let font = ancestor.font().map(|font| match font {
            NormalizedFont::Emphasis => TerminalFont::Italic,
            NormalizedFont::Literal => TerminalFont::Roman,
            NormalizedFont::Symbolic => TerminalFont::Bold,
        });
        font.or_else(|| {
            // `Vt` italicizes its direct text arguments, but it does
            // not flatten nested semantic macro children: a nested `Sy`
            // must still render bold.  Inheritance preserves that
            // source-level boundary while covering both inline and
            // SYNOPSIS partial-block forms.
            (ancestor.macro_name() == Some("Vt")).then_some(TerminalFont::Italic)
        })
    })
}

/// Resolve the effective device font for one ordinary text node.  Structural
/// mdoc scopes deliberately take precedence over roff's process-like `.ft`
/// register; outside those scopes the request state remains in effect across
/// ordinary sibling blocks just as it does in the terminal device.
fn terminal_text_font(node: NodeRef<'_>) -> TerminalFont {
    terminal_scope_font(node).unwrap_or_else(|| terminal_request_font_before(node).current)
}

/// Reconstruct the `.ft` register immediately before `node` in document
/// order. Each level contributes every prior sibling subtree before advancing
/// down the path to the target, which handles requests nested inside a roff
/// body without relying on arena IDs or mutable global state.
fn terminal_request_font_before(node: NodeRef<'_>) -> TerminalRequestFontState {
    let mut lineage = vec![node];
    let mut cursor = node;
    while let Some(parent) = cursor.parent() {
        lineage.push(parent);
        cursor = parent;
    }
    lineage.reverse();

    let mut state = TerminalRequestFontState::default();
    for current in lineage.into_iter().skip(1) {
        let Some(parent) = current.parent() else {
            continue;
        };
        for sibling in parent.children() {
            if sibling.id() == current.id() {
                break;
            }
            terminal_apply_font_requests(sibling, &mut state);
        }
    }
    state
}

fn terminal_apply_font_requests(node: NodeRef<'_>, state: &mut TerminalRequestFontState) {
    if node.kind() == NodeKind::Element && node.macro_name() == Some("ft") {
        let selector = node.children().find_map(NodeRef::text);
        terminal_apply_font_request(selector, state);
        return;
    }
    for child in node.children() {
        terminal_apply_font_requests(child, state);
    }
}

fn terminal_apply_font_request(selector: Option<&str>, state: &mut TerminalRequestFontState) {
    let next = match selector.unwrap_or_default() {
        "B" | "CB" => Some(TerminalFont::Bold),
        "I" | "CI" => Some(TerminalFont::Italic),
        "BI" => Some(TerminalFont::BoldItalic),
        "R" | "CR" => Some(TerminalFont::Roman),
        "" | "P" => {
            std::mem::swap(&mut state.current, &mut state.previous);
            None
        }
        _ => None,
    };
    if let Some(next) = next {
        state.previous = state.current;
        state.current = next;
    }
}

/// Apply the cumulative `.po` device offset to one text node's enclosing
/// field. The raw offset can extend beyond the visible page; mandoc retains
/// that value for a later relative request, then clamps only the rendered
/// field to the terminal's `[-offset, 60]` range.
fn terminal_text_indentation(node: NodeRef<'_>, indentation: usize) -> usize {
    // A source tail released by `Fc` resumes one cell into the SYNOPSIS
    // field. The public AST correctly exposes it as the next text sibling of
    // `Fo`, but not the terminal-only continuation column.
    let indentation = if terminal_mdoc_function_tail(node) {
        indentation.saturating_add(1)
    } else {
        indentation
    };
    let indentation = terminal_request_indent_before(node, indentation).unwrap_or(indentation);
    let state = terminal_page_offset_before(node);
    let lower = -isize::try_from(indentation).unwrap_or(isize::MIN);
    let applied = state.current.clamp(lower, 60);
    if applied.is_negative() {
        indentation.saturating_sub(applied.unsigned_abs())
    } else {
        indentation.saturating_add(applied.unsigned_abs())
    }
}

fn terminal_mdoc_function_tail(node: NodeRef<'_>) -> bool {
    node.kind() == NodeKind::Text
        && terminal_previous_sibling(node).is_some_and(|previous| {
            previous.kind() == NodeKind::Block
                && previous.macro_name() == Some("Fo")
                && terminal_mdoc_synopsis(previous)
        })
}

fn terminal_page_offset_before(node: NodeRef<'_>) -> TerminalPageOffsetState {
    let mut lineage = vec![node];
    let mut cursor = node;
    while let Some(parent) = cursor.parent() {
        lineage.push(parent);
        cursor = parent;
    }
    lineage.reverse();

    let mut state = TerminalPageOffsetState::default();
    for current in lineage.into_iter().skip(1) {
        let Some(parent) = current.parent() else {
            continue;
        };
        for sibling in parent.children() {
            if sibling.id() == current.id() {
                break;
            }
            terminal_apply_page_offset_requests(sibling, &mut state);
        }
    }
    state
}

fn terminal_apply_page_offset_requests(node: NodeRef<'_>, state: &mut TerminalPageOffsetState) {
    if node.kind() == NodeKind::Element && node.macro_name() == Some("po") {
        let requested = node.children().find_map(NodeRef::text);
        terminal_apply_page_offset_request(requested, state);
        return;
    }
    for child in node.children() {
        terminal_apply_page_offset_requests(child, state);
    }
}

fn terminal_apply_page_offset_request(
    requested: Option<&str>,
    state: &mut TerminalPageOffsetState,
) {
    let relative = requested.is_some_and(|value| value.trim_start().starts_with(['+', '-']));
    let next = requested
        .and_then(terminal_page_offset_units)
        .map_or(state.previous, |value| {
            if relative {
                state.current.saturating_add(value)
            } else {
                value
            }
        });
    state.previous = state.current;
    state.current = next;
}

fn terminal_page_offset_units(value: &str) -> Option<isize> {
    terminal_signed_layout_units(value).or_else(|| value.trim().parse().ok())
}

/// Resolve the most recent roff `.in` request before a text node.  Its
/// absolute device column wins over the structural field passed by the AST;
/// a first relative request uses that structural field as its base.
fn terminal_request_indent_before(node: NodeRef<'_>, base: usize) -> Option<usize> {
    let mut lineage = vec![node];
    let mut cursor = node;
    while let Some(parent) = cursor.parent() {
        lineage.push(parent);
        cursor = parent;
    }
    lineage.reverse();

    let mut state = TerminalRequestIndentState::default();
    for current in lineage.into_iter().skip(1) {
        let Some(parent) = current.parent() else {
            continue;
        };
        for sibling in parent.children() {
            if sibling.id() == current.id() {
                break;
            }
            terminal_apply_indent_requests(sibling, base, &mut state);
        }
    }
    state.current.map(|value| value.max(0).unsigned_abs())
}

fn terminal_apply_indent_requests(
    node: NodeRef<'_>,
    base: usize,
    state: &mut TerminalRequestIndentState,
) {
    if matches!(node.macro_name(), Some("Pp" | "PP" | "LP")) {
        // Paragraph macros re-enter their package-managed body field, which
        // supersedes a preceding raw roff indentation request.
        state.current = None;
        return;
    }
    if node.kind() == NodeKind::Element
        && node.macro_name() == Some("in")
        && !terminal_man_tp_head_indent_request(node)
    {
        terminal_apply_indent_request(node.children().find_map(NodeRef::text), base, state);
        return;
    }
    for child in node.children() {
        terminal_apply_indent_requests(child, base, state);
    }
}

/// A man `TP` keeps an `.in` request inside its Head as a tag-only layout
/// adjustment. `render_terminal_man_tp` consumes that private meaning while
/// placing the tag; it must not update the ordinary roff field register seen
/// by the following Body.
fn terminal_man_tp_head_indent_request(node: NodeRef<'_>) -> bool {
    node.ancestors().any(|ancestor| {
        ancestor.kind() == NodeKind::Head
            && ancestor
                .parent()
                .is_some_and(|parent| parent.macro_name() == Some("TP"))
    })
}

fn terminal_apply_indent_request(
    requested: Option<&str>,
    base: usize,
    state: &mut TerminalRequestIndentState,
) {
    let Some(value) = requested.map(str::trim).filter(|value| !value.is_empty()) else {
        state.current = None;
        return;
    };
    let Some(units) = terminal_signed_roff_en_prefix(value) else {
        state.current = None;
        return;
    };
    if value.starts_with(['+', '-']) {
        let base = state
            .current
            .unwrap_or_else(|| isize::try_from(base).unwrap_or(isize::MAX));
        state.current = Some(base.saturating_add(units));
    } else {
        state.current = Some(units);
    }
}

/// Reconstruct the `.ll` register before one text node.  As with font and
/// page-offset requests, every prior sibling subtree along the ancestor path
/// contributes state, while the request's own AST argument stays public.
fn terminal_line_length_before(node: NodeRef<'_>) -> TerminalLineLength {
    let mut lineage = vec![node];
    let mut cursor = node;
    while let Some(parent) = cursor.parent() {
        lineage.push(parent);
        cursor = parent;
    }
    lineage.reverse();

    let mut state = TerminalLineLength::Default;
    for current in lineage.into_iter().skip(1) {
        let Some(parent) = current.parent() else {
            continue;
        };
        for sibling in parent.children() {
            if sibling.id() == current.id() {
                break;
            }
            terminal_apply_line_length_requests(sibling, &mut state);
        }
    }
    state
}

fn terminal_apply_line_length_requests(node: NodeRef<'_>, state: &mut TerminalLineLength) {
    if node.kind() == NodeKind::Element && node.macro_name() == Some("ll") {
        terminal_apply_line_length_request(node.children().find_map(NodeRef::text), state);
        return;
    }
    for child in node.children() {
        terminal_apply_line_length_requests(child, state);
    }
}

/// Apply the subset of `.ll` requests that changes a terminal field. Bare or
/// malformed requests restore the renderer's configured default; a signed
/// valid request remains symbolic when based on that default so a caller's
/// nonstandard `Renderer::with_width()` is honoured at the final width pass.
fn terminal_apply_line_length_request(requested: Option<&str>, state: &mut TerminalLineLength) {
    let Some(value) = requested.map(str::trim).filter(|value| !value.is_empty()) else {
        *state = TerminalLineLength::Default;
        return;
    };
    let Some(units) = terminal_signed_layout_units(value) else {
        *state = TerminalLineLength::Default;
        return;
    };
    if value.starts_with(['+', '-']) {
        *state = match *state {
            TerminalLineLength::Default => TerminalLineLength::Relative(units),
            TerminalLineLength::Relative(prior) => {
                TerminalLineLength::Relative(prior.saturating_add(units))
            }
            TerminalLineLength::Absolute(prior) => {
                TerminalLineLength::Absolute(prior.saturating_add_signed(units))
            }
        };
    } else {
        *state = TerminalLineLength::Absolute(units.max(0).unsigned_abs());
    }
}

/// Whether an mdoc `Ef` was preserved as an otherwise empty `Bf` Body before
/// this node inside an outer syntactic scope.  The canonical AST must retain
/// that recovery node for source compatibility; terminal presentation uses it
/// as a state transition from the enclosing Bf font back to Roman.
fn terminal_bf_scope_closed_before(node: NodeRef<'_>) -> bool {
    let closes_bf = node
        .ancestors()
        .any(|ancestor| ancestor.macro_name() == Some("Bf"));
    let mut current = node;
    while let Some(parent) = current.parent() {
        for sibling in parent.children() {
            if sibling.id() == current.id() {
                break;
            }
            if terminal_is_closed_bf_scope(sibling)
                || (closes_bf
                    && terminal_embedded_quote_closing(sibling, RenderFormat::Ascii).is_some())
            {
                return true;
            }
        }
        current = parent;
    }
    false
}

fn terminal_contains_closed_bf_scope(node: NodeRef<'_>) -> bool {
    terminal_is_closed_bf_scope(node) || node.children().any(terminal_contains_closed_bf_scope)
}

fn terminal_is_closed_bf_scope(node: NodeRef<'_>) -> bool {
    node.kind() == NodeKind::Body
        && node.macro_name() == Some("Bf")
        && node.font().is_some()
        && node.children().next().is_none()
}

fn terminal_mdoc_display_indentation(node: NodeRef<'_>, indentation: usize) -> usize {
    let offset = terminal_mdoc_display_offset(node);
    if offset.is_negative() {
        indentation.saturating_sub(offset.unsigned_abs())
    } else {
        indentation.saturating_add(offset.unsigned_abs())
    }
}

fn terminal_mdoc_display_offset(node: NodeRef<'_>) -> isize {
    match node.offset() {
        None | Some("left") => 0,
        Some("indent") => 6,
        Some("indent-two") => 12,
        Some(value) => terminal_signed_layout_units(value)
            .unwrap_or_else(|| isize::try_from(display_width(value)).unwrap_or(isize::MAX)),
    }
}

fn terminal_mdoc_list_indentation(node: NodeRef<'_>, indentation: usize) -> usize {
    let offset = match node.offset() {
        None => 0,
        // These mdoc layout keywords name terminal fields rather than source
        // strings. Unknown names fall back to their visible-cell width.
        Some("left") => 4,
        Some("indent") => 6,
        Some("indent-two") => 10,
        Some(value) => terminal_signed_layout_units(value)
            .unwrap_or_else(|| isize::try_from(display_width(value)).unwrap_or(isize::MAX)),
    };
    if offset.is_negative() {
        indentation.saturating_sub(offset.unsigned_abs())
    } else {
        indentation.saturating_add(offset.unsigned_abs())
    }
}

fn terminal_authors_section(node: NodeRef<'_>) -> bool {
    terminal_mdoc_section_named(node, "AUTHORS")
}

/// Return the compact mdoc system-name forms with one optional version
/// argument.  `St` is deliberately excluded: its expanded standard name is
/// ordinary prose, not a single device word.
fn terminal_mdoc_system_macro(name: Option<&str>) -> bool {
    matches!(name, Some("Bsx" | "Dx" | "Fx" | "Nx" | "Ox" | "Ux"))
}

/// Render the stable system-name case of mdoc's short-lived `Bk` word keep.
/// The full macro keeps inter-node word boundaries by source line; use this
/// narrow renderer-private projection only once a system macro is present,
/// leaving complex `Bk` bodies on their established structural path.
fn terminal_mdoc_system_word_keep(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> Option<String> {
    let body = node
        .children()
        .find(|child| child.kind() == NodeKind::Body)?;
    let children = body
        .children()
        .filter(|child| !child.flags().no_print)
        .collect::<Vec<_>>();
    if !children
        .iter()
        .any(|child| terminal_mdoc_system_macro(child.macro_name()))
    {
        return None;
    }
    let mut output = String::new();
    for child in children {
        let mut fragment = if child.macro_name() == Some("Xr") {
            terminal_cross_reference(child, format, limits).unwrap_or_default()
        } else {
            let mut fragment = String::new();
            collect_terminal_text(child, format, limits, &mut fragment);
            fragment
        };
        if terminal_mdoc_system_macro(child.macro_name()) {
            fragment = fragment.replace(' ', &TERMINAL_NONBREAKING_SPACE_MARKER.to_string());
        }
        if fragment.is_empty() {
            continue;
        }
        if !output.is_empty() {
            output.push(if child.flags().line_start {
                ' '
            } else {
                TERMINAL_NONBREAKING_SPACE_MARKER
            });
        }
        output.push_str(&fragment);
    }
    Some(output)
}

/// Collect an ordinary `Bk` Body into one unbreakable device phrase.
///
/// `Bk`'s Head contains layout selectors (and, after recovery, invalid
/// selector tail words) rather than display content.  Its Body is the only
/// phrase that participates in the keep request.  Keep this narrow to inline
/// content so block-level layouts retain their established structural paths.
fn terminal_mdoc_word_keep(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> Option<String> {
    let body = node
        .children()
        .find(|child| child.kind() == NodeKind::Body)?;
    let children = body
        .children()
        .filter(|child| !child.flags().no_print)
        .collect::<Vec<_>>();
    if children.is_empty()
        // A word keep around ordinary free-form text is intentionally inert:
        // only a macro-owned phrase activates the device keep state.
        || children.iter().all(|child| child.kind() == NodeKind::Text)
        || children.iter().any(|child| {
            matches!(child.kind(), NodeKind::Table | NodeKind::Equation)
                || matches!(child.macro_name(), Some("Bd" | "Bl" | "D1" | "Dl" | "Fn" | "Fo"))
        })
    {
        return None;
    }
    let line_started_fragments = terminal_mdoc_bk_line_started_fragments(body, format, limits);
    let mut output = String::new();
    for child in children {
        let fragment = if child.macro_name() == Some("Xr") {
            terminal_cross_reference(child, format, limits).unwrap_or_default()
        } else {
            let mut fragment = String::new();
            collect_terminal_text(child, format, limits, &mut fragment);
            fragment
        };
        if fragment.is_empty() {
            continue;
        }
        if !output.is_empty() {
            output.push(if child.flags().line_start {
                ' '
            } else {
                TERMINAL_NONBREAKING_SPACE_MARKER
            });
        }
        output.push_str(&fragment.replace(' ', &TERMINAL_NONBREAKING_SPACE_MARKER.to_string()));
    }
    for punctuation in ['.', ',', ';', ':', '!', '?', ')', ']'] {
        output = output.replace(
            &format!("{TERMINAL_NONBREAKING_SPACE_MARKER}{punctuation}"),
            &punctuation.to_string(),
        );
    }
    // `Bk` keeps words only after the first rendered word on a physical
    // source line.  A nested optional or plain `No` word that starts a later
    // line after its preceding sibling has closed therefore retains an
    // ordinary breakable separator. The arena has already normalized the
    // literal `Oc`, but the nested Body sibling boundary still distinguishes
    // this from a line containing only a new `Oo` opener.
    for fragment in line_started_fragments {
        output = output.replace(
            &format!("{TERMINAL_NONBREAKING_SPACE_MARKER}{fragment}"),
            &format!(" {fragment}"),
        );
    }
    (!output.is_empty()).then_some(output)
}

fn terminal_mdoc_bk_line_started_fragments(
    body: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> Vec<String> {
    fn visit(node: NodeRef<'_>, format: RenderFormat, limits: &Limits, output: &mut Vec<String>) {
        let is_optional = node.kind() == NodeKind::Block && node.macro_name() == Some("Oo");
        let is_plain_no = node.kind() == NodeKind::Element && node.macro_name() == Some("No");
        if (is_optional || is_plain_no)
            && node.flags().line_start
            && node
                .parent()
                .is_some_and(|parent| parent.macro_name() == Some("Oo"))
            && terminal_previous_sibling(node).is_some()
        {
            let mut optional = String::new();
            collect_terminal_text(node, format, limits, &mut optional);
            if !optional.is_empty() {
                output.push(optional);
            }
        }
        for child in node.children() {
            visit(child, format, limits, output);
        }
    }

    let mut optionals = Vec::new();
    for child in body.children() {
        visit(child, format, limits, &mut optionals);
    }
    optionals
}

/// Select the synopsis continuation field for a kept phrase.
///
/// `Bk` continues below the owning declaration name, not at a fixed global
/// offset.  The compatible tree keeps that declaration as an ancestor, so
/// recover its display width only for this renderer-private field decision.
fn terminal_mdoc_bk_continuation_indent(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
) -> usize {
    let Some(name) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == NodeKind::Block && ancestor.macro_name() == Some("Nm"))
        .and_then(|name| name.children().find(|child| child.kind() == NodeKind::Head))
    else {
        return indentation.saturating_add(10);
    };
    let mut rendered = String::new();
    collect_terminal_mdoc_synopsis_name_head(name, format, limits, &mut rendered);
    if rendered.is_empty() {
        indentation.saturating_add(10)
    } else {
        indentation
            .saturating_add(display_width(&rendered))
            .saturating_add(1)
    }
}

/// A synopsis declaration whose implicit `Nm` Head exceeds the device width
/// retains each later mdoc macro argument as one field phrase.  Otherwise the
/// width pass would split the synthesized default of a bare `Ar` into two
/// impossible columns beyond the name field.
fn terminal_mdoc_long_name_field(node: NodeRef<'_>, format: RenderFormat, limits: &Limits) -> bool {
    let Some(head) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == NodeKind::Block && ancestor.macro_name() == Some("Nm"))
        .and_then(|name| name.children().find(|child| child.kind() == NodeKind::Head))
    else {
        return false;
    };
    let mut rendered = String::new();
    collect_terminal_mdoc_synopsis_name_head(head, format, limits, &mut rendered);
    display_width(&rendered) > 70
}

/// Resolve the persistent mdoc `An` layout mode in one containing body.
///
/// The parser keeps an option directive as a public `An` element so AST
/// consumers can observe it, but the terminal device treats that element as
/// a state update and consumes all its remaining words.  `An` siblings are
/// emitted in source order under a single mdoc body, so a bounded sibling
/// scan exactly matches the device's state without adding renderer state to
/// the public arena.
fn terminal_author_mode(node: NodeRef<'_>) -> AuthorMode {
    let mut mode = if terminal_authors_section(node) {
        AuthorMode::Split
    } else {
        AuthorMode::NoSplit
    };
    let Some(parent) = node.parent() else {
        return mode;
    };
    for sibling in parent.children() {
        if sibling.id() == node.id() {
            break;
        }
        if sibling.macro_name() == Some("An")
            && let Some(updated) = sibling.author_mode()
        {
            mode = updated;
        }
    }
    mode
}

/// A split author begins a fresh terminal line after an earlier `An` sibling.
/// The AUTHORS section's implicit initial split mode deliberately leaves its
/// first author attached to preceding prose; an explicit `-split` directive
/// counts as the earlier sibling and therefore starts the next author line.
fn terminal_author_starts_line(node: NodeRef<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent
            .children()
            .take_while(|sibling| sibling.id() != node.id())
            .any(|sibling| sibling.macro_name() == Some("An"))
    })
}

fn terminal_mdoc_section_named(node: NodeRef<'_>, name: &str) -> bool {
    node.ancestors().any(|ancestor| {
        if ancestor.kind() != NodeKind::Block || ancestor.macro_name() != Some("Sh") {
            return false;
        }
        let Some(head) = ancestor
            .children()
            .find(|child| child.kind() == NodeKind::Head)
        else {
            return false;
        };
        let mut title = String::new();
        collect_terminal_plain_words(head, &mut title);
        title.eq_ignore_ascii_case(name)
    })
}

fn collect_terminal_plain_words(node: NodeRef<'_>, output: &mut String) {
    if let Some(text) = node.text()
        && !text.is_empty()
    {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(text);
    }
    for child in node.children() {
        collect_terminal_plain_words(child, output);
    }
}

fn terminal_mdoc_synopsis(node: NodeRef<'_>) -> bool {
    node.flags().synopsis_pretty || terminal_mdoc_section_named(node, "SYNOPSIS")
}

/// A paragraph can be parsed after an inline `nS` reset while still sitting
/// inside an already-open synopsis-pretty `Nm` block.  The device retains the
/// declaration field through that nested recovery shape, so the paragraph's
/// own flag is not sufficient to select its continuation column.
fn terminal_mdoc_synopsis_paragraph(node: NodeRef<'_>) -> bool {
    node.flags().synopsis_pretty
        || node.ancestors().any(|ancestor| {
            ancestor.kind() == NodeKind::Block
                && ancestor.macro_name() == Some("Nm")
                && ancestor.flags().synopsis_pretty
        })
}

/// A synopsis paragraph inherits the name continuation field only while it
/// remains structurally inside the owning `Nm` block. Section-level synopsis
/// prose and function declarations use the ordinary five-cell field even
/// though their parser flags also carry synopsis provenance.
fn terminal_mdoc_synopsis_name_paragraph(node: NodeRef<'_>) -> bool {
    node.ancestors().any(|ancestor| {
        ancestor.kind() == NodeKind::Block
            && ancestor.macro_name() == Some("Nm")
            && ancestor.flags().synopsis_pretty
    })
}

/// True for the compact `Nm` synopsis grammar consisting solely of optional
/// argument forms.  Its body uses the device's standard five-plus-four-cell
/// continuation field, unlike an arbitrary mixed synopsis body (and unlike a
/// nested `Bk`, which calculates its own field from the preceding argument).
fn terminal_mdoc_synopsis_option_body(node: NodeRef<'_>) -> bool {
    let mut found = false;
    node.children()
        .filter(|child| !child.flags().no_print)
        .all(|child| {
            found = true;
            child.kind() == NodeKind::Block && child.macro_name() == Some("Op")
        })
        && found
}

/// Whether `node` is being formatted inside an mdoc `Bk` body.  The public
/// compatible AST intentionally discards the validator-only `-words` option,
/// but every retained Bk block represents the terminal keep scope introduced
/// by that request.
fn terminal_mdoc_word_keep_scope(node: NodeRef<'_>) -> bool {
    node.ancestors()
        .any(|ancestor| ancestor.kind() == NodeKind::Block && ancestor.macro_name() == Some("Bk"))
}

/// Mirror the terminal device's `synopsis_pre()` vertical spacing for the
/// declaration families currently rendered structurally.  `Ft` followed by a
/// function starts the next declaration line; a later `Ft` after a completed
/// function starts a new vertical group.
fn terminal_mdoc_synopsis_spacing(
    node: NodeRef<'_>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(previous) = terminal_previous_sibling(node) else {
        return Ok(());
    };
    if previous.macro_name() == node.macro_name()
        && !matches!(node.macro_name(), Some("Ft" | "Fo" | "Fn"))
    {
        if !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    match previous.macro_name() {
        Some("Fd" | "Fn" | "Fo" | "In" | "Vt") => append_blank_line(output, maximum),
        Some("Ft") if node.macro_name() == Some("Ft") => append_blank_line(output, maximum),
        _ if !output.is_empty() && !output.ends_with('\n') => append(output, "\n", maximum),
        _ => Ok(()),
    }
}

fn terminal_previous_sibling(node: NodeRef<'_>) -> Option<NodeRef<'_>> {
    node.parent()?
        .children()
        .take_while(|child| child.id() != node.id())
        .last()
}

fn terminal_next_visible_sibling(node: NodeRef<'_>) -> Option<NodeRef<'_>> {
    node.parent()?
        .children()
        .skip_while(|child| child.id() != node.id())
        .skip(1)
        .find(|child| !child.flags().no_print)
}

fn terminal_signed_layout_units(value: &str) -> Option<isize> {
    if let Some(value) = value.strip_suffix('n') {
        return value.parse().ok();
    }
    let value = value.strip_suffix('i')?.parse::<f64>().ok()?;
    // The terminal device rounds scaled inch values to the nearest `n` unit.
    (value * 10.0).round().to_string().parse().ok()
}

/// Parse the bare numeric field width accepted by a man `RS` request.
///
/// The caller has already tried all scaled forms.  The terminal device
/// truncates a finite bare decimal toward zero and accepts only values an
/// `isize` can represent.
#[allow(clippy::cast_precision_loss)] // Bounds only compare the f64 parser domain with the target integer range.
fn terminal_plain_field_width(value: &str) -> Option<isize> {
    let value = value.parse::<f64>().ok()?;
    if !value.is_finite() || value < isize::MIN as f64 || value > isize::MAX as f64 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some(value as isize)
}

/// Apply man(7)'s persistent `.in` request to a terminal field.  The parser
/// normalizes a request captured below an open `TP` Head to a signed relative
/// value, while an ordinary unsigned request names an absolute column.
fn terminal_man_in_target(value: &str, indentation: usize) -> Option<usize> {
    let value = value.trim();
    let units = terminal_signed_layout_units(value)?;
    if value.starts_with(['+', '-']) {
        return Some(if units.is_negative() {
            indentation.saturating_sub(units.unsigned_abs())
        } else {
            indentation.saturating_add(units.unsigned_abs())
        });
    }
    Some(units.max(0).unsigned_abs())
}

/// Parse the prefix accepted by `a2roffsu(value, SCALE_EN)`, then resolve it
/// to terminal cells. Unlike mdoc's `a2width()`, the man formatter accepts a
/// numeric prefix even when a trailing byte remains; an unrecognised suffix
/// keeps the default `n` unit.
fn terminal_signed_roff_en_prefix(value: &str) -> Option<isize> {
    let mut numeric = None;
    for end in value
        .char_indices()
        .map(|(index, _)| index)
        .skip(1)
        .chain(std::iter::once(value.len()))
    {
        if let Ok(scale) = value[..end].parse::<f64>()
            && scale.is_finite()
        {
            numeric = Some((end, scale));
        }
    }
    let (end, scale) = numeric?;
    let unit = value[end..].chars().next();
    let multiplier = match unit {
        Some('c') => 240.0 / 2.54,
        Some('i') => 240.0,
        Some('f') => 65_536.0,
        Some('M') => 0.24,
        Some('m' | 'n') => 24.0,
        Some('P' | 'v') => 40.0,
        Some('p') => 10.0 / 3.0,
        Some('u') => 1.0,
        Some(_) | None => 24.0,
    };
    terminal_hen(scale, multiplier)
}

fn terminal_hen(scale: f64, multiplier: f64) -> Option<isize> {
    let basic = (scale * multiplier).trunc();
    if !basic.is_finite() {
        return None;
    }
    // Finite values are clamped to the target range before reproducing C's
    // truncating conversion from scaled layout units.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let basic = basic.clamp(isize::MIN as f64, isize::MAX as f64) as isize;
    Some(if basic >= 0 {
        basic.saturating_add(11) / 24
    } else {
        -(basic.saturating_abs().saturating_add(11) / 24)
    })
}

/// Convert an mdoc `Bl` layout field the same way libmandoc's terminal
/// `a2width()` does.  It accepts a complete roff scale, rounds it in the
/// terminal's 24-basic-unit grid, and deliberately falls back to the visible
/// width of malformed or suffix-bearing input such as `1cx` and `xxx`.
fn terminal_mdoc_a2width(value: &str) -> isize {
    let Some(unit) = value.chars().last() else {
        return 0;
    };
    let number = &value[..value.len().saturating_sub(unit.len_utf8())];
    let Some(multiplier) = (match unit {
        'c' => Some(240.0 / 2.54),
        'i' => Some(240.0),
        'f' => Some(65_536.0),
        'M' => Some(0.24),
        'm' | 'n' => Some(24.0),
        'P' | 'v' => Some(40.0),
        'p' => Some(10.0 / 3.0),
        'u' => Some(1.0),
        _ => None,
    }) else {
        return isize::try_from(display_width(value)).unwrap_or(isize::MAX);
    };
    let Ok(scale) = number.parse::<f64>() else {
        return isize::try_from(display_width(value)).unwrap_or(isize::MAX);
    };
    terminal_hen(scale, multiplier)
        .unwrap_or_else(|| isize::try_from(display_width(value)).unwrap_or(isize::MAX))
}

/// Resolve roff's one-line temporary indentation. Signed forms are relative
/// to the current structural field; an unsigned value is an absolute terminal
/// column. The device clamps a request at column 72, except that an already
/// wider enclosing structural field is never pulled back to the clamp.
fn terminal_temporary_indent_target(value: &str, indentation: usize) -> Option<usize> {
    let value = value.trim();
    let units = terminal_signed_layout_units(value)?;
    let relative = value.starts_with(['+', '-']);
    let target = if relative {
        if units.is_negative() {
            indentation.saturating_sub(units.unsigned_abs())
        } else {
            indentation.saturating_add(units.unsigned_abs())
        }
    } else {
        units.max(0).unsigned_abs()
    };
    Some(target.min(indentation.max(72)))
}

/// Convert roff's vertical scaled units to terminal line spans. This mirrors
/// libmandoc's `term_vspan()`: the terminal's basic unit is one fortieth of a
/// line, while centimetres, inches, picas, points, ens, and ems retain the
/// device's fixed conversion factors.
#[allow(clippy::cast_possible_truncation)] // Match C's deliberate cast after the 0.4995 rounding offset.
fn terminal_vertical_span(value: &str) -> Option<isize> {
    let value = value.trim();
    let numeric_end = value
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(value.len()))
        .filter_map(|end| value[..end].parse::<f64>().ok().map(|number| (end, number)))
        .next_back()?;
    let (numeric_end, number) = numeric_end;
    let factor = match value[numeric_end..].chars().next() {
        Some('u') => 1.0 / 40.0,
        Some('c') => 6.0 / 2.54,
        Some('f') => 65_536.0 / 40.0,
        Some('i') => 6.0,
        Some('M') => 0.006,
        Some('m' | 'n') => 0.6,
        Some('P' | 'v') => 1.0,
        Some('p') => 1.0 / 12.0,
        _ => 1.0,
    };
    let scaled = number * factor;
    let rounded = if scaled.is_sign_positive() {
        (scaled + 0.4995) as isize
    } else {
        (scaled - 0.4995) as isize
    };
    Some(if rounded < 66 { rounded } else { 1 })
}

fn append_terminal_indentation(
    output: &mut String,
    indentation: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    if indentation > 0 {
        append(output, &" ".repeat(indentation), maximum)?;
    }
    Ok(())
}

/// Emit the physical lines requested by roff's `.sp`, including its final
/// line break. A zero-height scaled span still owns that one break; positive
/// spans add one blank line per terminal vertical unit. Negative spans defer
/// their effect: the reference renderer suppresses the next vertical spaces
/// rather than retracting output that has already been flushed.
fn append_terminal_vertical_space(
    output: &mut String,
    span: isize,
    maximum: usize,
) -> Result<(), RenderError> {
    if output.is_empty() {
        return Ok(());
    }
    if span.is_negative() {
        for _ in 0..span.unsigned_abs() {
            mark_terminal_vertical_skip(output);
        }
        if !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    let requested = span.unsigned_abs();
    let emitted = (0..requested)
        .filter(|_| !take_terminal_vertical_skip(output))
        .count();
    let required = emitted.saturating_add(1);
    let trailing = output
        .chars()
        .rev()
        .take_while(|character| *character == '\n')
        .count();
    // `term_vspace()` is cumulative once an earlier vertical request has
    // completed the current physical line.  In particular, two adjacent
    // `.sp` requests produce two blank device lines rather than sharing one
    // already-present separator.  The first request still owns its terminal
    // line break below, which is why a text line starts at two newlines.
    if trailing >= 2 {
        for _ in 0..emitted {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    for _ in trailing..required {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Consume one pending negative `.sp` adjustment, if any.  The markers live
/// immediately before the pending physical line break, leaving all ordinary
/// terminal layout predicates (`ends_with('\\n')`) unchanged.
fn take_terminal_vertical_skip(output: &mut String) -> bool {
    let newline_start = output.trim_end_matches('\n').len();
    let prefix = &output[..newline_start];
    if prefix.ends_with(TERMINAL_VERTICAL_SKIP_MARKER) {
        let marker_start = newline_start - TERMINAL_VERTICAL_SKIP_MARKER.len_utf8();
        output.drain(marker_start..newline_start);
        true
    } else {
        false
    }
}

fn mark_terminal_vertical_skip(output: &mut String) {
    let newline_start = output.trim_end_matches('\n').len();
    output.insert(newline_start, TERMINAL_VERTICAL_SKIP_MARKER);
}

fn mark_terminal_table_vertical_skip(output: &mut String) {
    let newline_start = output.trim_end_matches('\n').len();
    output.insert(newline_start, TERMINAL_TABLE_VERTICAL_SKIP_MARKER);
}

fn take_terminal_table_vertical_skip(output: &mut String) -> bool {
    take_terminal_table_vertical_skips(output) != 0
}

fn take_terminal_table_vertical_skips(output: &mut String) -> usize {
    let newline_start = output.trim_end_matches('\n').len();
    let marker_width = TERMINAL_TABLE_VERTICAL_SKIP_MARKER.len_utf8();
    let mut marker_start = newline_start;
    let mut count = 0_usize;
    while marker_start >= marker_width
        && output[..marker_start].ends_with(TERMINAL_TABLE_VERTICAL_SKIP_MARKER)
    {
        marker_start -= marker_width;
        count += 1;
    }
    output.drain(marker_start..newline_start);
    count
}

/// Start the next rendered phrase on a roff `.ti` temporary column. The
/// marker remains private until `wrap_terminal_output`, where only that
/// phrase's first visual line receives the requested column.
fn append_terminal_temporary_indent(
    output: &mut String,
    target: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    append(
        output,
        &TERMINAL_TEMPORARY_INDENT_MARKER.to_string(),
        maximum,
    )?;
    append(output, &target.to_string(), maximum)?;
    append(
        output,
        &TERMINAL_TEMPORARY_INDENT_MARKER.to_string(),
        maximum,
    )
}

/// Start the next rendered phrase in a man hanging-paragraph field. Unlike
/// `.ti`, the current line retains its normal structural indentation while
/// every wrapped continuation uses the encoded target column.
fn append_terminal_hanging_indent(
    output: &mut String,
    continuation: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    append(output, &TERMINAL_HANGING_INDENT_MARKER.to_string(), maximum)?;
    append(output, &continuation.to_string(), maximum)?;
    append(output, &TERMINAL_HANGING_INDENT_MARKER.to_string(), maximum)
}

/// Give the current device line a distinct wrap continuation field.
///
/// The marker parser intentionally accepts hanging fields only at a physical
/// line's beginning. A `Bk` begins after already-rendered synopsis words, so
/// prepend rather than append the private marker before the line is wrapped.
fn mark_terminal_hanging_indent(output: &mut String, continuation: usize) {
    let line_start = output.rfind('\n').map_or(0, |index| index + 1);
    output.insert_str(
        line_start,
        &format!("{TERMINAL_HANGING_INDENT_MARKER}{continuation}{TERMINAL_HANGING_INDENT_MARKER}"),
    );
}

/// Prefix each visible source line of a centered display with the renderer's
/// private centering marker.  Rendering the Body into its own buffer first
/// lets ordinary inline and block rules remain unchanged while the final
/// width pass sees the same device state on every physical display line.
fn append_terminal_centered_lines(
    output: &mut String,
    centered: &str,
    maximum: usize,
) -> Result<(), RenderError> {
    for (index, line) in centered.split('\n').enumerate() {
        if index > 0 {
            append(output, "\n", maximum)?;
        }
        if !line.is_empty() {
            append(output, &TERMINAL_CENTER_MARKER.to_string(), maximum)?;
        }
        append(output, line, maximum)?;
    }
    Ok(())
}

/// Render the text children structurally attached to roff's `.ce` and `.rj`
/// requests.  They are presentation-only requests: the first child is their
/// line count, each remaining child is a physical no-fill line, and ordinary
/// prose resumes after the requested count.  The parser intentionally retains
/// both the request argument and the owned source texts for AST compatibility.
fn render_terminal_adjusted_input_lines(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if terminal_has_visible_output(output) && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    let marker = if node.macro_name() == Some("rj") {
        TERMINAL_RIGHT_MARKER
    } else {
        TERMINAL_CENTER_MARKER
    };
    // `man.rs` already bounds attached text to the normalized positive count,
    // so skipping the count child here also correctly handles a recovered
    // empty request without producing a phantom terminal line.
    for child in node.children().skip(1) {
        let Some(text) = child.text() else {
            continue;
        };
        append(output, &marker.to_string(), maximum)?;
        append(output, &TERMINAL_NO_WRAP_MARKER.to_string(), maximum)?;
        // `rj` moves text to the device margin.  Centered input remains in
        // the enclosing field, matching term.c's distinct offset behavior.
        if node.macro_name() != Some("rj") {
            append(output, &" ".repeat(indentation), maximum)?;
        }
        let rendered =
            render_terminal_visible_text_with_font(text, format, limits, terminal_text_font(child));
        append(output, rendered.trim_end(), maximum)?;
        append(output, "\n", maximum)?;
    }
    Ok(())
}

fn append_terminal_text(
    output: &mut String,
    text: &str,
    layout: TerminalTextLayout,
    indentation: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    let break_replacement;
    let text = if text.contains(TERMINAL_PENDING_LINE_BREAK_MARKER) {
        break_replacement = format!("\n{}", " ".repeat(indentation));
        text.replace(TERMINAL_PENDING_LINE_BREAK_MARKER, &break_replacement)
    } else {
        text.to_owned()
    };
    let spacing_disabled = terminal_spacing_disabled(output);
    let visible_output = terminal_has_visible_output(output);
    let pending_special_indentation = output.ends_with([
        TERMINAL_TEMPORARY_INDENT_MARKER,
        TERMINAL_HANGING_INDENT_MARKER,
        TERMINAL_LINE_LENGTH_MARKER,
    ]);
    let empty_word = output.ends_with(TERMINAL_EMPTY_WORD_MARKER);
    if empty_word {
        let _ = output.pop();
    }
    let force_separator = output.ends_with(TERMINAL_FORCE_SEPARATOR_MARKER);
    if force_separator {
        let _ = output.pop();
    }
    let continue_source_line = output.ends_with(TERMINAL_CONTINUE_SOURCE_LINE_MARKER);
    if continue_source_line {
        let _ = output.pop();
    }
    let follows_no_fill_line = output
        .rsplit('\n')
        .next()
        .is_some_and(|line| line.starts_with(TERMINAL_NO_WRAP_MARKER));
    let attach_previous = output.ends_with(TERMINAL_ATTACH_NEXT_MARKER);
    if attach_previous {
        let _ = output.pop();
    }
    let mut pending_sentence = output.ends_with(TERMINAL_SENTENCE_PENDING_MARKER);
    if pending_sentence {
        let _ = output.pop();
    }
    let literal_punctuation = output.ends_with(TERMINAL_LITERAL_PUNCTUATION_MARKER);
    if literal_punctuation {
        let _ = output.pop();
    }
    if !attach_previous
        && !continue_source_line
        && !pending_special_indentation
        && (layout.line_start
            || (follows_no_fill_line && !layout.no_fill && !layout.no_fill_continuation))
        && visible_output
        && !output.ends_with('\n')
    {
        pending_sentence = false;
        append(output, "\n", maximum)?;
    } else if attach_previous || matches!(layout.join, TerminalJoin::Attach) {
        if output.ends_with(' ') {
            let _ = output.pop();
        }
    } else if empty_word && !output.is_empty() && !output.ends_with('\n') {
        // `Eo`/`Ec` can be a zero-width word. Its preceding separator was
        // already emitted, but the following visible word must receive its
        // own separator as well.
        append(
            output,
            &format!(" {TERMINAL_SENTENCE_SPACE_MARKER} "),
            maximum,
        )?;
    } else if (force_separator || continue_source_line)
        && !output.is_empty()
        && !output.ends_with('\n')
    {
        let separator = if pending_sentence {
            format!(" {TERMINAL_SENTENCE_SPACE_MARKER} ")
        } else {
            " ".to_owned()
        };
        append(output, &separator, maximum)?;
    } else if spacing_disabled {
        // `.Sm off` suppresses only ordinary word separation; explicit line
        // starts, parsed attachments, and structural field breaks above keep
        // their own terminal semantics.
    } else if !pending_special_indentation
        && visible_output
        // Literal punctuation is not layout state.  In particular, a roff
        // translation can leave visible `<<` at the end of a text node; the
        // next source phrase still needs its ordinary fill separator.  Only
        // the parser-informed private attachment marker denotes an opening
        // delimiter that owns the next word.
        && !output.ends_with([' ', '\n'])
    {
        let separator = if pending_sentence
            || output.chars().next_back().is_some_and(|character| {
                !literal_punctuation && matches!(character, '.' | '!' | '?')
            }) {
            " \u{1b} "
        } else {
            " "
        };
        append(output, separator, maximum)?;
    }
    let at_line_start = pending_special_indentation || !visible_output || output.ends_with('\n');
    if matches!(layout.tabs, TerminalTabLayout::PhysicalLiteral) {
        mark_terminal_line(output, TERMINAL_LITERAL_TAB_MARKER);
    }
    if layout.no_fill {
        mark_terminal_line(output, TERMINAL_NO_WRAP_MARKER);
    } else if layout.keep_spacing {
        mark_terminal_line(output, TERMINAL_KEEP_SPACING_MARKER);
    }
    if at_line_start {
        append_terminal_indentation(output, indentation, maximum)?;
    }
    append(output, &text, maximum)?;
    if layout.sentence_end || (pending_sentence && matches!(layout.join, TerminalJoin::Attach)) {
        append(
            output,
            &TERMINAL_SENTENCE_PENDING_MARKER.to_string(),
            maximum,
        )?;
    }
    if layout.literal_punctuation
        || (literal_punctuation && matches!(layout.join, TerminalJoin::Attach))
    {
        append(
            output,
            &TERMINAL_LITERAL_PUNCTUATION_MARKER.to_string(),
            maximum,
        )?;
    }
    Ok(())
}

fn terminal_has_visible_text(node: NodeRef<'_>, format: RenderFormat, limits: &Limits) -> bool {
    let mut text = String::new();
    collect_terminal_text(node, format, limits, &mut text);
    !text.is_empty()
}

/// A tagged man field cannot share its tag line with Body content after an
/// explicit terminal break.  The Body can still contain visible prose later,
/// but its initial `.sp`/`.br` has already completed the tag's device line.
fn terminal_body_starts_with_break(body: NodeRef<'_>) -> bool {
    body.children()
        .find(|child| !child.flags().no_print)
        .is_some_and(|child| matches!(child.macro_name(), Some("sp" | "br" | "PP" | "LP" | "Pp")))
}

/// `PD` owns no terminal glyphs, but it is still a physical body boundary
/// when it appears between a section heading and the next nested section.
fn terminal_has_pd_control(node: NodeRef<'_>) -> bool {
    node.macro_name() == Some("PD") || node.children().any(terminal_has_pd_control)
}

fn mark_terminal_attach_next(output: &mut String, maximum: usize) -> Result<(), RenderError> {
    if !output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
        append(output, &TERMINAL_ATTACH_NEXT_MARKER.to_string(), maximum)?;
    }
    Ok(())
}

/// Record a source-order `.ta` request on its own private terminal line.
///
/// Roff requests arrive at physical-line boundaries.  Keeping the state
/// marker standalone makes the next source line begin normally while the
/// width pass can remove the marker without manufacturing a blank output
/// line.  Individual arguments are already scanner-normalized AST text and
/// cannot contain the unit separator used by this bounded private encoding.
fn append_terminal_tab_stops_request(
    node: NodeRef<'_>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let request = node
        .children()
        .filter_map(NodeRef::text)
        .collect::<Vec<_>>()
        .join("\u{1f}");
    append_terminal_tab_stops_control(output, &request, maximum)
}

fn append_terminal_tab_stops_control(
    output: &mut String,
    request: &str,
    maximum: usize,
) -> Result<(), RenderError> {
    if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    append(output, &TERMINAL_TAB_STOPS_MARKER.to_string(), maximum)?;
    append(output, request, maximum)?;
    append(output, &TERMINAL_TAB_STOPS_MARKER.to_string(), maximum)?;
    append(output, "\n", maximum)
}

fn terminal_tab_stop_request(line: &str) -> Option<&str> {
    line.strip_prefix(TERMINAL_TAB_STOPS_MARKER)?
        .strip_suffix(TERMINAL_TAB_STOPS_MARKER)
}

fn terminal_apply_tab_stop_request(tab_stops: &mut TerminalTabStops, request: &str) {
    *tab_stops = TerminalTabStops {
        configured: true,
        ..TerminalTabStops::default()
    };
    let mut periodic = false;
    for argument in request.split('\u{1f}') {
        if argument == "T" {
            periodic = true;
            continue;
        }
        let Some(width) = terminal_signed_roff_en_prefix(argument) else {
            continue;
        };
        let width = width.max(0).unsigned_abs();
        let positions = if periodic {
            &mut tab_stops.periodic
        } else {
            &mut tab_stops.absolute
        };
        let position = if argument.starts_with('+') {
            positions.last().copied().unwrap_or(0).saturating_add(width)
        } else {
            width
        };
        positions.push(position);
    }
}

fn terminal_tab_next(tab_stops: &TerminalTabStops, previous: usize) -> usize {
    if let Some(position) = tab_stops
        .absolute
        .iter()
        .copied()
        .find(|position| previous < *position)
    {
        return position;
    }
    if tab_stops.periodic.is_empty() {
        return previous;
    }
    let cycle = *tab_stops.absolute.last().unwrap_or(&0);
    let period = *tab_stops.periodic.last().unwrap_or(&0);
    if period == 0 {
        return previous;
    }
    let mut base = cycle;
    while base.saturating_add(period) <= previous {
        base = base.saturating_add(period);
    }
    for position in &tab_stops.periodic {
        let position = base.saturating_add(*position);
        if previous < position {
            return position;
        }
    }
    previous
}

fn mark_terminal_force_separator(output: &mut String, maximum: usize) -> Result<(), RenderError> {
    if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
        let _ = output.pop();
    }
    if !output.ends_with(TERMINAL_FORCE_SEPARATOR_MARKER) {
        append(
            output,
            &TERMINAL_FORCE_SEPARATOR_MARKER.to_string(),
            maximum,
        )?;
    }
    Ok(())
}

/// `.Sm off` suppresses ordinary mdoc word spacing, but a later physical
/// phrase still observes the preceding sentence boundary.  Preserve that
/// narrow terminal state before forcing the source-line separator.
fn mark_terminal_force_separator_after_sentence(
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
        let _ = output.pop();
    }
    let literal_punctuation = output.ends_with(TERMINAL_LITERAL_PUNCTUATION_MARKER);
    if !literal_punctuation
        && !output.ends_with(TERMINAL_SENTENCE_PENDING_MARKER)
        && output.ends_with(['.', '!', '?'])
    {
        append(
            output,
            &TERMINAL_SENTENCE_PENDING_MARKER.to_string(),
            maximum,
        )?;
    }
    if !output.ends_with(TERMINAL_FORCE_SEPARATOR_MARKER) {
        append(
            output,
            &TERMINAL_FORCE_SEPARATOR_MARKER.to_string(),
            maximum,
        )?;
    }
    Ok(())
}

fn append_terminal_empty_word(
    output: &mut String,
    indentation: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    let attached = output.ends_with(TERMINAL_ATTACH_NEXT_MARKER);
    append_terminal_text(
        output,
        "",
        TerminalTextLayout::default(),
        indentation,
        maximum,
    )?;
    let marker = if attached {
        TERMINAL_FORCE_SEPARATOR_MARKER
    } else {
        TERMINAL_EMPTY_WORD_MARKER
    };
    append(output, &marker.to_string(), maximum)
}

fn terminal_sentence_terminator(text: &str) -> bool {
    text.trim_end()
        .chars()
        .next_back()
        .is_some_and(|character| {
            matches!(character, '.' | '!' | '?' | '"' | '\'' | ')' | ']' | '}')
        })
}

fn mark_terminal_line(output: &mut String, marker: char) {
    let line_start = output.rfind('\n').map_or(0, |index| index + 1);
    // No-fill literal text needs both the no-wrap and literal-tab markers.
    // They are prepended in a fixed order, so a later text node continuing
    // that same physical line must recognise either marker rather than
    // inserting a duplicate behind the first one.
    let already_marked = output[line_start..]
        .chars()
        .take_while(|character| {
            matches!(
                *character,
                TERMINAL_NO_WRAP_MARKER
                    | TERMINAL_LITERAL_TAB_MARKER
                    | TERMINAL_KEEP_SPACING_MARKER
            )
        })
        .any(|character| character == marker);
    if !already_marked {
        output.insert(line_start, marker);
    }
}

/// Prefix the pending raw terminal line with its non-default `.ll` state.
/// The paired encoding makes the state unambiguous beside other one-byte
/// layout markers and is removed before caller-visible output is returned.
fn mark_terminal_line_length(
    output: &mut String,
    state: TerminalLineLength,
    maximum: usize,
) -> Result<(), RenderError> {
    let encoded = match state {
        TerminalLineLength::Default => {
            format!("{TERMINAL_LINE_LENGTH_MARKER}D{TERMINAL_LINE_LENGTH_MARKER}")
        }
        TerminalLineLength::Absolute(value) => {
            format!("{TERMINAL_LINE_LENGTH_MARKER}A{value}{TERMINAL_LINE_LENGTH_MARKER}")
        }
        TerminalLineLength::Relative(value) => {
            format!("{TERMINAL_LINE_LENGTH_MARKER}R{value}{TERMINAL_LINE_LENGTH_MARKER}")
        }
    };
    let line_start = output.rfind('\n').map_or(0, |index| index + 1);
    let Some(relative_start) = output[line_start..].find(TERMINAL_LINE_LENGTH_MARKER) else {
        if matches!(state, TerminalLineLength::Default) {
            return Ok(());
        }
        if output.len().saturating_add(encoded.len()) > maximum {
            return Err(RenderError {
                kind: RenderErrorKind::OutputLimit,
                message: format!("rendered output exceeds {maximum} bytes").into(),
            });
        }
        output.insert_str(line_start, &encoded);
        return Ok(());
    };
    let marker_start = line_start + relative_start;
    let payload_start = marker_start + TERMINAL_LINE_LENGTH_MARKER.len_utf8();
    let Some(relative_end) = output[payload_start..].find(TERMINAL_LINE_LENGTH_MARKER) else {
        // An incomplete private marker can only arise while handling a
        // bounded-output error. It is discarded rather than leaked.
        output.truncate(marker_start);
        return Ok(());
    };
    let marker_end = payload_start + relative_end + TERMINAL_LINE_LENGTH_MARKER.len_utf8();
    let replaced = marker_end.saturating_sub(marker_start);
    let next_len = output
        .len()
        .saturating_sub(replaced)
        .saturating_add(encoded.len());
    if next_len > maximum {
        return Err(RenderError {
            kind: RenderErrorKind::OutputLimit,
            message: format!("rendered output exceeds {maximum} bytes").into(),
        });
    }
    output.replace_range(marker_start..marker_end, &encoded);
    Ok(())
}

fn terminal_spacing_disabled(output: &str) -> bool {
    output.starts_with(TERMINAL_NO_SPACE_MARKER)
}

fn terminal_has_visible_output(output: &str) -> bool {
    !output.is_empty() && !output.chars().eq(std::iter::once(TERMINAL_NO_SPACE_MARKER))
}

/// Apply mdoc's stateful spacing request. Valid `on` and `off` selectors are
/// retained as the Element's sole child. An argument-less request toggles the
/// state; parser recovery relinks an invalid same-line word after an empty
/// Element, which deliberately leaves the current state unchanged.
fn terminal_apply_mdoc_spacing(
    node: NodeRef<'_>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let requested = node
        .children()
        .find_map(NodeRef::text)
        .and_then(|value| match value {
            "on" => Some(true),
            "off" => Some(false),
            _ => None,
        });
    let invalid_argument = terminal_mdoc_sm_has_relinked_invalid_argument(node);
    // Both a bare request and a recovered invalid selector take the device's
    // toggle path. The invalid spelling itself is relinked as ordinary text;
    // it does not leave a separate renderer-only spacing mode behind.
    let enabled = requested.unwrap_or_else(|| terminal_spacing_disabled(output));
    if enabled {
        if terminal_spacing_disabled(output) {
            output.drain(..TERMINAL_NO_SPACE_MARKER.len_utf8());
        }
    } else if !terminal_spacing_disabled(output) {
        if output
            .len()
            .saturating_add(TERMINAL_NO_SPACE_MARKER.len_utf8())
            > maximum
        {
            return Err(RenderError {
                kind: RenderErrorKind::OutputLimit,
                message: format!("rendered output exceeds {maximum} bytes").into(),
            });
        }
        output.insert(0, TERMINAL_NO_SPACE_MARKER);
    }
    // Recovery leaves an invalid `.Sm bad` argument as the request's
    // immediate text sibling.  The request itself is invisible, but it still
    // closes the preceding filled phrase; keep the recovered word separate.
    if invalid_argument && terminal_has_visible_output(output) {
        mark_terminal_force_separator(output, maximum)?;
    } else if requested.is_some()
        && terminal_mdoc_sm_has_relinked_valid_argument(node)
        && terminal_has_visible_output(output)
    {
        // A valid selector's surplus words are relinked after the invisible
        // request. The first one starts the request's visible phrase, while
        // its following source line remains subject to `.Sm off`.
        mark_terminal_force_separator(output, maximum)?;
    }
    Ok(())
}

fn terminal_mdoc_sm_has_relinked_invalid_argument(node: NodeRef<'_>) -> bool {
    if node
        .children()
        .find_map(NodeRef::text)
        .is_some_and(|argument| matches!(argument, "on" | "off"))
    {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    let Some(next) = parent
        .children()
        .skip_while(|sibling| sibling.id() != node.id())
        .nth(1)
    else {
        return false;
    };
    next.text().is_some()
        && node
            .source_position()
            .zip(next.source_position())
            .is_some_and(|(request, argument)| request.line == argument.line)
}

fn terminal_mdoc_sm_has_relinked_valid_argument(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let Some(next) = parent
        .children()
        .skip_while(|sibling| sibling.id() != node.id())
        .nth(1)
    else {
        return false;
    };
    next.text().is_some()
        && node
            .source_position()
            .zip(next.source_position())
            .is_some_and(|(request, argument)| request.line == argument.line)
}

fn terminal_mdoc_sm_relinked_valid_argument(node: NodeRef<'_>) -> bool {
    terminal_mdoc_sm_relink_before(node) == Some(TerminalMdocSmRelink::Valid)
}

fn terminal_mdoc_sm_relinked_invalid_argument(node: NodeRef<'_>) -> bool {
    terminal_mdoc_sm_relink_before(node) == Some(TerminalMdocSmRelink::Invalid)
}

fn terminal_mdoc_sm_relinked_argument_precedes(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        .last()
        .is_some_and(|previous| terminal_mdoc_sm_relink_before(previous).is_some())
}

/// Classify a word the parser detached from a same-line `.Sm` request.
///
/// The valid and invalid paths look similar in the public AST, but their
/// terminal spacing differs: valid `off two` retains `two` as the first
/// no-space phrase, while recovery for `bad two` resumes ordinary word flow.
fn terminal_mdoc_sm_relink_before(node: NodeRef<'_>) -> Option<TerminalMdocSmRelink> {
    node.text()?;
    let target = node.source_position()?;
    let parent = node.parent()?;
    let preceding = parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        .collect::<Vec<_>>();
    for sibling in preceding.into_iter().rev() {
        let Some(position) = sibling.source_position() else {
            continue;
        };
        if position.line != target.line {
            break;
        }
        if sibling.kind() != NodeKind::Element || sibling.macro_name() != Some("Sm") {
            continue;
        }
        return Some(
            if sibling
                .children()
                .find_map(NodeRef::text)
                .is_some_and(|argument| matches!(argument, "on" | "off"))
            {
                TerminalMdocSmRelink::Valid
            } else {
                TerminalMdocSmRelink::Invalid
            },
        );
    }
    None
}

fn terminal_mdoc_sm_starts_new_source_phrase(node: NodeRef<'_>) -> bool {
    if !node.flags().line_start {
        return false;
    }
    match node.kind() {
        NodeKind::Text => true,
        NodeKind::Element => !matches!(
            node.macro_name(),
            Some("Pp" | "PP" | "LP" | "sp" | "br" | "Sm" | "Tg" | "Es" | "ft" | "po" | "ll" | "in")
        ),
        // An `Op` block at an input-line boundary begins a visible optional
        // phrase. Under `.Sm off` its opening bracket still receives the
        // one source-phrase separator, while nested same-line options do
        // not manufacture one.
        NodeKind::Block => node.macro_name() == Some("Op"),
        _ => false,
    }
}

/// Return the mdoc word-spacing state effective at `node`'s source position.
///
/// Terminal rendering normally carries `.Sm` state in its private output
/// buffer.  Some presentation paths first collect an enclosure or a styled
/// macro into a separate string, though, so that buffer is deliberately not
/// available there. Replaying the tiny state machine from the immutable tree
/// keeps those nested phrases faithful without making the public AST carry a
/// renderer-only control bit.
fn terminal_mdoc_spacing_disabled_before(node: NodeRef<'_>) -> bool {
    let Some(target) = node.source_position() else {
        return false;
    };
    let mut root = node;
    while let Some(parent) = root.parent() {
        root = parent;
    }

    let mut spacing_enabled = true;
    let mut pending = vec![root];
    while let Some(current) = pending.pop() {
        if current.kind() == NodeKind::Element
            && current.macro_name() == Some("Sm")
            && current
                .source_position()
                .is_some_and(|position| terminal_source_position_precedes(position, target))
        {
            match current.children().find_map(NodeRef::text) {
                Some("on") => {
                    spacing_enabled = true;
                }
                Some("off") => {
                    spacing_enabled = false;
                }
                None => {
                    spacing_enabled = !spacing_enabled;
                }
                Some(_) => {}
            }
        }
        let children = current.children().collect::<Vec<_>>();
        pending.extend(children.into_iter().rev());
    }
    !spacing_enabled
}

fn terminal_source_position_precedes(
    position: crate::SourcePosition,
    target: crate::SourcePosition,
) -> bool {
    (position.line, position.column) < (target.line, target.column)
}

fn collect_terminal_text(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    output: &mut String,
) {
    if node.flags().no_print || node.ancestors().any(|ancestor| ancestor.flags().no_print) {
        return;
    }
    if node.macro_name() == Some("PD") {
        if node.kind() == NodeKind::Block {
            for body in node
                .children()
                .filter(|child| child.kind() == NodeKind::Body)
            {
                collect_terminal_text(body, format, limits, output);
            }
        }
        return;
    }
    if let Some(closing) = terminal_embedded_quote_closing(node, format) {
        let font = if node
            .ancestors()
            .any(|ancestor| ancestor.macro_name() == Some("Bf"))
        {
            TerminalFont::Roman
        } else {
            terminal_inherited_font(node)
        };
        output.push_str(&render_terminal_font(closing, font));
        return;
    }
    if matches!(
        node.macro_name(),
        Some("Es" | "Sm" | "Tg" | "ft" | "po" | "ll" | "in" | "sp" | "br" | "ta")
    ) {
        return;
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Bf") {
        // A Bf Head is formatter configuration, not phrase text.  This
        // collector is reached from explicit enclosures and list terms as
        // well as the top-level walker, so mirror the terminal dispatch here
        // instead of leaking its normalized `Em`/`Li`/`Sy` argument.
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            collect_terminal_text(body, format, limits, output);
        }
        return;
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Eo") {
        // Collection is used inside a surrounding quote/list phrase, where
        // the top-level Eo dispatcher is intentionally bypassed.  Preserve
        // the explicit Head/Body/Tail attachment here as well: Eo's opening
        // delimiter attaches to its Body, and its recovered Ec Tail attaches
        // back to that Body without also swallowing the following phrase.
        let mut tail = None;
        let mut has_head_or_body = false;
        let mut embedded_outer_closer = false;
        let has_visible_tail = node.children().any(|child| {
            child.kind() == NodeKind::Tail && terminal_has_visible_text(child, format, limits)
        });
        for child in node.children() {
            match child.kind() {
                NodeKind::Head => {
                    let visible = terminal_has_visible_text(child, format, limits);
                    has_head_or_body |= visible;
                    for nested in child.children() {
                        collect_terminal_text(nested, format, limits, output);
                    }
                    if visible {
                        output.push(TERMINAL_ATTACH_NEXT_MARKER);
                    }
                }
                NodeKind::Body => {
                    // A recovered `Bc` nested in Eo is represented as an
                    // empty `Body(Bo)` child. It emits the *outer* quote's
                    // closing bracket at this source point, but does not
                    // make an otherwise empty Eo own content for Ec-tail
                    // attachment purposes.
                    let has_embedded_closer = child
                        .children()
                        .any(|nested| terminal_embedded_quote_closing(nested, format).is_some());
                    let has_own_content = child.children().any(|nested| {
                        terminal_embedded_quote_closing(nested, format).is_none()
                            && terminal_has_visible_text(nested, format, limits)
                    });
                    if has_embedded_closer
                        && !has_head_or_body
                        && !has_own_content
                        && has_visible_tail
                    {
                        // An empty Eo survives only to close after the
                        // surrounding partial block. Preserve the blank
                        // before that outer closer, then attach Ec to it.
                        if !output.ends_with(' ') {
                            output.push(' ');
                        }
                        embedded_outer_closer = true;
                    }
                    has_head_or_body |= has_own_content;
                    for nested in child.children() {
                        collect_terminal_text(nested, format, limits, output);
                    }
                }
                NodeKind::Tail => tail = Some(child),
                _ => {}
            }
        }
        let has_tail = tail.is_some_and(|tail| terminal_has_visible_text(tail, format, limits));
        if let Some(tail) = tail.filter(|_| has_tail) {
            if has_head_or_body || embedded_outer_closer {
                output.push(TERMINAL_ATTACH_NEXT_MARKER);
            } else {
                // Eo may survive only as the owner of a late Ec after an
                // enclosing quote has already closed.  That closer starts a
                // new phrase; never inherit the enclosing quote's old
                // attachment marker.
                if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
                    let _ = output.pop();
                }
                if !output.ends_with(' ') {
                    output.push(' ');
                }
            }
            for nested in tail.children() {
                collect_terminal_text(nested, format, limits, output);
            }
        } else if has_head_or_body {
            if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
                let _ = output.pop();
            }
            // Unlike the top-level renderer, this collector returns one
            // already-assembled phrase.  Emit the separator directly so the
            // following collected text cannot consume Eo's old attachment.
            if !output.ends_with(' ') {
                output.push(' ');
            }
        } else {
            output.push(TERMINAL_EMPTY_WORD_MARKER);
        }
        return;
    }
    if node.kind() == NodeKind::Body
        && node.macro_name() == Some("Eo")
        && node
            .parent()
            .is_some_and(|parent| parent.macro_name() != Some("Eo"))
    {
        // When an Eo closes while another partial block owns the active
        // body, mandoc retains Ec as an Eo Body nested at that exact source
        // position rather than as the outer block's Tail.  It is still a
        // closing delimiter: attach it to the preceding phrase, but let the
        // following sibling receive its ordinary separator.
        if node
            .children()
            .any(|child| terminal_has_visible_text(child, format, limits))
        {
            output.push(TERMINAL_ATTACH_NEXT_MARKER);
            for child in node.children() {
                collect_terminal_text(child, format, limits, output);
            }
        } else {
            // A bare Ec has no delimiter payload. It closes Eo's attachment,
            // but it must not attach the next word to the preceding phrase.
            if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
                let _ = output.pop();
            }
            if !output.ends_with(' ') {
                output.push(' ');
            }
        }
        return;
    }
    if is_mdoc_description_block(node) {
        // A broken or explicitly enclosed `.Nd` can be collected through a
        // surrounding quote/list phrase instead of the normal top-level
        // block dispatcher.  Its Body alone omits the device's description
        // separator, so reproduce the small inline form here.
        let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
            return;
        };
        let mut description = String::new();
        collect_terminal_text(body, format, limits, &mut description);
        if !output.is_empty()
            && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
        {
            output.push(' ');
        }
        if matches!(format, RenderFormat::Utf8) {
            output.push('–');
        } else {
            output.push('-');
        }
        if !description.is_empty() {
            output.push(' ');
            output.push_str(&description);
        }
        return;
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Op")
        && let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body)
        && let Some((opening, closing)) = terminal_quote_delimiters(node, Some(body), format)
    {
        // A nested optional phrase is collected into its enclosing terminal
        // quote Body rather than walked through the top-level dispatcher.
        // Preserve its own brackets here; `.Sm off` still controls only the
        // gap before this source phrase, not the nested macro contents.
        if !terminal_mdoc_spacing_disabled_before(node)
            && !output.is_empty()
            && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
        {
            output.push(' ');
        }
        let opening = render_terminal_font(opening, terminal_inherited_font(node));
        let closing = if terminal_quote_has_embedded_closer(body, node.macro_name()) {
            String::new()
        } else {
            render_terminal_font(closing, terminal_inherited_font(node))
        };
        output.push_str(&opening);
        collect_terminal_text(body, format, limits, output);
        output.push_str(&closing);
        return;
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("En")
        && let Some(enclosure) = node.enclosure()
    {
        // The obsolete `Es` request stores its resolved delimiters on each
        // later `En` block.  These blocks are often collected as a phrase,
        // where walking only their Body would silently discard that state.
        if !terminal_mdoc_spacing_disabled_before(node)
            && !output.is_empty()
            && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
        {
            output.push(' ');
        }
        for leading in node
            .children()
            .filter(|child| child.kind() == NodeKind::Head || child.flags().delimiter_open)
        {
            collect_terminal_text(leading, format, limits, output);
        }
        output.push_str(&enclosure.opening);
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            collect_terminal_text(body, format, limits, output);
        }
        if let Some(closing) = &enclosure.closing {
            output.push_str(closing);
        }
        return;
    }
    if node.kind() == NodeKind::Block
        && let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body)
        && let Some((opening, closing)) = terminal_quote_delimiters(node, Some(body), format)
    {
        // Collection is also used for list terms and other partial syntax
        // regions that bypass the top-level block dispatcher.  Retain an
        // ordinary explicit quote scope here rather than flattening it to
        // its Body words; otherwise an `Ao` extended item head, for example,
        // loses its visible angle brackets.
        if !terminal_mdoc_spacing_disabled_before(node)
            && !output.is_empty()
            && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
        {
            output.push(' ');
        }
        let opening = render_terminal_font(opening, terminal_inherited_font(node));
        let closing = if terminal_quote_has_embedded_closer(body, node.macro_name()) {
            String::new()
        } else {
            render_terminal_font(closing, terminal_inherited_font(node))
        };
        output.push_str(&opening);
        collect_terminal_text(body, format, limits, output);
        output.push_str(&closing);
        for tail in node
            .children()
            .filter(|child| child.kind() == NodeKind::Tail)
        {
            collect_terminal_text(tail, format, limits, output);
        }
        return;
    }
    if node.kind() == NodeKind::Element && node.macro_name() == Some("OP") {
        output.push_str(&terminal_man_option(node, format, limits));
        return;
    }
    if node.kind() == NodeKind::Element && node.macro_name() == Some("Pf") {
        for child in node.children() {
            collect_terminal_text(child, format, limits, output);
        }
        if terminal_mdoc_prefix_attaches_to_following_token(node) {
            output.push(TERMINAL_ATTACH_NEXT_MARKER);
        }
        return;
    }
    if node.kind() == NodeKind::Element
        && let Some(fonts) = terminal_man_alternating_fonts(node.macro_name())
    {
        for (index, child) in node.children().enumerate() {
            let mut fragment = String::new();
            collect_terminal_semantic_text(
                child,
                format,
                limits,
                fonts[index % fonts.len()],
                &mut fragment,
            );
            output.push_str(&fragment);
        }
        return;
    }
    if node.kind() == NodeKind::Element
        && let Some(font) = match node.macro_name() {
            Some("B") => Some(TerminalFont::Bold),
            Some("I") => Some(TerminalFont::Italic),
            Some("R") => Some(TerminalFont::Roman),
            _ => None,
        }
    {
        collect_terminal_semantic_text(node, format, limits, font, output);
        return;
    }
    if node.kind() == NodeKind::Element && node.macro_name() == Some("Nm") {
        // Collection paths (notably an `Nd` nested in an explicit quote)
        // bypass the top-level Nm dispatcher.  Preserve Nm's bold base font
        // here instead of flattening its child text to ordinary prose.
        let mut phrase = String::new();
        collect_terminal_semantic_text(node, format, limits, TerminalFont::Bold, &mut phrase);
        if !phrase.is_empty() {
            if !terminal_mdoc_spacing_disabled_before(node)
                && !output.is_empty()
                && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
            {
                output.push(' ');
            }
            output.push_str(&phrase);
        }
        return;
    }
    if node.kind() == NodeKind::Element
        && let Some(font) = terminal_mdoc_element_font(node)
    {
        let mut phrase = String::new();
        collect_terminal_semantic_text(node, format, limits, font, &mut phrase);
        let empty_flag = node.macro_name() == Some("Fl") && node.children().next().is_none();
        if node.macro_name() == Some("Fl")
            && (phrase.is_empty() || node.children().next().is_some())
        {
            phrase.insert_str(0, &render_terminal_font("-", font));
        }
        if !phrase.is_empty() {
            if !terminal_mdoc_spacing_disabled_before(node)
                && !output.is_empty()
                && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
            {
                output.push(' ');
            }
            output.push_str(&phrase);
            if empty_flag && terminal_mdoc_empty_fl_attaches_to_following_macro(node) {
                output.push(TERMINAL_ATTACH_NEXT_MARKER);
            }
        }
        return;
    }
    if let Some(text) = node.text() {
        let sentence_boundary = node.flags().sentence_end
            && terminal_sentence_terminator(text)
            && terminal_mdoc_plain_text_sentence(node)
            && !node.flags().delimiter_close
            && terminal_next_visible_sibling(node).is_some_and(|next| {
                // A later explicit enclosure is still ordinary terminal
                // prose from the preceding plain sentence's perspective.
                // The collector otherwise flattens it before the final
                // layout call can see that transition.
                next.kind() == NodeKind::Text || next.macro_name() == Some("Ao")
            });
        if node.flags().delimiter_close
            || (terminal_closing_punctuation(text)
                && !node
                    .ancestors()
                    .any(|ancestor| ancestor.macro_name() == Some("Pf")))
        {
            if output.ends_with(' ') {
                let _ = output.pop();
            }
        } else if !output.is_empty()
            && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER])
            && !terminal_mdoc_spacing_disabled_before(node)
            // A parsed opening delimiter owns the following phrase's
            // adjacency. The collector is used for partial-block bodies,
            // where that parser flag has already been consumed into the
            // visible punctuation spelling.
            && !output.ends_with(['(', '[', '{', '<'])
        {
            output.push(' ');
        }
        output.push_str(&render_terminal_visible_text_with_font(
            text,
            format,
            limits,
            terminal_inherited_font(node),
        ));
        if node.flags().line_continuation && !text.ends_with("\\z\\c") {
            output.push(TERMINAL_ATTACH_NEXT_MARKER);
        }
        if sentence_boundary {
            // This collector supplies one assembled phrase to tag/list and
            // enclosure renderers. Preserve the device's sentence token
            // explicitly, since the final `append_terminal_text()` call no
            // longer sees the original node boundary.
            output.push(' ');
            output.push(TERMINAL_SENTENCE_SPACE_MARKER);
            output.push(' ');
        }
    }
    for child in node.children() {
        collect_terminal_text(child, format, limits, output);
    }
}

fn terminal_closing_punctuation(text: &str) -> bool {
    matches!(text, "." | "," | ";" | ":" | "!" | "?" | ")" | "]" | "}")
}

/// Render mdoc's fixed two-argument cross-reference form.  The parser keeps
/// its target and section as individual children for navigation; the terminal
/// device presents them as one `name(section)` phrase.
fn terminal_cross_reference(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> Option<String> {
    let mut arguments = node.children().filter(|child| !child.flags().no_print);
    let mut name = String::new();
    collect_terminal_text(arguments.next()?, format, limits, &mut name);
    if name.is_empty() {
        return None;
    }
    let mut section = String::new();
    let Some(section_argument) = arguments.next() else {
        return Some(name);
    };
    collect_terminal_text(section_argument, format, limits, &mut section);
    if section.is_empty() {
        Some(name)
    } else {
        Some(format!("{name}({section})"))
    }
}

/// Collect a SYNOPSIS `.Nm` Head.  Most of the head is a bold semantic name,
/// but a partial quote block can be nested inside it when the parser closes
/// the implicit Nm block around another mdoc macro.  The normal semantic
/// collector intentionally flattens syntax blocks; this presentation-only
/// path preserves the nested terminal delimiters without changing that AST.
fn collect_terminal_mdoc_synopsis_name_head(
    head: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    output: &mut String,
) {
    for child in head.children() {
        if child.kind() == NodeKind::Block
            && let Some(body) = child
                .children()
                .find(|nested| nested.kind() == NodeKind::Body)
            && let Some((opening, closing)) = terminal_quote_delimiters(child, Some(body), format)
        {
            if !output.is_empty() && !output.ends_with(' ') {
                output.push(' ');
            }
            output.push_str(&render_terminal_font(opening, TerminalFont::Bold));
            let mut contents = String::new();
            collect_terminal_semantic_text(body, format, limits, TerminalFont::Bold, &mut contents);
            output.push_str(&contents);
            output.push_str(&render_terminal_font(closing, TerminalFont::Bold));
        } else {
            collect_terminal_semantic_text(child, format, limits, TerminalFont::Bold, output);
        }
    }
}

/// Render mdoc's hyperlink form.  Its first argument is the URL; remaining
/// arguments are a human-readable label.  The terminal device displays the
/// label first in italic, followed by a Roman colon and the URL in bold.  A
/// delimiter parsed as a separate final label child belongs after the URL.
fn terminal_link(node: NodeRef<'_>, format: RenderFormat, limits: &Limits) -> Option<String> {
    let mut arguments = node.children().filter(|child| !child.flags().no_print);
    let target = arguments.next()?;
    let mut target_text = String::new();
    collect_terminal_semantic_text(target, format, limits, TerminalFont::Bold, &mut target_text);
    if target_text.is_empty() {
        return None;
    }

    let mut label_arguments = arguments.collect::<Vec<_>>();
    if label_arguments.is_empty() {
        return Some(target_text);
    }

    // `.Lk url label ,` tokenizes the comma as a delimiter child of the
    // label. `term.c` moves that delimiter after its rendered URL, while a
    // comma authored directly in a word (for example `label,`) stays within
    // the italic label as parsed.
    let delimiter = if label_arguments
        .last()
        .is_some_and(|argument| argument.flags().delimiter_close)
    {
        label_arguments.pop()
    } else {
        None
    };

    let mut label = String::new();
    for argument in label_arguments {
        collect_terminal_semantic_text(argument, format, limits, TerminalFont::Italic, &mut label);
    }
    if label.is_empty() {
        if let Some(delimiter) = delimiter {
            let mut trailing = String::new();
            collect_terminal_semantic_text(
                delimiter,
                format,
                limits,
                TerminalFont::Roman,
                &mut trailing,
            );
            target_text.push_str(&trailing);
        }
        return Some(target_text);
    }

    let mut rendered = format!("{label}: {target_text}");
    if let Some(delimiter) = delimiter {
        let mut trailing = String::new();
        collect_terminal_semantic_text(
            delimiter,
            format,
            limits,
            TerminalFont::Roman,
            &mut trailing,
        );
        rendered.push_str(&trailing);
    }
    Some(rendered)
}

/// Render an mdoc `Rs` bibliography block as one terminal reference.  The
/// parser has already normalized direct `%` fields into the reference order;
/// terminal presentation adds the package-specific author conjunction,
/// typography, separators, and final sentence punctuation.
fn render_terminal_reference_block(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if terminal_mdoc_section_named(node, "SEE ALSO")
        && terminal_has_visible_preceding_sibling(node, format, limits)
    {
        append_blank_line(output, maximum)?;
    }
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let fields = body
        .children()
        .filter(|child| !child.flags().no_print)
        .collect::<Vec<_>>();
    let has_journal = fields.iter().any(|field| field.macro_name() == Some("%J"));
    let mut fields_after_authors = Vec::new();
    let mut authors = Vec::new();
    let mut direct_prefix = Vec::new();
    for field in &fields {
        if field.macro_name() == Some("%A") {
            let mut author = String::new();
            collect_terminal_text(*field, format, limits, &mut author);
            if !author.is_empty() {
                authors.push(author);
            }
        } else if let Some(phrase) = terminal_reference_field(*field, format, limits, has_journal) {
            fields_after_authors.push(phrase);
        } else {
            let mut direct = String::new();
            if let Some(font) = terminal_mdoc_element_font(*field) {
                collect_terminal_semantic_text(*field, format, limits, font, &mut direct);
            } else {
                collect_terminal_text(*field, format, limits, &mut direct);
            }
            if !direct.is_empty() {
                direct_prefix.push(direct);
            }
        }
    }
    if direct_prefix.is_empty() && authors.is_empty() && fields_after_authors.is_empty() {
        return Ok(());
    }
    let mut reference = direct_prefix.join(" ");
    if !authors.is_empty() {
        if !reference.is_empty() {
            reference.push(' ');
        }
        reference.push_str(&terminal_reference_authors(&authors));
    }
    for phrase in &fields_after_authors {
        if !reference.is_empty() {
            reference.push_str(", ");
        }
        reference.push_str(phrase);
    }
    reference.push('.');
    append_terminal_text(
        output,
        &reference,
        TerminalTextLayout {
            sentence_end: true,
            ..TerminalTextLayout::default()
        },
        indentation,
        maximum,
    )
}

fn terminal_has_visible_preceding_sibling(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> bool {
    node.parent().is_some_and(|parent| {
        parent
            .children()
            .take_while(|sibling| sibling.id() != node.id())
            .any(|sibling| terminal_has_visible_text(sibling, format, limits))
    })
}

fn terminal_reference_authors(authors: &[String]) -> String {
    match authors {
        [] => String::new(),
        [author] => author.clone(),
        [first, second] => format!("{first} and {second}"),
        _ => {
            let mut output = authors[..authors.len() - 1].join(", ");
            output.push_str(", and ");
            output.push_str(authors.last().expect("nonempty author list"));
            output
        }
    }
}

fn terminal_reference_field(
    field: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    has_journal: bool,
) -> Option<String> {
    let macro_name = field.macro_name()?;
    if !matches!(
        macro_name,
        "%B" | "%C" | "%D" | "%I" | "%J" | "%N" | "%O" | "%P" | "%Q" | "%R" | "%T" | "%U" | "%V"
    ) {
        return None;
    }
    let mut value = String::new();
    let font = match macro_name {
        "%B" | "%I" | "%J" => Some(TerminalFont::Italic),
        "%T" if !has_journal => Some(TerminalFont::Italic),
        _ => None,
    };
    if let Some(font) = font {
        collect_terminal_semantic_text(field, format, limits, font, &mut value);
    } else {
        collect_terminal_text(field, format, limits, &mut value);
    }
    if value.is_empty() {
        return None;
    }
    if macro_name == "%T" && has_journal {
        let (open, close) = if matches!(format, RenderFormat::Utf8) {
            ("“", "”")
        } else {
            ("\"", "\"")
        };
        value = format!("{open}{value}{close}");
    }
    Some(value)
}

fn collect_terminal_inline_text(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    output: &mut String,
) {
    let children = node.children().collect::<Vec<_>>();
    for (index, child) in children.iter().copied().enumerate() {
        // `\c` is collected as the same private attachment marker used by
        // ordinary terminal flow.  A man font element applies its style only
        // after collecting all of its arguments, so consume that marker here
        // before introducing this helper's otherwise-normal inter-argument
        // separator; rendering the marker in bold would turn it into a
        // visible overstrike space.
        let attach_previous = output.ends_with(TERMINAL_ATTACH_NEXT_MARKER);
        if attach_previous {
            let _ = output.pop();
        }
        if index > 0 && !output.is_empty() && !attach_previous {
            let separator = if children[index - 1].separator_after() == Some(b'\t') {
                "\t"
            } else {
                " "
            };
            output.push_str(separator);
        }
        let mut fragment = String::new();
        collect_terminal_text(child, format, limits, &mut fragment);
        output.push_str(&fragment);
    }
}

/// Collect mdoc macro arguments with the macro's semantic font as their
/// initial terminal state. Source `\f` controls then switch away from and
/// back to that state, rather than being discarded or overriding the whole
/// phrase. This is distinct from ordinary prose, whose initial state is Roman.
fn collect_terminal_semantic_text(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    font: TerminalFont,
    output: &mut String,
) {
    if node.flags().no_print || node.ancestors().any(|ancestor| ancestor.flags().no_print) {
        return;
    }
    if node.macro_name() == Some("PD") {
        if node.kind() == NodeKind::Block {
            for body in node
                .children()
                .filter(|child| child.kind() == NodeKind::Body)
            {
                collect_terminal_semantic_text(body, format, limits, font, output);
            }
        }
        return;
    }
    if matches!(node.macro_name(), Some("Es" | "Sm" | "Tg")) {
        return;
    }
    if node.kind() == NodeKind::Element && node.macro_name() == Some("Pf") {
        for child in node.children() {
            collect_terminal_semantic_text(child, format, limits, font, output);
        }
        if terminal_mdoc_prefix_attaches_to_following_token(node) {
            output.push(TERMINAL_ATTACH_NEXT_MARKER);
        }
        return;
    }
    // A man font request can remain open across following request lines. The
    // AST therefore nests the later request below the opener (for example a
    // blank `.B` followed by `.I next-line` in a TP Head). Descendant font
    // requests override that inherited device state rather than receiving the
    // outer font a second time.
    let font = match (node.kind(), node.macro_name()) {
        (NodeKind::Element, Some("B")) => TerminalFont::Bold,
        (NodeKind::Element, Some("I")) => TerminalFont::Italic,
        (NodeKind::Element, Some("R")) => TerminalFont::Roman,
        _ => font,
    };
    if let Some(text) = node.text() {
        if !terminal_mdoc_spacing_disabled_before(node)
            && !output.is_empty()
            && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER])
        {
            output.push(' ');
        }
        let rendered = render_terminal_visible_text_with_font(text, format, limits, font);
        output.push_str(&terminal_quoted_trailing_spaces(node, rendered));
        if node.flags().line_continuation && !text.ends_with("\\z\\c") {
            output.push(TERMINAL_ATTACH_NEXT_MARKER);
        }
    }
    for child in node.children() {
        collect_terminal_semantic_text(child, format, limits, font, output);
    }
}

/// Keep blanks that belong to a quoted mdoc macro argument through the filled
/// width pass.  Ordinary whitespace splitting is correct for source layout,
/// but would collapse the public-AST spelling of `.Fl "one " "two "`.
/// A private nonbreaking marker remains one terminal cell and is converted
/// back to a literal blank only after wrapping has completed.
fn terminal_quoted_trailing_spaces(node: NodeRef<'_>, mut rendered: String) -> String {
    if !node.argument_quoted() {
        return rendered;
    }
    let trailing_start = rendered.trim_end_matches(' ').len();
    if trailing_start < rendered.len() {
        let count = rendered[trailing_start..].chars().count();
        rendered.replace_range(
            trailing_start..,
            &TERMINAL_NONBREAKING_SPACE_MARKER.to_string().repeat(count),
        );
    }
    rendered
}

/// Retain an authored interior run of spaces without turning the complete
/// terminal line into a no-wrap line.  The width pass treats the private
/// marker as one visible cell and restores it to a blank after choosing its
/// normal line breaks.
fn terminal_internal_spaces_to_nonbreaking(rendered: &str) -> String {
    let mut output = String::with_capacity(rendered.len());
    let mut previous_was_space = false;
    for character in rendered.chars() {
        if character == ' ' && previous_was_space {
            output.push(TERMINAL_NONBREAKING_SPACE_MARKER);
        } else {
            output.push(character);
        }
        previous_was_space = character == ' ';
    }
    output
}

/// Encode the stable terminal-device bold convention. Both upstream ASCII and
/// UTF-8 terminal outputs use overstriking (`X\\bX`), while HTML follows its
/// independent DOM path. It needs no terminal-capability probing and remains
/// deterministic in a library call.
fn render_terminal_bold(value: &str, _format: RenderFormat) -> String {
    render_terminal_font(value, TerminalFont::Bold)
}

fn render_terminal_font(value: &str, font: TerminalFont) -> String {
    if matches!(font, TerminalFont::Roman) {
        return value.replace(TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER, "\u{8}");
    }
    let mut output = String::with_capacity(value.len().saturating_mul(3));
    for character in value.chars() {
        if character.is_whitespace()
            || character == '\u{8}'
            || character == TERMINAL_NONBREAKING_SPACE_MARKER
            || character == TERMINAL_PENDING_LINE_BREAK_MARKER
        {
            output.push(character);
        } else if character == TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER {
            output.push('\u{8}');
        } else {
            match font {
                TerminalFont::Roman => output.push(character),
                TerminalFont::Bold => {
                    output.push(character);
                    output.push('\u{8}');
                    output.push(character);
                }
                TerminalFont::Italic => {
                    output.push('_');
                    output.push('\u{8}');
                    output.push(character);
                }
                TerminalFont::BoldItalic => {
                    output.push('_');
                    output.push('\u{8}');
                    output.push(character);
                    output.push('\u{8}');
                    output.push(character);
                }
            }
        }
    }
    output
}

fn append_blank_line(output: &mut String, maximum: usize) -> Result<(), RenderError> {
    if output.ends_with(TERMINAL_SENTENCE_PENDING_MARKER) {
        let _ = output.pop();
    }
    // `term_vspace()` consumes a deferred negative `.sp` request before it
    // emits anything.  In particular, `.sp -1v` followed by `.PP` leaves one
    // ordinary line break rather than a blank paragraph gap.
    if take_terminal_vertical_skip(output) {
        return Ok(());
    }
    if take_terminal_table_vertical_skip(output) {
        return Ok(());
    }
    if output.is_empty() || output.ends_with("\n\n") {
        return Ok(());
    }
    if output.ends_with('\n') {
        append(output, "\n", maximum)
    } else {
        append(output, "\n\n", maximum)
    }
}

/// Decode a private `.ti` marker at the start of one pending rendered line.
/// An incomplete marker is deliberately consumed as no temporary indentation:
/// it can only arise from a bounded-output truncation and must never leak to
/// caller-visible terminal text.
fn terminal_temporary_indent(line: &str) -> (Option<usize>, &str) {
    let Some(encoded) = line.strip_prefix(TERMINAL_TEMPORARY_INDENT_MARKER) else {
        return (None, line);
    };
    let Some(end) = encoded.find(TERMINAL_TEMPORARY_INDENT_MARKER) else {
        return (None, "");
    };
    let value = encoded[..end].parse().ok();
    let remainder = &encoded[end + TERMINAL_TEMPORARY_INDENT_MARKER.len_utf8()..];
    (value, remainder)
}

/// Decode a private man `.HP` continuation marker at the start of a pending
/// rendered line. It shares the paired marker encoding used by `.ti`, but
/// affects wrapped lines rather than the first line.
fn terminal_hanging_indent(line: &str) -> (Option<usize>, &str) {
    let Some(encoded) = line.strip_prefix(TERMINAL_HANGING_INDENT_MARKER) else {
        return (None, line);
    };
    let Some(end) = encoded.find(TERMINAL_HANGING_INDENT_MARKER) else {
        return (None, "");
    };
    let value = encoded[..end].parse().ok();
    let remainder = &encoded[end + TERMINAL_HANGING_INDENT_MARKER.len_utf8()..];
    (value, remainder)
}

/// Decode one pending roff `.ll` field width. Invalid private encodings use
/// the caller-configured width, keeping output bounded and never exposing a
/// layout marker to public terminal text.
fn terminal_line_length(line: &str, default: usize) -> (usize, &str) {
    let Some(encoded) = line.strip_prefix(TERMINAL_LINE_LENGTH_MARKER) else {
        return (default, line);
    };
    let Some(end) = encoded.find(TERMINAL_LINE_LENGTH_MARKER) else {
        return (default, "");
    };
    let value = &encoded[..end];
    let state = if value == "D" {
        TerminalLineLength::Default
    } else if let Some(value) = value
        .strip_prefix('A')
        .and_then(|value| value.parse::<usize>().ok())
    {
        TerminalLineLength::Absolute(value)
    } else if let Some(value) = value
        .strip_prefix('R')
        .and_then(|value| value.parse::<isize>().ok())
    {
        TerminalLineLength::Relative(value)
    } else {
        TerminalLineLength::Default
    };
    let remainder = &encoded[end + TERMINAL_LINE_LENGTH_MARKER.len_utf8()..];
    (terminal_line_length_value(state, default), remainder)
}

/// Resolve a reconstructed `.ll` register for one terminal device field.
/// Keeping this separate from marker decoding lets layout primitives (notably
/// tbl's eager `x` width calculation) use the same state before a raw line
/// exists to carry a private marker.
fn terminal_line_length_value(state: TerminalLineLength, default: usize) -> usize {
    match state {
        TerminalLineLength::Default => default,
        TerminalLineLength::Absolute(value) => value,
        TerminalLineLength::Relative(delta) => default.saturating_add_signed(delta),
    }
    .max(1)
}

/// Wrap filled terminal prose with Unicode display-width accounting.
///
/// Explicit line breaks and table tabs remain structural boundaries. The
/// parser already records literal/no-fill layout separately; this conservative
/// first terminal pass therefore wraps only ordinary whitespace-separated
/// prose and never truncates a single long token.
#[allow(clippy::too_many_lines)] // Terminal wrapping keeps all width and marker state in one ordered pass.
fn wrap_terminal_output(
    input: &str,
    width: usize,
    maximum: usize,
    protected_header_lines: usize,
    protected_footer_lines: usize,
) -> Result<String, RenderError> {
    let input = input.replace(
        [
            TERMINAL_ATTACH_NEXT_MARKER,
            TERMINAL_LITERAL_PUNCTUATION_MARKER,
            TERMINAL_FORCE_SEPARATOR_MARKER,
            TERMINAL_CONTINUE_SOURCE_LINE_MARKER,
            TERMINAL_VERTICAL_SKIP_MARKER,
            TERMINAL_TABLE_VERTICAL_SKIP_MARKER,
            TERMINAL_NO_SPACE_MARKER,
        ],
        "",
    );
    // `.ta` state commands occupy private source-order lines.  Consume them
    // before counting device lines so they neither create blank output nor
    // perturb the header/footer protection indexes.
    let mut tab_stops = TerminalTabStops {
        periodic: vec![5],
        ..TerminalTabStops::default()
    };
    let mut lines = Vec::new();
    for line in input.split('\n') {
        if let Some(request) = terminal_tab_stop_request(line) {
            terminal_apply_tab_stop_request(&mut tab_stops, request);
        } else {
            lines.push((line, tab_stops.clone()));
        }
    }
    let mut output = String::new();
    let line_count = lines.len();
    for (line_index, (raw_line, tab_stops)) in lines.into_iter().enumerate() {
        if line_index > 0 {
            append(&mut output, "\n", maximum)?;
        }
        let output_start = output.len();
        let (centered, raw_line) = raw_line
            .strip_prefix(TERMINAL_CENTER_MARKER)
            .map_or((false, raw_line), |line| (true, line));
        let (right_adjusted, raw_line) = raw_line
            .strip_prefix(TERMINAL_RIGHT_MARKER)
            .map_or((false, raw_line), |line| (true, line));
        let (no_wrap, line) = raw_line
            .strip_prefix(TERMINAL_NO_WRAP_MARKER)
            .map_or((false, raw_line), |line| (true, line));
        let (literal_tabs, line) = line
            .strip_prefix(TERMINAL_LITERAL_TAB_MARKER)
            .map_or((false, line), |line| (true, line));
        let (keep_spacing, line) = line
            .strip_prefix(TERMINAL_KEEP_SPACING_MARKER)
            .map_or((false, line), |line| (true, line));
        let (line_width, line) = terminal_line_length(line, width);
        let (temporary_indent, line) = terminal_temporary_indent(line);
        let (hanging_indent, line) = terminal_hanging_indent(line);
        // The default `T`/`.5i` tab policy starts at the fifth column of the
        // text field, then advances in five-column fields. The distinct
        // `Bd -literal` device state uses eight-column stops unless an
        // authored `.ta` request has supplied an explicit configuration.
        let expanded = line.contains('\t').then(|| {
            if tab_stops.configured {
                expand_terminal_tabs(line, &tab_stops)
            } else if literal_tabs {
                expand_literal_terminal_tabs(line)
            } else {
                expand_filled_terminal_tabs(line)
            }
        });
        let line = expanded.as_deref().unwrap_or(line);
        if line_index < protected_header_lines
            || line_index >= line_count.saturating_sub(protected_footer_lines)
            || no_wrap
            || keep_spacing
            || line.is_empty()
            || line.contains('\t')
        {
            let temporary_line = temporary_indent.map(|target| {
                let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
                format!("{}{}", " ".repeat(target), &line[indentation..])
            });
            let line = temporary_line.as_deref().unwrap_or(line);
            let line = line
                .replace(
                    [
                        TERMINAL_SENTENCE_SPACE_MARKER,
                        TERMINAL_OPTIONAL_BREAK_MARKER,
                        TERMINAL_NO_HYPHEN_BREAK_MARKER,
                        TERMINAL_SENTENCE_PENDING_MARKER,
                    ],
                    "",
                )
                .replace(TERMINAL_NONBREAKING_SPACE_MARKER, " ");
            append(&mut output, &line, maximum)?;
            if centered {
                center_terminal_output_segment(&mut output, output_start, line_width, maximum)?;
            } else if right_adjusted {
                right_adjust_terminal_output_segment(
                    &mut output,
                    output_start,
                    line_width,
                    maximum,
                )?;
            }
            continue;
        }
        let indent_width = line.bytes().take_while(|byte| *byte == b' ').count();
        let (indent, content) = line.split_at(indent_width);
        let initial_indent_width = temporary_indent.unwrap_or(indent_width);
        let initial_indent =
            temporary_indent.map_or_else(|| indent.to_owned(), |target| " ".repeat(target));
        let continuation_indent_width = hanging_indent.unwrap_or(indent_width);
        let continuation_indent =
            hanging_indent.map_or_else(|| indent.to_owned(), |target| " ".repeat(target));
        let mut current_width = 0_usize;
        let mut first_word = true;
        let mut initial_line = true;
        let mut sentence_spacing = false;
        for raw_word in content.split_whitespace() {
            if raw_word == "\u{1b}" {
                sentence_spacing = true;
                continue;
            }
            let no_hyphen_break = raw_word.contains(TERMINAL_NO_HYPHEN_BREAK_MARKER);
            let word = raw_word.replace(
                [
                    TERMINAL_OPTIONAL_BREAK_MARKER,
                    TERMINAL_NO_HYPHEN_BREAK_MARKER,
                    TERMINAL_SENTENCE_PENDING_MARKER,
                ],
                "",
            );
            let word_width = display_width(&word);
            let separator = if first_word {
                0
            } else if sentence_spacing {
                2
            } else {
                1
            };
            if first_word
                && raw_word.contains(TERMINAL_OPTIONAL_BREAK_MARKER)
                && initial_indent_width.saturating_add(word_width) > line_width
                && let Some((prefix, suffix)) = terminal_optional_break(
                    raw_word,
                    line_width.saturating_sub(initial_indent_width),
                )
            {
                let prefix = prefix.replace(TERMINAL_OPTIONAL_BREAK_MARKER, "");
                let suffix = suffix.replace(TERMINAL_OPTIONAL_BREAK_MARKER, "");
                append(&mut output, &initial_indent, maximum)?;
                append(&mut output, &prefix, maximum)?;
                append(&mut output, "\n", maximum)?;
                append(&mut output, &continuation_indent, maximum)?;
                append(&mut output, &suffix, maximum)?;
                current_width = continuation_indent_width.saturating_add(display_width(&suffix));
                first_word = false;
                initial_line = false;
                sentence_spacing = false;
                continue;
            }
            if !first_word
                && current_width > 0
                && current_width
                    .saturating_add(separator)
                    .saturating_add(word_width)
                    > line_width
            {
                let available = line_width.saturating_sub(current_width.saturating_add(separator));
                if let Some((prefix, suffix)) = terminal_optional_break(raw_word, available) {
                    let prefix = prefix.replace(TERMINAL_OPTIONAL_BREAK_MARKER, "");
                    let suffix = suffix.replace(TERMINAL_OPTIONAL_BREAK_MARKER, "");
                    append(&mut output, &" ".repeat(separator), maximum)?;
                    append(&mut output, &prefix, maximum)?;
                    append(&mut output, "\n", maximum)?;
                    append(&mut output, &continuation_indent, maximum)?;
                    append(&mut output, &suffix, maximum)?;
                    current_width =
                        continuation_indent_width.saturating_add(display_width(&suffix));
                    first_word = false;
                    initial_line = false;
                    sentence_spacing = false;
                    continue;
                }
                if !no_hyphen_break
                    && let Some((prefix, suffix)) = terminal_hyphen_break(&word, available)
                {
                    append(&mut output, &" ".repeat(separator), maximum)?;
                    append(&mut output, prefix, maximum)?;
                    append(&mut output, "\n", maximum)?;
                    append(&mut output, &continuation_indent, maximum)?;
                    append(&mut output, suffix, maximum)?;
                    current_width = continuation_indent_width.saturating_add(display_width(suffix));
                    first_word = false;
                    initial_line = false;
                    sentence_spacing = false;
                    continue;
                }
                append(&mut output, "\n", maximum)?;
                append(&mut output, &continuation_indent, maximum)?;
                current_width = continuation_indent_width;
                first_word = true;
                initial_line = false;
            }
            if first_word {
                if initial_line && current_width == 0 {
                    append(&mut output, &initial_indent, maximum)?;
                    current_width = initial_indent_width;
                }
            } else {
                append(&mut output, &" ".repeat(separator), maximum)?;
                current_width = current_width.saturating_add(separator);
            }
            append(&mut output, &word, maximum)?;
            current_width = current_width.saturating_add(word_width);
            first_word = false;
            sentence_spacing = false;
        }
        if centered {
            center_terminal_output_segment(&mut output, output_start, line_width, maximum)?;
        } else if right_adjusted {
            right_adjust_terminal_output_segment(&mut output, output_start, line_width, maximum)?;
        }
    }
    Ok(output
        .replace(TERMINAL_NONBREAKING_SPACE_MARKER, " ")
        .replace(TERMINAL_SENTENCE_PENDING_MARKER, ""))
}

/// Center a just-emitted display fragment inside the visible field already
/// represented by its leading indentation.  The fragment is limited to one
/// source display line, but normal wrapping may have introduced additional
/// physical lines; each receives its own centering calculation.
fn center_terminal_output_segment(
    output: &mut String,
    start: usize,
    width: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    let fragment = output.split_off(start);
    for (index, line) in fragment.split('\n').enumerate() {
        if index > 0 {
            append(output, "\n", maximum)?;
        }
        if line.is_empty() {
            continue;
        }
        let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
        let content_width = display_width(&line[indentation..]);
        let leading = width
            .saturating_sub(indentation)
            .saturating_sub(content_width)
            / 2;
        append(output, &" ".repeat(leading), maximum)?;
        append(output, line, maximum)?;
    }
    Ok(())
}

/// Right-align a completed no-fill roff request at the device margin.
/// `.rj` is distinct from a section or display field: it uses the page's
/// current right column, so the marker's payload begins with no field prefix.
fn right_adjust_terminal_output_segment(
    output: &mut String,
    start: usize,
    width: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    let fragment = output.split_off(start);
    for (index, line) in fragment.split('\n').enumerate() {
        if index > 0 {
            append(output, "\n", maximum)?;
        }
        if line.is_empty() {
            continue;
        }
        append(
            output,
            &" ".repeat(width.saturating_sub(display_width(line))),
            maximum,
        )?;
        append(output, line, maximum)?;
    }
    Ok(())
}

fn terminal_hyphen_break(word: &str, available: usize) -> Option<(&str, &str)> {
    let hyphen = word.rfind('-')?;
    let (prefix, suffix) = word.split_at(hyphen + 1);
    (!suffix.is_empty() && display_width(prefix) <= available).then_some((prefix, suffix))
}

fn terminal_optional_break(word: &str, available: usize) -> Option<(&str, &str)> {
    word.match_indices(TERMINAL_OPTIONAL_BREAK_MARKER)
        .filter_map(|(offset, _)| {
            let prefix = &word[..offset];
            let suffix = &word[offset + TERMINAL_OPTIONAL_BREAK_MARKER.len_utf8()..];
            (!suffix.is_empty()
                && display_width(&prefix.replace(TERMINAL_OPTIONAL_BREAK_MARKER, "")) <= available)
                .then_some((prefix, suffix))
        })
        .next_back()
}

fn expand_filled_terminal_tabs(line: &str) -> String {
    expand_terminal_tabs(
        line,
        &TerminalTabStops {
            periodic: vec![5],
            ..TerminalTabStops::default()
        },
    )
}

fn expand_terminal_tabs(line: &str, tab_stops: &TerminalTabStops) -> String {
    let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
    let (prefix, content) = line.split_at(indentation);
    let mut output = String::with_capacity(line.len().saturating_add(8));
    output.push_str(prefix);
    let mut column = 0_usize;
    let mut characters = content.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\t' {
            let next = terminal_tab_next(tab_stops, column);
            let spaces = next.saturating_sub(column);
            output.push_str(&" ".repeat(spaces));
            column = next;
            continue;
        }
        output.push(character);
        column = column.saturating_add(terminal_character_width(character));
        if characters.peek() == Some(&'\u{8}') {
            output.push(characters.next().expect("peeked overstrike is present"));
            if let Some(overstrike) = characters.next() {
                output.push(overstrike);
            }
        }
    }
    output
}

fn expand_literal_terminal_tabs(line: &str) -> String {
    expand_terminal_tabs(
        line,
        &TerminalTabStops {
            periodic: vec![8],
            ..TerminalTabStops::default()
        },
    )
}

fn display_width(value: &str) -> usize {
    let mut column = 0_usize;
    let mut maximum = 0_usize;
    for character in value.chars() {
        match character {
            '\n' => column = 0,
            // The terminal's historical emphasis streams can contain more
            // than one consecutive overstrike (the bullet is
            // `+\b+\bo\bo`).  Track the actual cursor rather than assuming
            // each backspace occurs only in a two-glyph pair.
            '\u{8}' => column = column.saturating_sub(1),
            character => {
                let width = terminal_character_width(character);
                column = column.saturating_add(width);
                maximum = maximum.max(column);
            }
        }
    }
    maximum
}

/// Width of one terminal device character.
///
/// mandoc's pinned UTF-8 regressions use the platform `wcwidth()` contract.
/// Hangul Jamo Extended-B is double-width there, while `unicode-width` treats
/// it as a zero-width combining range.  Keep that device distinction local to
/// terminal geometry so source text and the public AST retain their Unicode
/// spelling unchanged.
fn terminal_character_width(character: char) -> usize {
    if character == TERMINAL_NONBREAKING_SPACE_MARKER {
        return 1;
    }
    let scalar = u32::from(character);
    // The reference terminal coerces negative `wcwidth()` results to zero.
    // These stable-regression scalars are unassigned or noncharacters in the
    // pinned device table, while `unicode-width` reports one cell for them.
    // Keep that distinction local to terminal geometry: source text and the
    // public AST retain their authored spelling unchanged.
    if matches!(
        scalar,
        0x0fff | 0xd7ff | 0x3ffff | 0x40000 | 0xc0000 | 0xeffff | 0xfffff
    ) || matches!(scalar & 0xffff, 0xfffe | 0xffff)
    {
        return 0;
    }
    if matches!(character, '\u{d7b0}'..='\u{d7fb}') {
        return 2;
    }
    UnicodeWidthChar::width(character).unwrap_or(0)
}

/// Interpret presentation-only roff escapes after parsing has preserved their
/// authored spelling in the public AST.
///
/// Parsing deliberately retains several formatter controls because source
/// fidelity, diagnostics, and downstream lowering need that spelling. A
/// reference renderer instead consumes the zero-width controls and resolves
/// named characters. Numeric `\\N'…'` escapes are a renderer concern too: the
/// stable mandoc ASCII device accepts only its one-byte character domain.
fn render_visible_text(text: &str, format: RenderFormat, limits: &Limits) -> String {
    let device_strings = render_default_device_string_escapes(text, format);
    let whitespace = render_terminal_whitespace_escapes(&device_strings);
    // Mandoc's two-character `~=` and `~~` names share U+2248 in the
    // character catalogue but use distinct ASCII-device spellings. Preserve
    // the only ambiguous source form before scalar normalization erases that
    // distinction; all other formats intentionally use the common scalar.
    let whitespace = if format == RenderFormat::Ascii {
        whitespace.replace(r"\(~=", "~=")
    } else {
        whitespace
    };
    let unicode = render_unicode_character_escapes(&whitespace, format);
    let numeric = render_numeric_character_escapes(&unicode, format);
    let normalized = crate::escape::normalize_escapes(numeric.as_bytes(), b'\\', limits)
        .text
        .replace(RENDER_LITERAL_BACKSLASH_MARKER, "\\");
    if format == RenderFormat::Ascii {
        ascii_terminal_text(&normalized)
    } else {
        normalized
    }
}

/// Resolve the formatter's default `.T` string only in presentation.
///
/// The parser intentionally retains `\*(.T` and `\*[.T]` in the compatible
/// public AST until a user `.ds .T` override exists.  The terminal and HTML
/// formatters, however, expose their own device name at render time.  Treat a
/// doubled escape as literal input so this renderer-only substitution cannot
/// reinterpret an explicitly escaped spelling.
fn render_default_device_string_escapes(text: &str, format: RenderFormat) -> String {
    let device = match format {
        RenderFormat::Ascii => "ascii",
        RenderFormat::Utf8 => "utf8",
        RenderFormat::Html => "html",
    };
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\\\") {
            output.push_str("\\\\");
            cursor = cursor.saturating_add(2);
            continue;
        }
        if bytes.get(cursor..cursor.saturating_add(5)) == Some(b"\\*(.T") {
            output.push_str(device);
            cursor = cursor.saturating_add(5);
            continue;
        }
        if bytes.get(cursor..cursor.saturating_add(6)) == Some(b"\\*[.T]") {
            output.push_str(device);
            cursor = cursor.saturating_add(6);
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        output.push(character);
        cursor = cursor.saturating_add(character.len_utf8());
    }
    output
}

fn ascii_terminal_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(
            character,
            TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER | TERMINAL_PENDING_LINE_BREAK_MARKER
        ) {
            output.push(character);
            continue;
        }
        match character {
            // The ASCII device encodes these arrows as overstruck glyphs.
            '\u{2191}' => output.push_str("|\u{8}^"),
            '\u{21d1}' => output.push_str("=\u{8}^"),
            // The named combining-accent fallbacks occupy one terminal
            // column in mandoc's ASCII device rather than becoming `?`.
            '\u{00b4}' => output.push('\''),
            '\u{02dd}' | '\u{00a8}' => output.push('"'),
            '\u{00b8}' | '\u{02db}' => output.push(','),
            '\u{02c7}' => output.push('v'),
            '\u{02da}' => output.push('o'),
            // Punctuation names use printable device fallbacks in ASCII.
            '\u{2010}' | '\u{2013}' => output.push('-'),
            '\u{2014}' => output.push_str("--"),
            '\u{2018}' => output.push('`'),
            '\u{2019}' => output.push('\''),
            '\u{201a}' => output.push(','),
            '\u{201c}' | '\u{201d}' => output.push('"'),
            '\u{201e}' => output.push_str(",,"),
            '\u{226a}' => output.push_str("<<"),
            '\u{226b}' => output.push_str(">>"),
            // The terminal table draws extensible delimiters with their
            // portable ASCII pieces rather than treating each Unicode scalar
            // as an unsupported glyph.
            '\u{203e}' => output.push('-'),
            '\u{210f}' => output.push_str("/h"),
            '\u{2195}' => output.push_str("^v"),
            '\u{21d5}' => output.push_str("^=v"),
            '\u{239b}' | '\u{23a0}' => output.push('/'),
            '\u{239c}' | '\u{239f}' | '\u{23a1}' | '\u{23a2}' | '\u{23a3}' | '\u{23a4}'
            | '\u{23a5}' | '\u{23a6}' | '\u{23aa}' => output.push('|'),
            '\u{239d}' | '\u{239e}' => output.push('\\'),
            '\u{23a7}' => output.push_str(",-"),
            '\u{23a8}' => output.push('{'),
            '\u{23a9}' => output.push_str("`-"),
            '\u{23ab}' => output.push_str("-."),
            '\u{23ac}' => output.push('}'),
            '\u{23ad}' => output.push_str("-'"),
            _ => {
                if let Some(fallback) = ascii_terminal_catalog_fallback(character) {
                    output.push_str(fallback);
                } else {
                    output.push(ascii_terminal_character(character));
                }
            }
        }
    }
    output
}

fn ascii_terminal_named_scalar_is_known(character: char) -> bool {
    ascii_terminal_catalog_fallback(character).is_some()
        || matches!(
            character,
            '\u{2191}'
                | '\u{21d1}'
                | '\u{00b4}'
                | '\u{02dd}'
                | '\u{00a8}'
                | '\u{00b8}'
                | '\u{02db}'
                | '\u{02c7}'
                | '\u{02da}'
                | '\u{2010}'
                | '\u{2013}'
                | '\u{2014}'
                | '\u{2212}'
                | '\u{2018}'
                | '\u{2019}'
                | '\u{201a}'
                | '\u{201c}'
                | '\u{201d}'
                | '\u{201e}'
                | '\u{226a}'
                | '\u{226b}'
                | '\u{203e}'
                | '\u{210f}'
                | '\u{2195}'
                | '\u{21d5}'
                | '\u{239b}'
                | '\u{23a0}'
                | '\u{239c}'
                | '\u{239f}'
                | '\u{23a1}'
                | '\u{23a2}'
                | '\u{23a3}'
                | '\u{23a4}'
                | '\u{23a5}'
                | '\u{23a6}'
                | '\u{23aa}'
                | '\u{239d}'
                | '\u{239e}'
                | '\u{23a7}'
                | '\u{23a8}'
                | '\u{23a9}'
                | '\u{23ab}'
                | '\u{23ac}'
                | '\u{23ad}'
        )
}

/// ASCII device spellings for catalog scalars that cannot occupy one
/// printable ASCII cell. The table is pinned to mandoc 1.14.6.
fn ascii_terminal_catalog_fallback(character: char) -> Option<&'static str> {
    let fallback = match character {
        // Latin-1 symbols and letters are emitted through the same catalog
        // as `\\[u00xx]` escapes.  Preserve mandoc's terminal-device
        // spellings, including its backspace overstrikes for diacritics.
        '\u{00a1}' => "!",
        '\u{00a2}' => "/\x08c",
        '\u{00a3}' => "-\x08L",
        '\u{00a4}' => "o\x08x",
        '\u{00a5}' => "=\x08Y",
        '\u{00a6}' => "|",
        '\u{00a7}' => "<section>",
        '\u{00a9}' => "(C)",
        '\u{00aa}' => "_\x08a",
        '\u{00ab}' => "<<",
        '\u{00ac}' => "~",
        '\u{00ad}' => "",
        '\u{00ae}' => "(R)",
        '\u{00af}' => "-",
        '\u{00b0}' => "<degree>",
        '\u{00b1}' => "+-",
        '\u{00b2}' => "^2",
        '\u{00b3}' => "^3",
        '\u{00b5}' => "<micro>",
        '\u{00b6}' => "<paragraph>",
        '\u{00b7}' => ".",
        '\u{00b9}' => "^1",
        '\u{00ba}' => "_\x08o",
        '\u{00bb}' => ">>",
        '\u{00bc}' => "1/4",
        '\u{00bd}' => "1/2",
        '\u{00be}' => "3/4",
        '\u{00bf}' => "?",
        '\u{00c0}' => "\x60\x08A",
        '\u{00c1}' => "\x27\x08A",
        '\u{00c2}' => "^\x08A",
        '\u{00c3}' => "~\x08A",
        '\u{00c4}' => "\"\x08A",
        '\u{00c5}' => "o\x08A",
        '\u{00c6}' => "AE",
        '\u{00c7}' => ",\x08C",
        '\u{00c8}' => "\x60\x08E",
        '\u{00c9}' => "\x27\x08E",
        '\u{00ca}' => "^\x08E",
        '\u{00cb}' => "\"\x08E",
        '\u{00cc}' => "\x60\x08I",
        '\u{00cd}' => "\x27\x08I",
        '\u{00ce}' => "^\x08I",
        '\u{00cf}' => "\"\x08I",
        '\u{00d0}' => "Dh",
        '\u{00d1}' => "~\x08N",
        '\u{00d2}' => "\x60\x08O",
        '\u{00d3}' => "\x27\x08O",
        '\u{00d4}' => "^\x08O",
        '\u{00d5}' => "~\x08O",
        '\u{00d6}' => "\"\x08O",
        '\u{00d7}' => "x",
        '\u{00d8}' => "/\x08O",
        '\u{00d9}' => "\x60\x08U",
        '\u{00da}' => "\x27\x08U",
        '\u{00db}' => "^\x08U",
        '\u{00dc}' => "\"\x08U",
        '\u{00dd}' => "\x27\x08Y",
        '\u{00de}' => "Th",
        '\u{00df}' => "ss",
        '\u{00e0}' => "\x60\x08a",
        '\u{00e1}' => "\x27\x08a",
        '\u{00e2}' => "^\x08a",
        '\u{00e3}' => "~\x08a",
        '\u{00e4}' => "\"\x08a",
        '\u{00e5}' => "o\x08a",
        '\u{00e6}' => "ae",
        '\u{00e7}' => ",\x08c",
        '\u{00e8}' => "\x60\x08e",
        '\u{00e9}' => "\x27\x08e",
        '\u{00ea}' => "^\x08e",
        '\u{00eb}' => "\"\x08e",
        '\u{00ec}' => "\x60\x08i",
        '\u{00ed}' => "\x27\x08i",
        '\u{00ee}' => "^\x08i",
        '\u{00ef}' => "\"\x08i",
        '\u{00f0}' => "dh",
        '\u{00f1}' => "~\x08n",
        '\u{00f2}' => "\x60\x08o",
        '\u{00f3}' => "\x27\x08o",
        '\u{00f4}' => "^\x08o",
        '\u{00f5}' => "~\x08o",
        '\u{00f6}' => "\"\x08o",
        '\u{00f7}' => "/",
        '\u{00f8}' => "/\x08o",
        '\u{00f9}' => "\x60\x08u",
        '\u{00fa}' => "\x27\x08u",
        '\u{00fb}' => "^\x08u",
        '\u{00fc}' => "\"\x08u",
        '\u{00fd}' => "\x27\x08y",
        '\u{00fe}' => "th",
        '\u{00ff}' => "\"\x08y",
        '\u{02d8}' => "\x27\x08\x60",
        '\u{02d9}' => ".",
        // The stable Unicode-name regression also exercises the portable
        // punctuation, mathematical, and symbol catalogue.  These strings
        // are the ASCII column of mandoc 1.14.6's `chars.c` table.
        '\u{2020}' => "<*>",
        '\u{2021}' => "<**>",
        '\u{2022}' => "+\x08o",
        '\u{2030}' => "<permille>",
        '\u{2032}' => "'",
        '\u{2033}' => "''",
        '\u{2039}' => "<",
        '\u{203a}' => ">",
        '\u{2044}' => "/",
        '\u{20ac}' => "EUR",
        '\u{2111}' => "<Im>",
        '\u{2118}' => "p",
        '\u{211c}' => "<Re>",
        '\u{2122}' => "tm",
        '\u{2135}' => "<Aleph>",
        '\u{215b}' => "1/8",
        '\u{215c}' => "3/8",
        '\u{215d}' => "5/8",
        '\u{215e}' => "7/8",
        '\u{2190}' => "<-",
        '\u{2192}' => "->",
        '\u{2193}' => "|\x08v",
        '\u{2194}' => "<->",
        '\u{21b5}' => "<cr>",
        '\u{21d0}' => "<=",
        '\u{21d2}' => "=>",
        '\u{21d3}' => "=\x08v",
        '\u{21d4}' => "<=>",
        '\u{2200}' => "<for all>",
        '\u{2202}' => "<del>",
        '\u{2203}' => "<there exists>",
        '\u{2205}' => "{}",
        '\u{2207}' => "<nabla>",
        '\u{2208}' => "<element of>",
        '\u{2209}' => "<not element of>",
        '\u{220b}' => "<such that>",
        '\u{220f}' => "<product>",
        '\u{2210}' => "<coproduct>",
        '\u{2211}' => "<sum>",
        '\u{2213}' => "-+",
        '\u{2217}' => "*",
        '\u{221a}' => "<sqrt>",
        '\u{221d}' => "<proportional to>",
        '\u{221e}' => "<infinity>",
        '\u{2220}' => "<angle>",
        '\u{2227}' => "^",
        '\u{2228}' => "v",
        '\u{2229}' => "<intersection>",
        '\u{222a}' => "<union>",
        '\u{222b}' => "<integral>",
        '\u{2234}' => "<therefore>",
        '\u{223c}' => "~",
        '\u{2243}' => "-~",
        '\u{2245}' => "=~",
        '\u{2248}' => "~~",
        '\u{2260}' => "!=",
        '\u{2261}' => "==",
        '\u{2262}' => "!==",
        '\u{2264}' => "<=",
        '\u{2265}' => ">=",
        '\u{2282}' => "<proper subset>",
        '\u{2283}' => "<proper superset>",
        '\u{2284}' => "<not subset>",
        '\u{2285}' => "<not superset>",
        '\u{2286}' => "<subset or equal>",
        '\u{2287}' => "<superset or equal>",
        '\u{2295}' => "O\x08+",
        '\u{2297}' => "O\x08x",
        '\u{22a5}' => "<perpendicular>",
        '\u{22c5}' => ".",
        '\u{2308}' => "|~",
        '\u{2309}' => "~|",
        '\u{230a}' => "|_",
        '\u{230b}' => "_|",
        '\u{23af}' => "-",
        '\u{2502}' => "|",
        '\u{25a1}' => "[]",
        '\u{25ca}' => "<>",
        '\u{25cb}' => "O",
        '\u{261c}' => "<=",
        '\u{261e}' => "=>",
        '\u{2660}' => "S",
        '\u{2663}' => "C",
        '\u{2665}' => "H",
        '\u{2666}' => "D",
        '\u{27e8}' => "<",
        '\u{27e9}' => ">",
        '\u{0131}' => "i",
        '\u{0132}' => "IJ",
        '\u{0133}' => "ij",
        '\u{0141}' => "/\x08L",
        '\u{0142}' => "/\x08l",
        '\u{0152}' => "OE",
        '\u{0153}' => "oe",
        '\u{0192}' => ",\x08f",
        '\u{0237}' => "j",
        '\u{0391}' => "A",
        '\u{0392}' => "B",
        '\u{0393}' => "<Gamma>",
        '\u{0394}' => "<Delta>",
        '\u{0395}' => "E",
        '\u{0396}' => "Z",
        '\u{0397}' => "H",
        '\u{0398}' => "<Theta>",
        '\u{0399}' => "I",
        '\u{039a}' => "K",
        '\u{039b}' => "<Lambda>",
        '\u{039c}' => "M",
        '\u{039d}' => "N",
        '\u{039e}' => "<Xi>",
        '\u{039f}' => "O",
        '\u{03a0}' => "<Pi>",
        '\u{03a1}' => "P",
        '\u{03a3}' => "<Sigma>",
        '\u{03a4}' => "T",
        '\u{03a5}' => "Y",
        '\u{03a6}' => "<Phi>",
        '\u{03a7}' => "X",
        '\u{03a8}' => "<Psi>",
        '\u{03a9}' => "<Omega>",
        '\u{03b1}' => "<alpha>",
        '\u{03b2}' => "<beta>",
        '\u{03b3}' => "<gamma>",
        '\u{03b4}' => "<delta>",
        '\u{03b5}' => "<epsilon>",
        '\u{03b6}' => "<zeta>",
        '\u{03b7}' => "<eta>",
        '\u{03b8}' => "<theta>",
        '\u{03b9}' => "<iota>",
        '\u{03ba}' => "<kappa>",
        '\u{03bb}' => "<lambda>",
        '\u{03bc}' => "<mu>",
        '\u{03bd}' => "<nu>",
        '\u{03be}' => "<xi>",
        '\u{03bf}' => "o",
        '\u{03c0}' => "<pi>",
        '\u{03c1}' => "<rho>",
        '\u{03c2}' | '\u{03c3}' => "<sigma>",
        '\u{03c4}' => "<tau>",
        '\u{03c5}' => "<upsilon>",
        '\u{03c6}' => "<phi>",
        '\u{03c7}' => "<chi>",
        '\u{03c8}' => "<psi>",
        '\u{03c9}' => "<omega>",
        '\u{03d1}' => "<theta>",
        '\u{03d5}' => "<phi>",
        '\u{03d6}' => "<pi>",
        '\u{03f5}' => "<epsilon>",
        '\u{fb00}' => "ff",
        '\u{fb01}' => "fi",
        '\u{fb02}' => "fl",
        '\u{fb03}' => "ffi",
        '\u{fb04}' => "ffl",
        _ => return None,
    };
    Some(fallback)
}

/// One private node in the renderer's retained eqn box tree.
///
/// This deliberately parallels mandoc's `eqn_box`: public `Node::equation`
/// is a compatibility projection and cannot carry these font, decoration, or
/// grouping edges without changing the owned-AST contract.
#[derive(Clone, Debug)]
struct TerminalEquationBox {
    parent: Option<usize>,
    children: Vec<usize>,
    kind: TerminalEquationKind,
    position: TerminalEquationPosition,
    font: TerminalEquationFont,
    quoted: bool,
    text: Option<Box<str>>,
    left: Option<Box<str>>,
    right: Option<Box<str>>,
    top: Option<Box<str>>,
    bottom: Option<Box<str>>,
    expected_arguments: usize,
}

impl TerminalEquationBox {
    fn root() -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            kind: TerminalEquationKind::List,
            position: TerminalEquationPosition::None,
            font: TerminalEquationFont::None,
            quoted: false,
            text: None,
            left: None,
            right: None,
            top: None,
            bottom: None,
            expected_arguments: usize::MAX,
        }
    }

    fn child(font: TerminalEquationFont, parent: usize) -> Self {
        Self {
            parent: Some(parent),
            children: Vec::new(),
            kind: TerminalEquationKind::Text,
            position: TerminalEquationPosition::None,
            font,
            quoted: false,
            text: None,
            left: None,
            right: None,
            top: None,
            bottom: None,
            expected_arguments: usize::MAX,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalEquationKind {
    Text,
    Subexpression,
    List,
    Pile,
    Matrix,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalEquationPosition {
    None,
    Sup,
    Subsup,
    Sub,
    To,
    From,
    Fromto,
    Over,
    Sqrt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalEquationFont {
    None,
    Roman,
    Bold,
    Fat,
    Italic,
}

impl TerminalEquationFont {
    fn terminal(self) -> TerminalFont {
        match self {
            Self::None | Self::Roman => TerminalFont::Roman,
            Self::Bold | Self::Fat => TerminalFont::Bold,
            Self::Italic => TerminalFont::Italic,
        }
    }
}

/// An allocation arena makes the C parser's re-parenting operation explicit
/// without self-referential Rust pointers.  It is private, bounded by parser
/// token limits, and dropped immediately after one render call.
#[derive(Default)]
struct TerminalEquationTree {
    boxes: Vec<TerminalEquationBox>,
}

impl TerminalEquationTree {
    fn new() -> Self {
        Self {
            boxes: vec![TerminalEquationBox::root()],
        }
    }

    fn allocate(&mut self, parent: usize) -> usize {
        let font = self.boxes[parent].font;
        let index = self.boxes.len();
        self.boxes.push(TerminalEquationBox::child(font, parent));
        self.boxes[parent].children.push(index);
        index
    }

    fn parent(&self, index: usize) -> usize {
        self.boxes[index].parent.unwrap_or(0)
    }

    fn previous(&self, index: usize) -> Option<usize> {
        let parent = self.boxes[index].parent?;
        let siblings = &self.boxes[parent].children;
        let position = siblings.iter().position(|sibling| *sibling == index)?;
        position
            .checked_sub(1)
            .and_then(|position| siblings.get(position).copied())
    }

    fn next(&self, index: usize) -> Option<usize> {
        let parent = self.boxes[index].parent?;
        let siblings = &self.boxes[parent].children;
        let position = siblings.iter().position(|sibling| *sibling == index)?;
        siblings.get(position + 1).copied()
    }

    fn first(&self, index: usize) -> Option<usize> {
        self.boxes[index].children.first().copied()
    }

    fn move_to_available(&self, mut parent: usize) -> usize {
        while parent != 0
            && self.boxes[parent].children.len() >= self.boxes[parent].expected_arguments
        {
            parent = self.parent(parent);
        }
        parent
    }

    fn move_past_singletons(&self, mut parent: usize) -> usize {
        while parent != 0
            && self.boxes[parent].kind == TerminalEquationKind::List
            && self.boxes[parent].expected_arguments == 1
            && self.boxes[parent].children.len() == 1
        {
            parent = self.parent(parent);
        }
        parent
    }

    fn make_binary(&mut self, parent: usize) -> usize {
        let previous = self.boxes[parent]
            .children
            .pop()
            .expect("binary eqn operator has a left box");
        let binary = self.allocate(parent);
        self.boxes[binary].kind = TerminalEquationKind::Subexpression;
        self.boxes[binary].expected_arguments = 2;
        self.boxes[binary].children.push(previous);
        self.boxes[previous].parent = Some(binary);
        binary
    }

    fn add_text(
        &mut self,
        parent: usize,
        text: impl Into<Box<str>>,
        font: Option<TerminalEquationFont>,
    ) -> usize {
        let text = text.into();
        let node = self.allocate(parent);
        self.boxes[node].kind = TerminalEquationKind::Text;
        self.boxes[node].text = Some(text);
        if let Some(font) = font {
            self.boxes[node].font = font;
        }
        node
    }
}

/// Build the private eqn box tree using the same left/right association rules
/// that define mandoc's device behavior.  The parser has already applied the
/// public budgets and definition expansion, so this phase cannot widen its
/// resource envelope or affect AST/diagnostic compatibility.
fn parse_terminal_equation(tokens: &[EquationTerminalToken]) -> TerminalEquationTree {
    let tokens = coalesce_terminal_equation_escapes(tokens);
    let mut tree = TerminalEquationTree::new();
    let mut parent = 0_usize;
    let mut index = 0_usize;
    while let Some(token) = tokens.get(index) {
        index += 1;
        let text = token.text.as_ref();
        let keyword = (!token.quoted).then_some(text);
        match keyword {
            Some("mark" | "lineup") => {}
            Some("gfont" | "gsize" | "fwd" | "back" | "down" | "up") => {
                index = index.saturating_add(usize::from(tokens.get(index).is_some()));
            }
            Some("size") => {
                index = index.saturating_add(usize::from(tokens.get(index).is_some()));
                parent = tree.move_to_available(parent);
                let size = tree.allocate(parent);
                tree.boxes[size].kind = TerminalEquationKind::List;
                tree.boxes[size].expected_arguments = 1;
                parent = size;
            }
            Some("roman" | "bold" | "italic" | "fat") => {
                parent = tree.move_to_available(parent);
                let font = tree.allocate(parent);
                tree.boxes[font].kind = TerminalEquationKind::List;
                tree.boxes[font].expected_arguments = 1;
                tree.boxes[font].font = match text {
                    "roman" => TerminalEquationFont::Roman,
                    "bold" => TerminalEquationFont::Bold,
                    "italic" => TerminalEquationFont::Italic,
                    "fat" => TerminalEquationFont::Fat,
                    _ => unreachable!("matched eqn font keyword"),
                };
                parent = font;
            }
            Some("sqrt") => {
                parent = tree.move_to_available(parent);
                let sqrt = tree.allocate(parent);
                tree.boxes[sqrt].kind = TerminalEquationKind::Subexpression;
                tree.boxes[sqrt].position = TerminalEquationPosition::Sqrt;
                tree.boxes[sqrt].expected_arguments = 1;
                parent = sqrt;
            }
            Some("sub" | "sup" | "from" | "to") => {
                if tree.boxes[parent].children.is_empty() {
                    let _ = tree.add_text(parent, "", Some(TerminalEquationFont::Roman));
                }
                while parent != 0
                    && tree.boxes[parent].expected_arguments == 1
                    && tree.boxes[parent].children.len() == 1
                {
                    parent = tree.parent(parent);
                }
                if matches!(text, "from" | "to") {
                    let mut positioned = Some(parent);
                    while let Some(candidate) = positioned {
                        if matches!(
                            tree.boxes[candidate].position,
                            TerminalEquationPosition::Sub
                                | TerminalEquationPosition::Sup
                                | TerminalEquationPosition::Subsup
                                | TerminalEquationPosition::Sqrt
                                | TerminalEquationPosition::Over
                        ) {
                            parent = tree.parent(candidate);
                            break;
                        }
                        positioned = tree.boxes[candidate].parent;
                    }
                }
                if text == "sup" && tree.boxes[parent].position == TerminalEquationPosition::Sub {
                    tree.boxes[parent].position = TerminalEquationPosition::Subsup;
                    tree.boxes[parent].expected_arguments = 3;
                    continue;
                }
                if text == "to" && tree.boxes[parent].position == TerminalEquationPosition::From {
                    tree.boxes[parent].position = TerminalEquationPosition::Fromto;
                    tree.boxes[parent].expected_arguments = 3;
                    continue;
                }
                let positioned = tree.make_binary(parent);
                tree.boxes[positioned].position = match text {
                    "sub" => TerminalEquationPosition::Sub,
                    "sup" => TerminalEquationPosition::Sup,
                    "from" => TerminalEquationPosition::From,
                    "to" => TerminalEquationPosition::To,
                    _ => unreachable!("matched eqn position keyword"),
                };
                parent = positioned;
            }
            Some("over") => {
                if tree.boxes[parent].children.is_empty() {
                    let _ = tree.add_text(parent, "", Some(TerminalEquationFont::Roman));
                }
                parent = tree.move_to_available(parent);
                while parent != 0 && tree.boxes[parent].kind == TerminalEquationKind::Subexpression
                {
                    parent = tree.parent(parent);
                }
                let fraction = tree.make_binary(parent);
                tree.boxes[fraction].position = TerminalEquationPosition::Over;
                parent = fraction;
            }
            Some("left" | "{") => {
                parent = tree.move_to_available(parent);
                let list = tree.allocate(parent);
                tree.boxes[list].kind = TerminalEquationKind::List;
                if text == "left" {
                    let delimiter = tokens.get(index).map_or("", |token| token.text.as_ref());
                    index = index.saturating_add(usize::from(tokens.get(index).is_some()));
                    tree.boxes[list].left = Some(terminal_equation_delimiter(delimiter).into());
                }
                parent = list;
            }
            Some("right" | "}") => {
                let mut candidate = Some(parent);
                let mut closing = None;
                while let Some(current) = candidate {
                    let box_ = &tree.boxes[current];
                    if box_.kind == TerminalEquationKind::List
                        && box_.expected_arguments > 1
                        && (text == "}" || box_.left.is_some())
                    {
                        closing = Some(current);
                        break;
                    }
                    candidate = box_.parent;
                }
                if let Some(closing) = closing {
                    if text == "right" {
                        let delimiter = tokens.get(index).map_or("", |token| token.text.as_ref());
                        index = index.saturating_add(usize::from(tokens.get(index).is_some()));
                        tree.boxes[closing].right =
                            Some(terminal_equation_delimiter(delimiter).into());
                    }
                    parent = tree.parent(closing);
                    if text == "}"
                        && matches!(
                            tree.boxes[parent].kind,
                            TerminalEquationKind::Pile | TerminalEquationKind::Matrix
                        )
                    {
                        parent = tree.parent(parent);
                    }
                    parent = tree.move_past_singletons(parent);
                }
            }
            Some("pile" | "lpile" | "rpile" | "cpile" | "ccol" | "lcol" | "rcol") => {
                parent = tree.move_to_available(parent);
                let pile = tree.allocate(parent);
                tree.boxes[pile].kind = TerminalEquationKind::Pile;
                tree.boxes[pile].expected_arguments = 1;
                parent = pile;
            }
            Some("above") => {
                let mut pile = Some(parent);
                while let Some(current) = pile {
                    if tree.boxes[current].kind == TerminalEquationKind::Pile {
                        let row = tree.allocate(current);
                        tree.boxes[row].kind = TerminalEquationKind::List;
                        parent = row;
                        break;
                    }
                    pile = tree.boxes[current].parent;
                }
            }
            Some("matrix") => {
                parent = tree.move_to_available(parent);
                let matrix = tree.allocate(parent);
                tree.boxes[matrix].kind = TerminalEquationKind::Matrix;
                tree.boxes[matrix].expected_arguments = 1;
                parent = matrix;
            }
            Some("dyad" | "vec" | "under" | "bar" | "tilde" | "hat" | "dot" | "dotdot") => {
                if tree.boxes[parent].children.is_empty() {
                    let _ = tree.add_text(parent, "", Some(TerminalEquationFont::Roman));
                }
                let decorated = tree.make_binary(parent);
                tree.boxes[decorated].kind = TerminalEquationKind::List;
                tree.boxes[decorated].expected_arguments = 1;
                tree.boxes[decorated].font = TerminalEquationFont::Roman;
                match text {
                    "under" => tree.boxes[decorated].bottom = Some("\\[ul]".into()),
                    "bar" => tree.boxes[decorated].top = Some("\\[rn]".into()),
                    "vec" => tree.boxes[decorated].top = Some("\\[->]".into()),
                    "dyad" => tree.boxes[decorated].top = Some("\\[<>]".into()),
                    "tilde" => tree.boxes[decorated].top = Some("\\[a~]".into()),
                    "hat" => tree.boxes[decorated].top = Some("\\[ha]".into()),
                    "dot" => tree.boxes[decorated].top = Some("\\[a.]".into()),
                    "dotdot" => tree.boxes[decorated].top = Some("\\[ad]".into()),
                    _ => unreachable!("matched eqn decoration keyword"),
                }
            }
            Some("define" | "ndefine" | "tdefine" | "undef" | "delim") => {
                // Parser-side preprocessing has already applied these requests.
            }
            _ => {
                parent = tree.move_to_available(parent);
                append_terminal_equation_text(&mut tree, parent, token);
            }
        }
    }
    tree
}

/// Scanner-stage escape normalization preserves a two-character roff escape
/// inside an eqn range as a bare backslash followed by its name. Rejoin that
/// bounded pair before device-box parsing so it remains one Roman symbol
/// rather than an empty text box plus italic prose.
fn coalesce_terminal_equation_escapes(
    tokens: &[EquationTerminalToken],
) -> Vec<EquationTerminalToken> {
    let mut output = Vec::with_capacity(tokens.len());
    let mut index = 0_usize;
    while let Some(token) = tokens.get(index) {
        if !token.quoted
            && token.text.as_ref() == "\\"
            && let Some(next) = tokens.get(index + 1).filter(|next| !next.quoted)
        {
            output.push(EquationTerminalToken {
                text: format!("\\({}", next.text).into(),
                quoted: false,
            });
            index += 2;
        } else {
            output.push(token.clone());
            index += 1;
        }
    }
    output
}

fn terminal_equation_delimiter(value: &str) -> &str {
    match value {
        "ceiling" => "\\[lc]",
        "floor" => "\\[lf]",
        other => other,
    }
}

fn append_terminal_equation_text(
    tree: &mut TerminalEquationTree,
    parent: usize,
    token: &EquationTerminalToken,
) {
    if token.quoted {
        let font = (tree.boxes[parent].font == TerminalEquationFont::None)
            .then_some(TerminalEquationFont::Italic);
        let node = tree.add_text(parent, token.text.clone(), font);
        tree.boxes[node].quoted = true;
        return;
    }
    let mapped = normalize_equation_symbol(&token.text);
    if mapped != token.text.as_ref() {
        let _ = tree.add_text(parent, mapped, None);
        return;
    }
    if token.text.starts_with("\\(") {
        let _ = tree.add_text(
            parent,
            token.text.clone(),
            Some(TerminalEquationFont::Roman),
        );
        return;
    }
    if equation_function(&token.text) {
        let _ = tree.add_text(
            parent,
            token.text.clone(),
            Some(TerminalEquationFont::Roman),
        );
        return;
    }
    if tree.boxes[parent].font != TerminalEquationFont::None || token.text.is_empty() {
        let _ = tree.add_text(parent, token.text.clone(), None);
        return;
    }
    let parts = split_terminal_equation_text(&token.text);
    let parent = if parts.len() > 1
        && tree.boxes[parent].children.len() + 1 >= tree.boxes[parent].expected_arguments
    {
        // Mandoc reparents a compound text box (for example `a+b`) into a
        // list before splitting it.  That keeps all pieces under a unary
        // operand such as `sqrt`, rather than letting only the first one be
        // consumed by the enclosing positional box.
        let list = tree.allocate(parent);
        tree.boxes[list].kind = TerminalEquationKind::List;
        list
    } else {
        parent
    };
    for (text, font) in parts {
        let _ = tree.add_text(parent, text, Some(font));
    }
}

fn equation_function(value: &str) -> bool {
    matches!(
        value,
        "acos"
            | "acsc"
            | "and"
            | "arc"
            | "asec"
            | "asin"
            | "atan"
            | "cos"
            | "cosh"
            | "coth"
            | "csc"
            | "det"
            | "exp"
            | "for"
            | "if"
            | "lim"
            | "ln"
            | "log"
            | "max"
            | "min"
            | "sec"
            | "sin"
            | "sinh"
            | "tan"
            | "tanh"
            | "Im"
            | "Re"
    )
}

fn split_terminal_equation_text(value: &str) -> Vec<(Box<str>, TerminalEquationFont)> {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Class {
        Letter,
        Digit,
        Punctuation,
    }
    fn class(character: char, previous: Option<Class>, next: Option<char>) -> Class {
        if character.is_ascii_alphabetic() {
            Class::Letter
        } else if character.is_ascii_digit()
            || (character == '.'
                && (previous == Some(Class::Digit)
                    || next.is_some_and(|character| character.is_ascii_digit())))
        {
            Class::Digit
        } else {
            Class::Punctuation
        }
    }

    let characters = value.chars().collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut start = 0_usize;
    let mut previous = None;
    for (index, character) in characters.iter().copied().enumerate() {
        let current = class(character, previous, characters.get(index + 1).copied());
        let boundary = index > 0
            && (current != previous.unwrap_or(current)
                || character == ','
                || characters[index - 1] == ',');
        if boundary {
            let text = characters[start..index].iter().collect::<String>();
            output.push((
                text.into_boxed_str(),
                if previous == Some(Class::Letter) {
                    TerminalEquationFont::Italic
                } else {
                    TerminalEquationFont::Roman
                },
            ));
            start = index;
        }
        previous = Some(current);
    }
    if start < characters.len() {
        let text = characters[start..].iter().collect::<String>();
        output.push((
            text.into_boxed_str(),
            if previous == Some(Class::Letter) {
                TerminalEquationFont::Italic
            } else {
                TerminalEquationFont::Roman
            },
        ));
    }
    output
}

#[derive(Default)]
struct TerminalEquationWriter {
    output: String,
    no_space: bool,
}

impl TerminalEquationWriter {
    fn attach(&mut self) {
        self.no_space = true;
    }

    fn word(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        if !self.output.is_empty() && !self.no_space {
            self.output.push(' ');
        }
        self.output.push_str(value);
        self.no_space = false;
    }
}

/// Render retained eqn boxes using the terminal device's compact positional
/// syntax (`_`, `^`, `/`, and overstrikes).  The final prose wrapping pass
/// still owns line width and indentation, exactly as for ordinary text.
fn render_terminal_equation(
    equation: &EquationTerminal,
    format: RenderFormat,
    limits: &Limits,
) -> String {
    let tree = parse_terminal_equation(&equation.tokens);
    let mut writer = TerminalEquationWriter::default();
    for child in tree.boxes[0].children.iter().copied() {
        render_terminal_equation_box(&tree, child, format, limits, &mut writer);
    }
    writer.output
}

fn render_terminal_equation_box(
    tree: &TerminalEquationTree,
    index: usize,
    format: RenderFormat,
    limits: &Limits,
    writer: &mut TerminalEquationWriter,
) {
    let box_ = &tree.boxes[index];
    let parent = box_.parent;
    let previous = tree.previous(index);
    let delimiter = (box_.kind == TerminalEquationKind::List && box_.expected_arguments > 1)
        || (box_.kind == TerminalEquationKind::Pile
            && (previous.is_some() || tree.next(index).is_some()))
        || parent.is_some_and(|parent| {
            let parent_box = &tree.boxes[parent];
            parent_box.position == TerminalEquationPosition::Sqrt
                || ((box_.top.is_some() || box_.bottom.is_some())
                    && parent_box.kind == TerminalEquationKind::Subexpression
                    && parent_box.position != TerminalEquationPosition::Over
                    && tree.next(index).is_some())
                || (box_.kind == TerminalEquationKind::Subexpression
                    && box_.position != TerminalEquationPosition::Sqrt
                    && ((parent_box.kind == TerminalEquationKind::List
                        && parent_box.expected_arguments == 1)
                        || (parent_box.kind == TerminalEquationKind::Subexpression
                            && box_.position != TerminalEquationPosition::Sqrt)))
        });
    if delimiter {
        let attach = parent.is_some_and(|parent| {
            (tree.boxes[parent].kind == TerminalEquationKind::Subexpression && previous.is_some())
                || (box_.kind == TerminalEquationKind::List
                    && tree.first(index).is_some_and(|first| {
                        !matches!(
                            tree.boxes[first].kind,
                            TerminalEquationKind::Pile | TerminalEquationKind::Matrix
                        )
                    })
                    && previous.is_some_and(|previous| {
                        tree.boxes[previous].kind == TerminalEquationKind::List
                            || (tree.boxes[previous].kind == TerminalEquationKind::Text
                                && tree.boxes[previous].text.as_deref().is_some_and(|text| {
                                    text.starts_with('\\') || text.starts_with(char::is_alphabetic)
                                }))
                    }))
        });
        if attach {
            writer.attach();
        }
        let parent_font = parent
            .map(|parent| tree.boxes[parent].font.terminal())
            .unwrap_or_default();
        writer.word(&render_terminal_font(
            &render_terminal_equation_text(box_.left.as_deref().unwrap_or("("), format, limits),
            parent_font,
        ));
        writer.attach();
    }

    if let Some(text) = box_.text.as_deref() {
        if text.starts_with(|character: char| {
            matches!(
                character,
                '!' | '\"' | '\'' | ')' | ',' | '.' | ':' | ';' | '?' | ']' | '}'
            )
        }) {
            writer.attach();
        }
        let rendered = render_terminal_equation_text(text, format, limits);
        writer.word(&render_terminal_font(&rendered, box_.font.terminal()));
        if text.ends_with(['"', '\'', '(', '[', '{'])
            || (previous.is_none() && (text.ends_with('-') || text.ends_with("\\[mi]")))
        {
            writer.attach();
        }
    }

    match box_.position {
        TerminalEquationPosition::Sqrt => {
            writer.word(&render_terminal_equation_text("\\(sr", format, limits));
            if let Some(child) = tree.first(index) {
                writer.attach();
                render_terminal_equation_box(tree, child, format, limits, writer);
            }
        }
        TerminalEquationPosition::Sup
        | TerminalEquationPosition::Sub
        | TerminalEquationPosition::Subsup
        | TerminalEquationPosition::To
        | TerminalEquationPosition::From
        | TerminalEquationPosition::Fromto
        | TerminalEquationPosition::Over => {
            let mut children = box_.children.iter().copied();
            if let Some(left) = children.next() {
                render_terminal_equation_box(tree, left, format, limits, writer);
            }
            writer.attach();
            writer.word(match box_.position {
                TerminalEquationPosition::Over => "/",
                TerminalEquationPosition::Sup | TerminalEquationPosition::To => "^",
                _ => "_",
            });
            if let Some(right) = children.next() {
                writer.attach();
                render_terminal_equation_box(tree, right, format, limits, writer);
            }
            if matches!(
                box_.position,
                TerminalEquationPosition::Subsup | TerminalEquationPosition::Fromto
            ) {
                writer.attach();
                writer.word("^");
                if let Some(upper) = children.next() {
                    writer.attach();
                    render_terminal_equation_box(tree, upper, format, limits, writer);
                }
            }
        }
        TerminalEquationPosition::None => {
            let mut children = box_.children.iter().copied();
            if box_.kind == TerminalEquationKind::Matrix
                && tree.first(index).is_some_and(|child| {
                    tree.boxes[child].kind == TerminalEquationKind::List
                        && tree.boxes[child].expected_arguments > 1
                })
            {
                children = tree.boxes[tree.first(index).expect("matrix has first child")]
                    .children
                    .iter()
                    .copied();
            }
            for child in children {
                let child = if box_.kind == TerminalEquationKind::Pile
                    && tree.boxes[child].kind == TerminalEquationKind::List
                    && tree.boxes[child].expected_arguments > 1
                    && tree.boxes[child].children.len() == 1
                {
                    tree.boxes[child].children[0]
                } else {
                    child
                };
                render_terminal_equation_box(tree, child, format, limits, writer);
            }
        }
    }

    if let Some(top) = box_.top.as_deref() {
        writer.attach();
        let parent_font = parent
            .map(|parent| tree.boxes[parent].font.terminal())
            .unwrap_or_default();
        writer.word(&render_terminal_font(
            &render_terminal_equation_text(top, format, limits),
            parent_font,
        ));
    }
    if box_.bottom.is_some() {
        writer.attach();
        writer.word("_");
    }
    if delimiter {
        writer.attach();
        let parent_font = parent
            .map(|parent| tree.boxes[parent].font.terminal())
            .unwrap_or_default();
        writer.word(&render_terminal_font(
            &render_terminal_equation_text(box_.right.as_deref().unwrap_or(")"), format, limits),
            parent_font,
        ));
        if let Some(parent) = parent
            && tree.boxes[parent].kind == TerminalEquationKind::Subexpression
            && tree.boxes[parent]
                .children
                .last()
                .is_some_and(|last| *last != index)
        {
            writer.attach();
        }
    }
}

/// Render the retained device eqn tree as the mathematical-markup fragment emitted by
/// mandoc's HTML backend.  It intentionally feeds the existing regression
/// extractor through the native eqn math element; surrounding HTML structure
/// remains the responsibility of the general native HTML renderer.
fn render_html_equation(equation: &EquationTerminal, limits: &Limits) -> String {
    let tree = parse_terminal_equation(&equation.tokens);
    if tree.boxes[0].children.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    render_html_equation_box(&tree, 0, limits, &mut output);
    output
}

fn render_html_equation_box(
    tree: &TerminalEquationTree,
    index: usize,
    limits: &Limits,
    output: &mut String,
) {
    let box_ = &tree.boxes[index];
    let post = match box_.position {
        TerminalEquationPosition::To => Some("mover"),
        TerminalEquationPosition::Sup => Some("msup"),
        TerminalEquationPosition::From => Some("munder"),
        TerminalEquationPosition::Sub => Some("msub"),
        TerminalEquationPosition::Over => Some("mfrac"),
        TerminalEquationPosition::Fromto => Some("munderover"),
        TerminalEquationPosition::Subsup => Some("msubsup"),
        TerminalEquationPosition::Sqrt => Some("msqrt"),
        TerminalEquationPosition::None if box_.top.is_some() && box_.bottom.is_some() => {
            Some("munderover")
        }
        TerminalEquationPosition::None if box_.top.is_some() => Some("mover"),
        TerminalEquationPosition::None if box_.bottom.is_some() => Some("munder"),
        TerminalEquationPosition::None
            if box_.kind == TerminalEquationKind::Pile
                && tree.first(index).is_some_and(|child| {
                    tree.boxes[child].kind == TerminalEquationKind::List
                        && tree.boxes[child].expected_arguments > 1
                }) =>
        {
            Some("mtable")
        }
        TerminalEquationPosition::None
            if box_.kind == TerminalEquationKind::List
                && box_.expected_arguments > 1
                && box_.parent.is_some_and(|parent| {
                    tree.boxes[parent].kind == TerminalEquationKind::Pile
                }) =>
        {
            Some("mtd")
        }
        TerminalEquationPosition::None => None,
    };

    if let Some(text) = box_.text.as_deref() {
        render_html_equation_text(text, box_.font, box_.quoted, limits, output);
        return;
    }
    if box_.kind == TerminalEquationKind::Matrix {
        render_html_equation_matrix(tree, index, limits, output);
        return;
    }

    if post == Some("mtd") {
        output.push_str("<mtr><mtd>");
    } else if let Some(post) = post {
        output.push('<');
        output.push_str(post);
        output.push('>');
    } else if box_.left.is_some() || box_.right.is_some() {
        output.push_str("<mfenced");
        if let Some(left) = box_.left.as_deref() {
            output.push_str(" open=\"");
            append_html_math_attribute(left, limits, output);
            output.push('"');
        }
        if let Some(right) = box_.right.as_deref() {
            output.push_str(" close=\"");
            append_html_math_attribute(right, limits, output);
            output.push('"');
        }
        output.push_str("><mrow>");
    } else {
        output.push_str("<mrow>");
    }

    for child in box_.children.iter().copied() {
        render_html_equation_box(tree, child, limits, output);
    }
    if let Some(bottom) = box_.bottom.as_deref() {
        render_html_equation_operator(bottom, limits, output);
    }
    if let Some(top) = box_.top.as_deref() {
        render_html_equation_operator(top, limits, output);
    }

    if post == Some("mtd") {
        output.push_str("</mtd></mtr>");
    } else if let Some(post) = post {
        output.push_str("</");
        output.push_str(post);
        output.push('>');
    } else if box_.left.is_some() || box_.right.is_some() {
        output.push_str("</mrow></mfenced>");
    } else {
        output.push_str("</mrow>");
    }
}

/// Matrix columns arrive in eqn source order, but mathematical markup requires rows. Each
/// ccol/lcol/rcol is represented by a private pile whose direct children are
/// the rows; transpose those bounded child lists without touching public AST
/// equation text.
fn render_html_equation_matrix(
    tree: &TerminalEquationTree,
    index: usize,
    limits: &Limits,
    output: &mut String,
) {
    let Some(scope) = tree.first(index) else {
        return;
    };
    let scope = &tree.boxes[scope];
    if scope.kind != TerminalEquationKind::List || scope.expected_arguments <= 1 {
        render_html_equation_box(
            tree,
            tree.first(index).expect("matrix child exists"),
            limits,
            output,
        );
        return;
    }
    let columns = &scope.children;
    let rows = columns
        .iter()
        .map(|column| tree.boxes[*column].children.len())
        .max()
        .unwrap_or(0);
    if rows == 0 {
        return;
    }
    output.push_str("<mtable>");
    for row in 0..rows {
        output.push_str("<mtr>");
        for column in columns {
            output.push_str("<mtd>");
            if let Some(cell) = tree.boxes[*column].children.get(row).copied() {
                let cell_box = &tree.boxes[cell];
                if cell_box.kind == TerminalEquationKind::List
                    && cell_box
                        .parent
                        .is_some_and(|parent| tree.boxes[parent].kind == TerminalEquationKind::Pile)
                {
                    for child in cell_box.children.iter().copied() {
                        render_html_equation_box(tree, child, limits, output);
                    }
                } else {
                    render_html_equation_box(tree, cell, limits, output);
                }
            }
            output.push_str("</mtd>");
        }
        output.push_str("</mtr>");
    }
    output.push_str("</mtable>");
}

fn render_html_equation_text(
    text: &str,
    font: TerminalEquationFont,
    quoted: bool,
    limits: &Limits,
    output: &mut String,
) {
    let mut visible = render_visible_text(text, RenderFormat::Utf8, limits);
    if quoted {
        visible = visible.replace(' ', "\n");
    }
    let mut characters = visible.chars();
    let first = characters.next();
    let tag = if text.starts_with("\\[") {
        "mo"
    } else if first.is_some_and(|character| character.is_ascii_digit())
        || (first == Some('.')
            && characters
                .next()
                .is_some_and(|character| character.is_ascii_digit()))
    {
        "mn"
    } else if first.is_some_and(|character| !character.is_alphabetic()) {
        if visible.chars().any(char::is_alphanumeric) {
            "mi"
        } else {
            "mo"
        }
    } else {
        "mi"
    };
    let default_font = if tag == "mi" && visible.chars().count() == 1 {
        TerminalEquationFont::Italic
    } else {
        TerminalEquationFont::Roman
    };
    output.push('<');
    output.push_str(tag);
    if font != TerminalEquationFont::None && font != default_font {
        match font {
            TerminalEquationFont::Roman => output.push_str(" fontstyle=\"normal\""),
            TerminalEquationFont::Bold | TerminalEquationFont::Fat => {
                output.push_str(" fontweight=\"bold\"");
            }
            TerminalEquationFont::Italic => output.push_str(" fontstyle=\"italic\""),
            TerminalEquationFont::None => {}
        }
    }
    output.push('>');
    append_html_math_text(&visible, output);
    output.push_str("</");
    output.push_str(tag);
    output.push('>');
}

fn render_html_equation_operator(text: &str, limits: &Limits, output: &mut String) {
    output.push_str("<mo>");
    append_html_math_text(
        &render_visible_text(text, RenderFormat::Utf8, limits),
        output,
    );
    output.push_str("</mo>");
}

fn append_html_math_attribute(text: &str, limits: &Limits, output: &mut String) {
    let visible = render_visible_text(text, RenderFormat::Utf8, limits);
    for character in visible.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '"' => output.push_str("&quot;"),
            _ => output.push(character),
        }
    }
}

fn append_html_math_text(text: &str, output: &mut String) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            character if character.is_ascii() => output.push(character),
            character => {
                use std::fmt::Write as _;
                let _ = write!(output, "&#x{:04X};", u32::from(character));
            }
        }
    }
}

/// Render an eqn expression with the terminal device's legacy ASCII names.
///
/// The ordinary text path intentionally turns unknown non-ASCII glyphs into
/// `?`.  Equation boxes carry the authored `\\[*…]` spelling, however, and
/// mandoc's ASCII device preserves the conventional Greek names instead.  Do
/// this before generic escape normalization so UTF-8 remains the catalog
/// glyph while ASCII retains the device's more useful textual form.
fn render_terminal_equation_text(text: &str, format: RenderFormat, limits: &Limits) -> String {
    if format != RenderFormat::Ascii {
        return render_visible_text(text, format, limits);
    }
    let bytes = text.as_bytes();
    let mut escaped = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\[")
            && let Some(close) = bytes[cursor + 2..].iter().position(|byte| *byte == b']')
        {
            let end = cursor + 2 + close;
            let name = &text[cursor + 2..end];
            if let Some(replacement) = ascii_equation_special_character(name) {
                escaped.push_str(replacement);
            } else {
                escaped.push_str(&text[cursor..=end]);
            }
            cursor = end + 1;
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        escaped.push(character);
        cursor += character.len_utf8();
    }
    render_visible_text(&escaped, format, limits)
}

/// The ASCII fallback spellings from mandoc 1.14.6's `chars.c` Greek table.
///
/// These are the entries emitted by eqn's canonical `\\[*…]` lowering.  The
/// rest of the character catalog continues through the normal terminal
/// fallback, which intentionally reports unsupported glyphs as `?`.
fn ascii_equation_special_character(name: &str) -> Option<&'static str> {
    let spelling = match name {
        "*A" => "A",
        "*B" => "B",
        "*G" => "<Gamma>",
        "*D" => "<Delta>",
        "*E" => "E",
        "*Z" => "Z",
        "*Y" => "H",
        "*H" => "<Theta>",
        "*I" => "I",
        "*K" => "K",
        "*L" => "<Lambda>",
        "*M" => "M",
        "*N" => "N",
        "*C" => "<Xi>",
        "*O" => "O",
        "*P" => "<Pi>",
        "*R" => "P",
        "*S" => "<Sigma>",
        "*T" => "T",
        "*U" => "Y",
        "*F" => "<Phi>",
        "*X" => "X",
        "*Q" => "<Psi>",
        "*W" => "<Omega>",
        "*a" => "<alpha>",
        "*b" => "<beta>",
        "*g" => "<gamma>",
        "*d" => "<delta>",
        "*e" => "<epsilon>",
        "*z" => "<zeta>",
        "*y" => "<eta>",
        "*h" => "<theta>",
        "*i" => "<iota>",
        "*k" => "<kappa>",
        "*l" => "<lambda>",
        "*m" => "<mu>",
        "*n" => "<nu>",
        "*c" => "<xi>",
        "*o" => "o",
        "*p" => "<pi>",
        "*r" => "<rho>",
        "*s" => "<sigma>",
        "*t" => "<tau>",
        "*u" => "<upsilon>",
        "*f" => "<phi>",
        "*x" => "<chi>",
        "*q" => "<psi>",
        "*w" => "<omega>",
        "+h" => "<theta>",
        "+f" => "<phi>",
        "+p" => "<pi>",
        "+e" => "<epsilon>",
        "ts" => "<sigma>",
        _ => return None,
    };
    Some(spelling)
}

fn ascii_terminal_character(character: char) -> char {
    if character.is_ascii() {
        character
    } else if matches!(character, '\u{2010}' | '\u{2011}' | '\u{2212}') {
        '-'
    } else if character == '\u{a0}' {
        ' '
    } else {
        '?'
    }
}

/// Apply the 1.14.6 device's whitespace-escape recovery before generic
/// named-character normalization. Bracketed control spellings are silently
/// zero-width, while malformed names with a leading blank lose only their
/// introducer and keep the remaining authored bytes.
fn render_terminal_whitespace_escapes(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        // Bracketed acute/grave spellings are source-visible invalid forms.
        // Package parsing keeps them for diagnostics, whereas the renderer
        // consumes them as zero-width controls. Their one-byte counterparts
        // remain ordinary visible accents.
        if bytes.get(cursor..cursor.saturating_add(4)) == Some(b"\\[']")
            || bytes.get(cursor..cursor.saturating_add(4)) == Some(b"\\[`]")
        {
            cursor += 4;
            continue;
        }
        if bytes.get(cursor..cursor.saturating_add(4)) == Some(b"\\[_]")
            || bytes.get(cursor..cursor.saturating_add(4)) == Some(b"\\[~]")
            || bytes.get(cursor..cursor.saturating_add(4)) == Some(b"\\[0]")
        {
            cursor += 4;
            continue;
        }
        if bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\~")
            || bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\0")
        {
            output.push(' ');
            cursor += 2;
            continue;
        }
        if bytes.get(cursor..cursor.saturating_add(3)) == Some(b"\\[ ") {
            cursor += 3;
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        output.push(character);
        cursor += character.len_utf8();
    }
    output
}

/// Resolve terminal-visible text, including the inline bold form emitted by
/// man and mdoc sources. The public AST keeps font escapes verbatim, while
/// terminal output uses the same deterministic overstrike convention as
/// structural headings and `.B` elements.
fn render_terminal_visible_text(text: &str, format: RenderFormat, limits: &Limits) -> String {
    render_terminal_visible_text_with_font(text, format, limits, TerminalFont::Roman)
}

fn render_terminal_visible_text_with_font(
    text: &str,
    format: RenderFormat,
    limits: &Limits,
    initial_font: TerminalFont,
) -> String {
    // The library catalogue synthesizes the traditional two-character quote
    // names. Resolve them here with ordinary terminal text, so generated
    // unknown-library prose follows the same delimiter joining as authored
    // quotation marks.
    let text = match format {
        RenderFormat::Utf8 => text.replace(r"\(lq", "“").replace(r"\(rq", "”"),
        RenderFormat::Ascii | RenderFormat::Html => {
            text.replace(r"\(lq", "\"").replace(r"\(rq", "\"")
        }
    }
    // These traditional guillemet names use a two-cell ASCII terminal
    // fallback in mandoc rather than the generic non-ASCII replacement.
    .replace(
        r"\(Fo",
        if matches!(format, RenderFormat::Ascii) {
            "<<"
        } else {
            "«"
        },
    )
    .replace(
        r"\(Fc",
        if matches!(format, RenderFormat::Ascii) {
            ">>"
        } else {
            "»"
        },
    )
    .replace(r"\:", "\u{1a}");
    let text = render_terminal_roff_controls(&text, format, limits);
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut fragment = String::new();
    let mut cursor = 0_usize;
    let mut font = initial_font;
    let mut previous_font = initial_font;
    while cursor < bytes.len() {
        if let Some((next_cursor, change)) = terminal_font_escape(bytes, cursor) {
            let visible = render_terminal_visible_fragment(&fragment, format, limits);
            output.push_str(&render_terminal_font(&visible, font));
            fragment.clear();
            match change {
                TerminalFontChange::Set(next_font) => {
                    previous_font = font;
                    font = next_font;
                }
                TerminalFontChange::Restore => std::mem::swap(&mut font, &mut previous_font),
            }
            cursor = next_cursor;
            continue;
        }
        if text[cursor..].starts_with(TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER) {
            let visible = render_terminal_visible_fragment(&fragment, format, limits);
            output.push_str(&render_terminal_font(&visible, font));
            fragment.clear();
            output.push('\u{8}');
            cursor += TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER.len_utf8();
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        fragment.push(character);
        cursor += character.len_utf8();
    }
    let visible = render_terminal_visible_fragment(&fragment, format, limits);
    output.push_str(&render_terminal_font(&visible, font));
    output
}

/// Decode the terminal device's accepted roff font selectors. Mandoc maps
/// bold-italic selectors to its underline/italic terminal convention, while
/// fixed-width aliases only select a style for the following fragment.
fn terminal_font_escape(bytes: &[u8], cursor: usize) -> Option<(usize, TerminalFontChange)> {
    if bytes.get(cursor..cursor.saturating_add(2)) != Some(b"\\f") {
        return None;
    }
    let selector_start = cursor.saturating_add(2);
    let selector = *bytes.get(selector_start)?;
    let (name, next_cursor) = match selector {
        b'(' => {
            let name =
                bytes.get(selector_start.saturating_add(1)..selector_start.saturating_add(3))?;
            (name, selector_start.saturating_add(3))
        }
        b'[' => {
            let closing = bytes[selector_start.saturating_add(1)..]
                .iter()
                .position(|byte| *byte == b']')?;
            let name_end = selector_start.saturating_add(1).saturating_add(closing);
            (
                bytes.get(selector_start.saturating_add(1)..name_end)?,
                name_end.saturating_add(1),
            )
        }
        _ => (
            &bytes[selector_start..selector_start.saturating_add(1)],
            selector_start.saturating_add(1),
        ),
    };
    let change = match name {
        b"B" | b"3" | b"CB" => TerminalFontChange::Set(TerminalFont::Bold),
        b"I" | b"2" | b"CI" => TerminalFontChange::Set(TerminalFont::Italic),
        b"4" | b"BI" => TerminalFontChange::Set(TerminalFont::BoldItalic),
        b"R" | b"1" | b"" | b"CW" | b"CR" => TerminalFontChange::Set(TerminalFont::Roman),
        b"P" => TerminalFontChange::Restore,
        _ => return None,
    };
    Some((next_cursor, change))
}

/// Reconstruct the `.ft` register immediately before a text node.  The
/// renderer stays re-entrant because this walks immutable ancestry and prior
/// siblings rather than storing document-global device state.
fn html_request_font_before(node: NodeRef<'_>) -> HtmlRequestFontState {
    let mut lineage = vec![node];
    let mut cursor = node;
    while let Some(parent) = cursor.parent() {
        lineage.push(parent);
        cursor = parent;
    }
    lineage.reverse();

    let mut state = HtmlRequestFontState::default();
    for current in lineage.into_iter().skip(1) {
        let Some(parent) = current.parent() else {
            continue;
        };
        for sibling in parent.children() {
            if sibling.id() == current.id() {
                break;
            }
            html_apply_font_requests(sibling, &mut state);
        }
    }
    state
}

fn html_apply_font_requests(node: NodeRef<'_>, state: &mut HtmlRequestFontState) {
    if node.kind() == NodeKind::Element && node.macro_name() == Some("ft") {
        html_apply_font_request(node.children().find_map(NodeRef::text), state);
        return;
    }
    for child in node.children() {
        html_apply_font_requests(child, state);
    }
}

fn html_apply_font_request(selector: Option<&str>, state: &mut HtmlRequestFontState) {
    let next = match selector.unwrap_or_default() {
        "B" => Some(HtmlFont::Bold),
        "I" => Some(HtmlFont::Italic),
        "BI" => Some(HtmlFont::BoldItalic),
        "CR" | "CW" => Some(HtmlFont::LiteralRoman),
        "CB" => Some(HtmlFont::LiteralBold),
        "CI" => Some(HtmlFont::LiteralItalic),
        "R" => Some(HtmlFont::Roman),
        // roff's HTML device accepts `.ft P` and an empty `.ft` but keeps
        // its already-open HTML font wrapper.  Inline `\fP` remains a real
        // swap in `html_font_escape`; this is request-specific behaviour.
        "" | "P" => None,
        _ => None,
    };
    if let Some(next) = next {
        state.previous = state.current;
        state.current = next;
    }
}

/// Render HTML-visible text while retaining roff's inline font changes and
/// the preceding `.ft` device selection.
///
/// The parser deliberately keeps `\\f` spellings in compatible text.  The
/// generic escape normalizer correctly removes their controls, but HTML must
/// first turn the known device selections into the reference's semantic
/// inline elements.  Literal (`C*`) selections use the same `Li` wrapper as
/// structural literal mdoc nodes.
fn render_html_visible_text_with_font(
    text: &str,
    limits: &Limits,
    initial_font: HtmlFont,
) -> String {
    let bytes = text.as_bytes();
    let mut output = String::new();
    let mut fragment = String::new();
    let mut cursor = 0_usize;
    let mut font = initial_font;
    let mut previous_font = font;
    while cursor < bytes.len() {
        if let Some((next_cursor, change)) = html_font_escape(bytes, cursor) {
            append_html_font_fragment(&fragment, font, limits, &mut output);
            fragment.clear();
            match change {
                HtmlFontChange::Set(next_font) => {
                    previous_font = font;
                    font = next_font;
                }
                HtmlFontChange::Restore => std::mem::swap(&mut font, &mut previous_font),
            }
            cursor = next_cursor;
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        fragment.push(character);
        cursor += character.len_utf8();
    }
    append_html_font_fragment(&fragment, font, limits, &mut output);
    output
}

fn append_html_font_fragment(fragment: &str, font: HtmlFont, limits: &Limits, output: &mut String) {
    if fragment.is_empty() {
        return;
    }
    let visible = escape_html(&render_visible_text(fragment, RenderFormat::Html, limits));
    let (prefix, suffix) = match font {
        HtmlFont::Roman => ("", ""),
        HtmlFont::Bold => ("<b>", "</b>"),
        HtmlFont::Italic => ("<i>", "</i>"),
        HtmlFont::BoldItalic => ("<b><i>", "</i></b>"),
        HtmlFont::LiteralRoman => ("<span class=\"Li\">", "</span>"),
        HtmlFont::LiteralBold => ("<span class=\"Li\"><b>", "</b></span>"),
        HtmlFont::LiteralItalic => ("<span class=\"Li\"><i>", "</i></span>"),
    };
    output.push_str(prefix);
    output.push_str(&visible);
    output.push_str(suffix);
}

fn html_font_escape(bytes: &[u8], cursor: usize) -> Option<(usize, HtmlFontChange)> {
    if bytes.get(cursor..cursor.saturating_add(2)) != Some(b"\\f") {
        return None;
    }
    let selector_start = cursor.saturating_add(2);
    let selector = *bytes.get(selector_start)?;
    let (name, next_cursor) = match selector {
        b'(' => {
            let name =
                bytes.get(selector_start.saturating_add(1)..selector_start.saturating_add(3))?;
            (name, selector_start.saturating_add(3))
        }
        b'[' => {
            let closing = bytes[selector_start.saturating_add(1)..]
                .iter()
                .position(|byte| *byte == b']')?;
            let name_end = selector_start.saturating_add(1).saturating_add(closing);
            (
                bytes.get(selector_start.saturating_add(1)..name_end)?,
                name_end.saturating_add(1),
            )
        }
        _ => (
            &bytes[selector_start..selector_start.saturating_add(1)],
            selector_start.saturating_add(1),
        ),
    };
    let change = match name {
        b"B" | b"3" => HtmlFontChange::Set(HtmlFont::Bold),
        b"I" | b"2" => HtmlFontChange::Set(HtmlFont::Italic),
        b"4" | b"BI" => HtmlFontChange::Set(HtmlFont::BoldItalic),
        b"CW" | b"CR" => HtmlFontChange::Set(HtmlFont::LiteralRoman),
        b"CB" => HtmlFontChange::Set(HtmlFont::LiteralBold),
        b"CI" => HtmlFontChange::Set(HtmlFont::LiteralItalic),
        b"R" | b"1" | b"" => HtmlFontChange::Set(HtmlFont::Roman),
        b"P" => HtmlFontChange::Restore,
        _ => return None,
    };
    Some((next_cursor, change))
}

/// Resolve one terminal text fragment while retaining non-breaking roff
/// spaces until the width pass.  The public AST preserves their source
/// spelling, so this renderer-only conversion cannot alter parser or engine
/// semantics.
fn render_terminal_visible_fragment(text: &str, format: RenderFormat, limits: &Limits) -> String {
    let bytes = text.as_bytes();
    let mut marked = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if matches!(
            bytes.get(cursor..cursor.saturating_add(2)),
            Some(b"\\~" | b"\\0" | b"\\ ")
        ) {
            marked.push(TERMINAL_NONBREAKING_SPACE_MARKER);
            cursor += 2;
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        marked.push(character);
        cursor += character.len_utf8();
    }
    render_visible_text(&marked, format, limits)
}

/// Resolve terminal-only roff controls that deliberately remain authored in
/// the compatible AST. These controls change device motion or presentation,
/// not document text: `\O` suppresses output, `\o` overstrikes its payload,
/// `\l` draws a terminal rule, and `\h` advances the current field.
fn render_terminal_roff_controls(text: &str, format: RenderFormat, limits: &Limits) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes.get(cursor) != Some(&b'\\') {
            let character = text[cursor..]
                .chars()
                .next()
                .expect("cursor remains within a valid UTF-8 string");
            output.push(character);
            cursor += character.len_utf8();
            continue;
        }
        match bytes.get(cursor + 1) {
            Some(b'k') => {
                // Position-register interpolation only records the current
                // device column.  Its traditional, two-character, and
                // bracketed names are all presentation-invisible.
                cursor = terminal_named_roff_argument_end(bytes, cursor);
            }
            Some(b'R' | b'A') => {
                // Number-register and numeric-expression escapes are
                // terminal state.  Nested quoted controls are legal inside
                // their payload, so consume the complete quoted form rather
                // than stopping at the first inner quote.
                cursor = terminal_quoted_roff_control_end(text, cursor)
                    .unwrap_or_else(|| cursor.saturating_add(2).min(bytes.len()));
            }
            Some(b's')
                if matches!(
                    bytes.get(cursor + 2),
                    Some(b'+' | b'-' | b'0'..=b'9' | b'(' | b'[' | b'\'')
                ) =>
            {
                // Font-size requests alter the device but never emit their
                // selector.  They accept the same compact, parenthesized,
                // bracketed, and quoted forms as the stable formatter.
                let argument = cursor.saturating_add(2);
                cursor = if matches!(bytes.get(argument), Some(b'+' | b'-')) {
                    let size = argument.saturating_add(1);
                    if bytes.get(size) == Some(&b'\'') {
                        terminal_quoted_roff_argument_end(text, size)
                            .unwrap_or_else(|| size.min(bytes.len()))
                    } else {
                        terminal_roff_argument_end(bytes, size)
                    }
                } else if bytes.get(argument) == Some(&b'\'') {
                    terminal_quoted_roff_control_end(text, cursor)
                        .unwrap_or_else(|| cursor.saturating_add(2).min(bytes.len()))
                } else {
                    terminal_named_roff_argument_end(bytes, cursor)
                };
            }
            Some(b'O') => {
                // The terminal capability escape has no printable payload.
                // Its argument is one byte, a two-byte `\O(..)` name, or a
                // bracketed name; all variants are ignored by mandoc's
                // standard terminal device.
                cursor = match bytes.get(cursor + 2) {
                    Some(b'(') if cursor + 5 <= bytes.len() => cursor + 5,
                    Some(b'[') => bytes[cursor + 3..]
                        .iter()
                        .position(|byte| *byte == b']')
                        .map_or(bytes.len(), |offset| cursor + 4 + offset),
                    Some(_) => cursor.saturating_add(3).min(bytes.len()),
                    None => bytes.len(),
                };
            }
            Some(b'o') => {
                let Some((payload, next)) = terminal_quoted_roff_control(text, cursor) else {
                    output.push('\\');
                    cursor += 1;
                    continue;
                };
                terminal_append_overstrike(payload, &mut output);
                cursor = next;
            }
            Some(b'l') => {
                let Some((payload, next)) = terminal_quoted_roff_control(text, cursor) else {
                    cursor = cursor.saturating_add(3).min(bytes.len());
                    continue;
                };
                let (scale, fill) = terminal_roff_rule_parts(payload);
                if let Some(width) = terminal_signed_roff_en_prefix(scale) {
                    let fill = if fill.is_empty() { "_" } else { fill };
                    let fill_width =
                        display_width(&render_visible_text(fill, format, limits)).max(1);
                    for _ in 0..width.max(0).unsigned_abs() / fill_width {
                        output.push_str(fill);
                    }
                }
                cursor = next;
            }
            Some(b'z') => {
                let (atom, next, zero_width) = terminal_zero_width_roff_atom(text, cursor);
                output.push_str(&atom);
                if zero_width {
                    output.push(TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER);
                }
                cursor = next;
            }
            Some(b'h') => {
                let Some((payload, next)) = terminal_quoted_roff_control(text, cursor) else {
                    cursor = cursor.saturating_add(3).min(bytes.len());
                    continue;
                };
                if let Some(target) = payload.strip_prefix('|') {
                    if let Some(target) = terminal_signed_roff_en_prefix(target) {
                        output.push_str(
                            &" ".repeat(
                                target
                                    .max(0)
                                    .unsigned_abs()
                                    .saturating_sub(display_width(&output)),
                            ),
                        );
                    }
                } else if let Some(delta) = terminal_signed_roff_en_prefix(payload)
                    && delta.is_positive()
                {
                    output.push_str(&" ".repeat(delta.unsigned_abs()));
                }
                cursor = next;
            }
            Some(b'p') => {
                // `\p` takes no argument.  If it is followed by source
                // whitespace it breaks immediately; otherwise it attaches
                // the next word to its left neighbor and breaks before the
                // following word.  Defer the actual newline until the text
                // layout path knows the active field indentation.
                cursor = cursor.saturating_add(2).min(bytes.len());
                if bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                        cursor += 1;
                    }
                    output.push(TERMINAL_PENDING_LINE_BREAK_MARKER);
                } else {
                    while let Some(byte) = bytes.get(cursor) {
                        if byte.is_ascii_whitespace() {
                            break;
                        }
                        let character = text[cursor..]
                            .chars()
                            .next()
                            .expect("cursor remains within a valid UTF-8 string");
                        output.push(character);
                        cursor += character.len_utf8();
                    }
                    output.push(TERMINAL_PENDING_LINE_BREAK_MARKER);
                    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                        cursor += 1;
                    }
                }
            }
            Some(b'!' | b'?') => {
                // Mandoc recognizes these as unsupported formatter controls:
                // retain their diagnostics in the AST, but emit no terminal
                // glyph or authored backslash.
                cursor = cursor.saturating_add(2).min(bytes.len());
            }
            Some(
                code @ (b'+' | b';' | b'<' | b'=' | b'>' | b'@' | b']' | b'1' | b'G' | b'I' | b'i'
                | b'J' | b'j' | b'K' | b'P' | b'Q' | b'q' | b'T' | b'U' | b'W' | b'y'),
            ) => {
                // Invalid one-byte escapes preserve their spelling's payload
                // while the terminal device consumes only the introducer.
                output.push(char::from(*code));
                cursor = cursor.saturating_add(2).min(bytes.len());
            }
            _ => {
                output.push('\\');
                cursor += 1;
            }
        }
    }
    output
}

fn terminal_append_overstrike(payload: &str, output: &mut String) {
    for (index, character) in payload.chars().enumerate() {
        if index > 0 {
            output.push('\u{8}');
        }
        output.push(character);
    }
}

/// Return the first byte after a traditional roff name/argument atom.
fn terminal_named_roff_argument_end(bytes: &[u8], cursor: usize) -> usize {
    terminal_roff_argument_end(bytes, cursor.saturating_add(2))
}

/// Return the first byte after a roff name/argument beginning at `start`.
fn terminal_roff_argument_end(bytes: &[u8], start: usize) -> usize {
    match bytes.get(start) {
        Some(b'(') => start.saturating_add(3).min(bytes.len()),
        Some(b'[') => bytes[start.saturating_add(1)..]
            .iter()
            .position(|byte| *byte == b']')
            .map_or(bytes.len(), |offset| {
                start.saturating_add(2).saturating_add(offset)
            }),
        Some(_) => start.saturating_add(1).min(bytes.len()),
        None => bytes.len(),
    }
}

/// Consume one quoted roff control, including nested quoted escapes.
fn terminal_quoted_roff_control_end(text: &str, cursor: usize) -> Option<usize> {
    terminal_quoted_roff_argument_end(text, cursor.saturating_add(2))
}

/// Consume a quoted roff argument whose delimiter sits at `delimiter_index`.
fn terminal_quoted_roff_argument_end(text: &str, delimiter_index: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let delimiter = *bytes.get(delimiter_index)?;
    let mut position = delimiter_index.saturating_add(1);
    while position < bytes.len() {
        if bytes[position] == delimiter {
            return Some(position.saturating_add(1));
        }
        if bytes[position] == b'\\'
            && matches!(
                bytes.get(position.saturating_add(1)),
                Some(b'R' | b'A' | b'w' | b's')
            )
            && bytes.get(position.saturating_add(2)).is_some()
        {
            position = terminal_quoted_roff_control_end(text, position)?;
            continue;
        }
        let character = text[position..].chars().next()?;
        position = position.saturating_add(character.len_utf8());
    }
    None
}

/// Consume the one roff atom owned by `\z`. Unlike ordinary source text the
/// atom returns the terminal cursor to its original column, represented by a
/// trailing backspace after its printable projection. A nested `\z` is left
/// for the next scanner iteration so repeated zero-width controls do not
/// manufacture extra motion.
fn terminal_zero_width_roff_atom(text: &str, cursor: usize) -> (String, usize, bool) {
    let bytes = text.as_bytes();
    let start = cursor.saturating_add(2);
    let Some(&first) = bytes.get(start) else {
        return (String::new(), bytes.len(), false);
    };
    if first != b'\\' {
        let character = text[start..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        let next = start + character.len_utf8();
        return (
            character.to_string(),
            next,
            !character.is_whitespace() && next < bytes.len(),
        );
    }
    match bytes.get(start + 1) {
        Some(b'z') => (String::new(), start, false),
        Some(b'c' | b'&') => (
            String::new(),
            start.saturating_add(2).min(bytes.len()),
            false,
        ),
        Some(b'f') => {
            let Some((font_end, _)) = terminal_font_escape(bytes, start) else {
                return (
                    String::new(),
                    start.saturating_add(2).min(bytes.len()),
                    false,
                );
            };
            let Some(character) = text[font_end..].chars().next() else {
                return (text[start..font_end].to_owned(), font_end, false);
            };
            let next = font_end + character.len_utf8();
            (
                format!("{}{}", &text[start..font_end], character),
                next,
                !character.is_whitespace() && next < bytes.len(),
            )
        }
        Some(b'(') if start + 4 <= bytes.len() => {
            let next = start + 4;
            (text[start..next].to_owned(), next, next < bytes.len())
        }
        Some(b'[') => {
            let next = bytes[start + 2..]
                .iter()
                .position(|byte| *byte == b']')
                .map_or(bytes.len(), |offset| start + 3 + offset);
            (text[start..next].to_owned(), next, next < bytes.len())
        }
        Some(b'o') => {
            let Some((payload, next)) = terminal_quoted_roff_control(text, start) else {
                return (
                    String::new(),
                    start.saturating_add(2).min(bytes.len()),
                    false,
                );
            };
            let mut overstrike = String::new();
            terminal_append_overstrike(payload, &mut overstrike);
            (overstrike, next, !payload.is_empty() && next < bytes.len())
        }
        Some(_) | None => (
            String::new(),
            start.saturating_add(2).min(bytes.len()),
            false,
        ),
    }
}

/// Decode the quoted argument common to roff's `\o`, `\l`, and `\h`
/// controls. An unterminated argument remains authored, so callers can leave
/// normal escape normalization and diagnostics unchanged.
fn terminal_quoted_roff_control(text: &str, cursor: usize) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    let delimiter = *bytes.get(cursor + 2)?;
    let payload_start = cursor + 3;
    let end = bytes[payload_start..]
        .iter()
        .position(|byte| *byte == delimiter)?
        + payload_start;
    Some((&text[payload_start..end], end + 1))
}

/// Split `\l`'s scale prefix from its optional fill character.  Roff's
/// default scale unit is `n`; only known explicit unit letters consume one
/// byte before the fill spelling begins.
fn terminal_roff_rule_parts(payload: &str) -> (&str, &str) {
    let bytes = payload.as_bytes();
    let mut end = 0_usize;
    while matches!(bytes.get(end), Some(b'+' | b'-' | b'.' | b'0'..=b'9')) {
        end += 1;
    }
    if matches!(
        bytes.get(end),
        Some(b'c' | b'i' | b'f' | b'M' | b'm' | b'n' | b'P' | b'v' | b'p' | b'u')
    ) {
        end += 1;
    }
    payload.split_at(end)
}

/// Resolve the two quoted Unicode forms consumed by mandoc's terminal
/// device. They remain renderer-only because the public AST deliberately
/// retains their authored spelling for diagnostics and lowering fidelity.
fn render_unicode_character_escapes(text: &str, format: RenderFormat) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\[")
            && let Some(close) = bytes[cursor + 2..]
                .iter()
                .position(|byte| *byte == b']')
                .map(|offset| cursor + 2 + offset)
            && let Some(name) = text.get(cursor + 2..close)
            && let Some(value) = name.strip_prefix('u')
            && let Some(character) = canonical_unicode_scalar(value)
        {
            if character <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&character) {
                push_renderer_device_character(&mut output, character, format);
            } else if format == RenderFormat::Ascii
                && !character.is_ascii()
                && !ascii_terminal_named_scalar_is_known(character)
            {
                // Numeric Unicode names use mandoc's explicit unknown-glyph
                // notation, which differs from an arbitrary UTF-8 scalar in
                // authored terminal prose.
                output.push_str("<?>");
            } else {
                push_renderer_resolved_character(&mut output, character);
            }
            cursor = close + 1;
            continue;
        }
        let escaped =
            bytes.get(cursor) == Some(&b'\\') && matches!(bytes.get(cursor + 1), Some(b'U' | b'C'));
        let Some(&quote) = bytes
            .get(cursor + 2)
            .filter(|quote| matches!(quote, b'\'' | b'"'))
        else {
            let character = text[cursor..]
                .chars()
                .next()
                .expect("cursor remains within a valid UTF-8 string");
            output.push(character);
            cursor += character.len_utf8();
            continue;
        };
        if !escaped {
            let character = text[cursor..]
                .chars()
                .next()
                .expect("cursor remains within a valid UTF-8 string");
            output.push(character);
            cursor += character.len_utf8();
            continue;
        }
        let value_start = cursor + 3;
        let Some(close) = bytes[value_start..]
            .iter()
            .position(|byte| *byte == quote)
            .map(|offset| value_start + offset)
        else {
            output.push('\\');
            cursor += 1;
            continue;
        };
        let value = &text[value_start..close];
        if bytes[cursor + 1] == b'U' {
            // The pinned 1.14.6 terminal device has no `\U` scalar escape:
            // it drops the escape introducer and leaves the authored `U…`
            // spelling visible. Preserve that compatibility rather than
            // projecting a newer roff extension into the reference output.
            output.push_str(&text[cursor + 1..=close]);
            cursor = close + 1;
            continue;
        }
        let character = named_unicode_scalar(value);
        if let Some(character) = character {
            push_renderer_device_character(&mut output, character, format);
            cursor = close + 1;
        } else {
            output.push_str(&text[cursor..=close]);
            cursor = close + 1;
        }
    }
    output
}

fn unicode_scalar(value: &str) -> Option<char> {
    (4..=6)
        .contains(&value.len())
        .then(|| u32::from_str_radix(value, 16).ok())
        .flatten()
        .and_then(char::from_u32)
}

fn canonical_unicode_scalar(value: &str) -> Option<char> {
    let character = unicode_scalar(value)?;
    let scalar = u32::from(character);
    let canonical_length = if scalar <= 0xffff {
        4
    } else {
        format!("{scalar:X}").len()
    };
    (value.len() == canonical_length).then_some(character)
}

fn named_unicode_scalar(value: &str) -> Option<char> {
    value
        .strip_prefix('u')
        .and_then(unicode_scalar)
        .or_else(|| match crate::special_character(value) {
            Some(crate::SpecialCharacter::Visible(character)) => Some(character),
            Some(crate::SpecialCharacter::ZeroWidth) | None => None,
        })
}

/// Convert valid single-byte `\\N'number'` escapes before generic escape
/// normalization. Invalid and out-of-device-range numbers are suppressed, as
/// the stable terminal device does; malformed spellings stay available to the
/// generic normalizer for conservative recovery.
fn render_numeric_character_escapes(text: &str, format: RenderFormat) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\N") {
            let quote = bytes.get(cursor + 2).copied();
            if matches!(quote, Some(b'\'' | b'\"')) {
                let number_start = cursor + 3;
                let digits = bytes[number_start..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit())
                    .count();
                let number_end = number_start + digits;
                if bytes.get(number_end).is_some() {
                    if let Ok(number) = std::str::from_utf8(&bytes[number_start..number_end])
                        && let Ok(number) = number.parse::<u8>()
                        && let Some(character) = char::from_u32(u32::from(number))
                    {
                        push_renderer_device_character(&mut output, character, format);
                    }
                    // The legacy device accepts only an immediate matching
                    // quote. On a malformed spelling it still consumes the
                    // first non-numeric byte before returning the remaining
                    // source to ordinary text flow.
                    cursor = number_end + 1;
                    continue;
                }
            } else if quote.is_some_and(|byte| !byte.is_ascii_digit()) {
                let number_start = cursor + 3;
                let digits = bytes[number_start..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit())
                    .count();
                let number_end = number_start + digits;
                if digits > 0 && bytes.get(number_end).is_some() {
                    if let Ok(number) = std::str::from_utf8(&bytes[number_start..number_end])
                        && let Ok(number) = number.parse::<u8>()
                        && let Some(character) = char::from_u32(u32::from(number))
                    {
                        push_renderer_device_character(&mut output, character, format);
                    }
                    cursor = number_end + 1;
                    continue;
                }
                // With no valid quoted-like number, consume the introducer
                // and its first argument byte as the stable recovery does.
                cursor += 3;
                continue;
            } else {
                // The escape introducer is consumed even without its required
                // quote; its first following byte becomes the malformed
                // delimiter and the remaining bytes stay visible.
                cursor = cursor.saturating_add(3).min(bytes.len());
                continue;
            }
        }
        if bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\-") {
            output.push('-');
            cursor += 2;
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor always points at a UTF-8 character boundary");
        output.push(character);
        cursor += character.len_utf8();
    }
    output
}

/// Keep renderer-produced backslashes inert until the authored escape stream
/// has been normalized exactly once.
fn push_renderer_resolved_character(output: &mut String, character: char) {
    output.push(if character == '\\' {
        RENDER_LITERAL_BACKSLASH_MARKER
    } else {
        character
    });
}

/// The formatter does not send C0/C1 controls to its output device.  A named
/// or numeric roff escape representing one is rendered as the device's
/// printable control notation in ASCII, and as U+FFFD everywhere else.
/// Keeping this at escape resolution time also prevents literal newline and
/// tab scalars from changing the renderer's structural layout.
fn push_renderer_device_character(output: &mut String, character: char, format: RenderFormat) {
    if character == '\t' {
        // Horizontal tabs retain their device tab-stop semantics in every
        // output format; unlike other controls, they are layout, not a
        // replacement glyph.
        output.push('\t');
    } else if character <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&character) {
        match format {
            RenderFormat::Ascii => output.push_str(ascii_terminal_control_name(character)),
            RenderFormat::Utf8 | RenderFormat::Html => output.push('\u{fffd}'),
        }
    } else {
        push_renderer_resolved_character(output, character);
    }
}

fn ascii_terminal_control_name(character: char) -> &'static str {
    match character {
        '\0' => "<NUL>",
        '\u{1}' => "<SOH>",
        '\u{2}' => "<STX>",
        '\u{3}' => "<ETX>",
        '\u{4}' => "<EOT>",
        '\u{5}' => "<ENQ>",
        '\u{6}' => "<ACK>",
        '\u{7}' => "<BEL>",
        '\u{8}' => "<BS>",
        '\t' => "\t",
        '\n' => "<LF>",
        '\u{b}' => "<VT>",
        '\u{c}' => "<FF>",
        '\r' => "<CR>",
        '\u{e}' => "<SO>",
        '\u{f}' => "<SI>",
        '\u{10}' => "<DLE>",
        '\u{11}' => "<DC1>",
        '\u{12}' => "<DC2>",
        '\u{13}' => "<DC3>",
        '\u{14}' => "<DC4>",
        '\u{15}' => "<NAK>",
        '\u{16}' => "<SYN>",
        '\u{17}' => "<ETB>",
        '\u{18}' => "<CAN>",
        '\u{19}' => "<EM>",
        '\u{1a}' => "<SUB>",
        '\u{1b}' => "<ESC>",
        '\u{1c}' => "<FS>",
        '\u{1d}' => "<GS>",
        '\u{1e}' => "<RS>",
        '\u{1f}' => "<US>",
        '\u{7f}' => "<DEL>",
        '\u{80}' => "<80>",
        '\u{81}' => "<81>",
        '\u{82}' => "<82>",
        '\u{83}' => "<83>",
        '\u{84}' => "<84>",
        '\u{85}' => "<85>",
        '\u{86}' => "<86>",
        '\u{87}' => "<87>",
        '\u{88}' => "<88>",
        '\u{89}' => "<89>",
        '\u{8a}' => "<8A>",
        '\u{8b}' => "<8B>",
        '\u{8c}' => "<8C>",
        '\u{8d}' => "<8D>",
        '\u{8e}' => "<8E>",
        '\u{8f}' => "<8F>",
        '\u{90}' => "<90>",
        '\u{91}' => "<91>",
        '\u{92}' => "<92>",
        '\u{93}' => "<93>",
        '\u{94}' => "<94>",
        '\u{95}' => "<95>",
        '\u{96}' => "<96>",
        '\u{97}' => "<97>",
        '\u{98}' => "<98>",
        '\u{99}' => "<99>",
        '\u{9a}' => "<9A>",
        '\u{9b}' => "<9B>",
        '\u{9c}' => "<9C>",
        '\u{9d}' => "<9D>",
        '\u{9e}' => "<9E>",
        '\u{9f}' => "<9F>",
        _ => "<?>",
    }
}

fn append(output: &mut String, value: &str, maximum: usize) -> Result<(), RenderError> {
    if output.len().saturating_add(value.len()) > maximum {
        return Err(RenderError {
            kind: RenderErrorKind::OutputLimit,
            message: format!("rendered output exceeds {maximum} bytes").into(),
        });
    }
    output.push_str(value);
    Ok(())
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            character if character.is_ascii() => escaped.push(character),
            character => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "&#x{:04X};", u32::from(character));
            }
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use crate::ast::{EquationTerminal, EquationTerminalToken, NodeKind};
    use crate::{Limits, Parser, Source, SourceName};

    use super::{
        DEFAULT_RENDER_OUTPUT_BYTES, RenderErrorKind, RenderFormat, Renderer,
        TERMINAL_HANGING_INDENT_MARKER, TERMINAL_NONBREAKING_SPACE_MARKER, TerminalFont,
        display_width, escape_html, expand_filled_terminal_tabs, expand_literal_terminal_tabs,
        render_html_equation, render_terminal_bold, render_terminal_equation,
        render_terminal_equation_text, render_terminal_visible_text,
        render_terminal_visible_text_with_font, render_visible_text, terminal_character_width,
        terminal_default_volume, terminal_mdoc_plain_text_sentence,
        terminal_table_text_block_lines, wrap_html_plain_paragraph, wrap_terminal_output,
    };

    #[test]
    fn renderer_resolves_visible_character_escapes_without_changing_ast_spelling() {
        let limits = Limits::default();
        assert_eq!(
            render_visible_text(r"x\N'65'x \[u2014]\&", RenderFormat::Utf8, &limits),
            "xAx —"
        );
        assert_eq!(
            render_visible_text(r"x\N'65'x \[u2014]", RenderFormat::Ascii, &limits),
            "xAx --"
        );
        assert_eq!(
            render_visible_text(r"\[u005C]\N'92'\e\(rs", RenderFormat::Utf8, &limits),
            r"\\\\"
        );
        assert_eq!(
            render_visible_text(
                r"\[u00A2]\[u00C1]\[u03B1]\[u02D8]",
                RenderFormat::Ascii,
                &limits,
            ),
            "/\x08c\x27\x08A<alpha>\x27\x08\x60"
        );
        assert_eq!(
            render_terminal_visible_text(r"\(lqmylib\(rq", RenderFormat::Ascii, &limits),
            "\"mylib\""
        );
        assert_eq!(
            render_terminal_visible_text(r"\(lqmylib\(rq", RenderFormat::Utf8, &limits),
            "“mylib”"
        );
        assert_eq!(
            render_visible_text(
                r"x\N'259'x x\N'XX'x x\N'65XX'x x\N''x x\N665x x\NX65Yx",
                RenderFormat::Utf8,
                &limits,
            ),
            "xx xX'x xAX'x xx x65x xAx"
        );
        assert_eq!(
            render_visible_text(r"\[u2191] \[u21D1]", RenderFormat::Ascii, &limits),
            "|\u{8}^ =\u{8}^"
        );
        assert_eq!(
            render_visible_text(r"e\'e\[']e e\`e\[`]e", RenderFormat::Ascii, &limits),
            "e'ee e`ee"
        );
        assert_eq!(
            render_visible_text(r"e\'e\[']e e\`e\[`]e", RenderFormat::Utf8, &limits),
            "e´ee e`ee"
        );
        assert_eq!(
            render_visible_text(r"e\U'0301' e\C'u0301'", RenderFormat::Utf8, &limits),
            "eU'0301' e\u{301}"
        );
        assert_eq!(
            render_terminal_visible_text(r"\fBname\fR plain", RenderFormat::Utf8, &limits),
            "n\u{8}na\u{8}am\u{8}me\u{8}e plain"
        );
        assert_eq!(
            render_terminal_visible_text(
                r"\fIitalic\fRroman\fPitalic",
                RenderFormat::Utf8,
                &limits,
            ),
            "_\u{8}i_\u{8}t_\u{8}a_\u{8}l_\u{8}i_\u{8}croman_\u{8}i_\u{8}t_\u{8}a_\u{8}l_\u{8}i_\u{8}c"
        );
        assert_eq!(
            render_terminal_visible_text_with_font(
                r"bold\fRplain\fPbold",
                RenderFormat::Ascii,
                &limits,
                TerminalFont::Bold,
            ),
            "b\u{8}bo\u{8}ol\u{8}ld\u{8}dplainb\u{8}bo\u{8}ol\u{8}ld\u{8}d"
        );
        assert_eq!(
            render_terminal_visible_text(
                r"\f4x\f3x\f2x\f1x\f(BIx\f(CBx\f(CIx\f[]x",
                RenderFormat::Ascii,
                &limits,
            ),
            "_\u{8}x\u{8}xx\u{8}x_\u{8}xx_\u{8}x\u{8}xx\u{8}x_\u{8}xx"
        );
        assert_eq!(
            render_visible_text(r"\*(.T \*[.T] \\*(.T", RenderFormat::Ascii, &limits),
            r"ascii ascii \*(.T"
        );
        assert_eq!(
            render_visible_text(r"\*(.T \*[.T]", RenderFormat::Utf8, &limits),
            "utf8 utf8"
        );
        assert_eq!(
            render_visible_text(
                r"a\[hy]b\[ hy]c a\~b\[~]c a\0b\[0]c",
                RenderFormat::Ascii,
                &limits
            ),
            "a-bhy]c a bc a bc"
        );
    }

    #[test]
    fn renderer_projects_named_and_numeric_control_scalars_without_layout_bytes() {
        let limits = Limits::default();
        assert_eq!(
            render_visible_text(
                r"\[u0000]\N'1'\[u007F]\N'128'",
                RenderFormat::Ascii,
                &limits,
            ),
            "<NUL><SOH><DEL><80>"
        );
        assert_eq!(
            render_visible_text(r"\[u0000]\N'1'\[u007F]\N'128'", RenderFormat::Utf8, &limits,),
            "����"
        );
        assert_eq!(
            render_visible_text(r"\[uD7FB]", RenderFormat::Ascii, &limits),
            "<?>"
        );
        assert_eq!(
            render_visible_text(r"\[u226A]", RenderFormat::Ascii, &limits),
            "<<"
        );
    }

    #[test]
    fn terminal_roff_presentation_controls_do_not_leak_as_source_text() {
        let limits = Limits::default();
        assert_eq!(
            render_terminal_visible_text(
                r"a\O1b a\O(52b a\O[5dummy]b",
                RenderFormat::Ascii,
                &limits
            ),
            "ab ab ab"
        );
        assert_eq!(
            render_terminal_visible_text(r"x\o'|O'x", RenderFormat::Ascii, &limits),
            "x|\u{8}Ox"
        );
        assert_eq!(
            render_terminal_visible_text(r">\l'3n'<", RenderFormat::Ascii, &limits),
            ">___<"
        );
        assert_eq!(
            render_terminal_visible_text(r">\h'0.16i'<", RenderFormat::Ascii, &limits),
            ">  <"
        );
        assert_eq!(
            render_terminal_visible_text(r">\z\fBxbold\fP<", RenderFormat::Ascii, &limits),
            ">x\u{8}x\u{8}b\u{8}bo\u{8}ol\u{8}ld\u{8}d<"
        );
        assert_eq!(
            render_terminal_visible_text(r"a\+b\!c\?d", RenderFormat::Ascii, &limits),
            "a+bcd"
        );
        assert_eq!(
            render_terminal_visible_text(
                r"a\kxb\k(xyc\k[xyz]d a\R'reg 0'b\R'reg \A'y'0'c a\s0b\s(12c\s[123]d\s'123'e\s'1\w'xy'2'f a\s-0b\s-(12c\s-[123]d\s-'123'e\s-'1\w'xy'2'f\s-",
                RenderFormat::Ascii,
                &limits
            ),
            "abcd abc abcdef abcdef"
        );
    }

    #[test]
    fn terminal_roff_p_breaks_at_its_device_word_boundary() {
        let name = SourceName::new("terminal-roff-p.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt ESC-P 1\n.Os\n.Sh DESCRIPTION\nno blank: line one\\pline two\n.Pp\nblank after esc: line one\\p line two\n.Pp\nblank before esc: line one \\pline two\n.Pp\nat eol: line one\\p\nline two\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "     no blank: line oneline\n     two\n\n     blank after esc: line one\n     line two\n\n     blank before esc: line one line\n     two\n\n     at eol: line one\n     line two"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_literal_opening_punctuation_does_not_suppress_next_word_spacing() {
        let name = SourceName::new("terminal-literal-opening-punctuation.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH PUNCT 1\n.SH DESCRIPTION\n.tr x\n>>x<<\ntwo words\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(">> << two words"),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_equations_keep_ascii_greek_fallback_names() {
        let limits = Limits::default();
        assert_eq!(
            render_terminal_equation_text(r"\[*a] \[*b] \[*g]", RenderFormat::Ascii, &limits),
            "<alpha> <beta> <gamma>"
        );
        assert_eq!(
            render_terminal_equation_text(r"\[*a] \[*b] \[*g]", RenderFormat::Utf8, &limits),
            "α β γ"
        );
    }

    #[test]
    fn terminal_equation_boxes_retain_positions_and_font_beyond_public_text() {
        let equation = EquationTerminal {
            tokens: [
                ("sum", false),
                ("from", false),
                ("{", false),
                ("i", false),
                ("=", false),
                ("1", false),
                ("}", false),
                ("to", false),
                ("inf", false),
                ("1", false),
                ("over", false),
                ("{", false),
                ("i", false),
                ("sup", false),
                ("2", false),
                ("}", false),
            ]
            .into_iter()
            .map(|(text, quoted)| EquationTerminalToken {
                text: text.into(),
                quoted,
            })
            .collect(),
        };
        assert_eq!(
            render_terminal_equation(&equation, RenderFormat::Ascii, &Limits::default()),
            "<sum>_(_\x08i = 1)^<infinity> 1/(_\x08i^2)"
        );
        assert_eq!(
            render_html_equation(&equation, &Limits::default()),
            "<mrow><munderover><mo>&#x2211;</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow><mo>&#x221E;</mo></munderover><mfrac><mn>1</mn><mrow><msup><mi>i</mi><mn>2</mn></msup></mrow></mfrac></mrow>"
        );

        let bold = EquationTerminal {
            tokens: [
                ("bold", false),
                ("{", false),
                ("sin", false),
                ("sin", true),
                ("}", false),
                ("text", true),
                ("bold", false),
                ("x", false),
                ("hat", false),
            ]
            .into_iter()
            .map(|(text, quoted)| EquationTerminalToken {
                text: text.into(),
                quoted,
            })
            .collect(),
        };
        assert_eq!(
            render_terminal_equation(&bold, RenderFormat::Ascii, &Limits::default()),
            "(\x08(sin s\x08si\x08in\x08n)\x08) _\x08t_\x08e_\x08x_\x08t x\x08x^\x08^"
        );
    }

    #[test]
    fn terminal_hangul_jamo_extended_b_uses_the_pinned_device_width() {
        assert_eq!(terminal_character_width('\u{d7fb}'), 2);
        assert_eq!(display_width("\u{d7fb}"), 2);
        assert_eq!(
            expand_filled_terminal_tabs("\u{d7fb}\tvalue"),
            "\u{d7fb}   value"
        );
        assert_eq!(terminal_character_width('\u{fffe}'), 0);
        assert_eq!(terminal_character_width('\u{10ffff}'), 0);
        assert_eq!(terminal_character_width('\u{0fff}'), 0);
        assert_eq!(terminal_character_width('\u{d7ff}'), 0);
        assert_eq!(terminal_character_width('\u{40000}'), 0);
        assert_eq!(terminal_character_width('\u{c0000}'), 0);
    }

    #[test]
    fn terminal_volume_defaults_match_the_pinned_manual_sections() {
        assert_eq!(terminal_default_volume("2"), "System Calls Manual");
        assert_eq!(terminal_default_volume("3p"), "Perl Library Manual");
        assert_eq!(terminal_default_volume("8"), "System Manager's Manual");
    }

    #[test]
    fn terminal_renderer_keeps_section_headings_out_of_body_flow() {
        let name = SourceName::new("sections.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".TH SECTIONS 1\n.SH NAME\nsections \\- body\n.SH DESCRIPTION\nvisible text\n",
            ))
            .unwrap();
        assert_eq!(
            report.output,
            "SECTIONS(1)                 General Commands Manual                SECTIONS(1)\n\nN\u{8}NA\u{8}AM\u{8}ME\u{8}E\n       sections - body\n\nD\u{8}DE\u{8}ES\u{8}SC\u{8}CR\u{8}RI\u{8}IP\u{8}PT\u{8}TI\u{8}IO\u{8}ON\u{8}N\n       visible text\n\nOpenBSD                                                            SECTIONS(1)\n"
        );
    }

    #[test]
    fn terminal_footer_accumulates_a_final_roff_vertical_space() {
        let name = SourceName::new("footer-final-sp.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FOOTER-FINAL-SP 1\n.Os\n.Sh DESCRIPTION\nlast table field\n.sp\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "     last table field\n\n\nOpenBSD                          July 4, 2017                          OpenBSD\n"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_renderer_wraps_by_display_width() {
        let name = SourceName::new("wrap.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .with_width(20)
            .render(Source::new(
                &name,
                b".TH WRAP 1\n.SH DESCRIPTION\nwide \\[u4E2D]\\[u6587] text stays together on terminal lines\n",
            ))
            .unwrap();
        assert_eq!(
            report.output,
            "WRAP(1)\nGeneral Commands Manual\n\nD\u{8}DE\u{8}ES\u{8}SC\u{8}CR\u{8}RI\u{8}IP\u{8}PT\u{8}TI\u{8}IO\u{8}ON\u{8}N\n       wide 中文\n       text stays\n       together on\n       terminal\n       lines\n\nOpenBSD      WRAP(1)\n"
        );
    }

    #[test]
    fn ascii_terminal_headings_use_deterministic_overstrike_emphasis() {
        let name = SourceName::new("ascii-heading.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH ASCII-HEADING 1\n.SH NAME\nascii-heading \\- test\n",
            ))
            .unwrap();
        assert!(report.output.contains("N\u{8}NA\u{8}AM\u{8}ME\u{8}E"));
        assert_eq!(display_width("N\u{8}N"), 1);
        assert_eq!(display_width("+\u{8}+\u{8}o\u{8}o"), 1);
        assert_eq!(
            render_terminal_bold("name", RenderFormat::Utf8),
            "n\u{8}na\u{8}am\u{8}me\u{8}e"
        );
    }

    #[test]
    fn mdoc_sections_begin_body_at_the_native_five_column_indent() {
        let name = SourceName::new("mdoc-indent.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt MDOC-INDENT 1\n.Os\n.Sh DESCRIPTION\nvisible text\n",
            ))
            .unwrap();
        assert!(report
            .output
            .contains("D\u{8}DE\u{8}ES\u{8}SC\u{8}CR\u{8}RI\u{8}IP\u{8}PT\u{8}TI\u{8}IO\u{8}ON\u{8}N\n     visible text"));
    }

    #[test]
    fn mdoc_section_headings_preserve_inline_semantic_fonts() {
        let name = SourceName::new("mdoc-section-inline-font.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SECTION 1\n.Os OpenBSD\n.Sh SEE Em ALSO\n.Tg reference\n.Rs\n.%A author\n.%J journal\n.%N 42\n.Re\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "S\u{8}SE\u{8}EE\u{8}E _\u{8}A_\u{8}L_\u{8}S_\u{8}O\n     author, _\u{8}j_\u{8}o_\u{8}u_\u{8}r_\u{8}n_\u{8}a_\u{8}l, 42."
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn empty_mdoc_section_headings_keep_a_blank_device_field() {
        let name = SourceName::new("mdoc-empty-section-heading.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SECTION 1\n.Os OpenBSD\n.Sh DESCRIPTION\nbefore\n.Sh \\ \\&\nafter\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("     before\n\n\n     after"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_column_lists_keep_declared_terminal_fields() {
        let name = SourceName::new("mdoc-column-fields.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd August 27, 2026\n.Dt COLUMN-FIELDS 1\n.Os\n.Sh DESCRIPTION\n.Bl -column wide column\n.It a Ta b\n.El\n.Bl -column a b c d e\n.It a Ta b Ta c Ta d Ta e\n.El\n",
            ))
            .unwrap();
        // The list labels are intentionally absent from the public AST, but
        // select device fields of `width + 4` (or `width + 3` for five
        // columns).  This remains no-fill terminal geometry, not prose.
        assert!(
            report.output.contains("     a       b\n"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("     a   b   c   d   e\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_column_lists_render_tbl_items_structurally() {
        let name = SourceName::new("mdoc-column-tbl.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd August 27, 2026\n.Dt COLUMN-TBL 1\n.Os\n.Sh DESCRIPTION\n.Bl -column a b\n.Sy a Ta b\n.TS\nlll.\n1\t2\t3\n4\t5\t6\n.TE\n.Em c Ta d\n.El\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     1   2   3\n     4   5   6\n\n     _\u{8}c    d\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_column_list_cells_keep_recovered_displays_structural() {
        let name = SourceName::new("mdoc-column-display.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt COLUMN-DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Bl -column column\n.It column\n.Bd -ragged -offset indent\ninside display\n.El\nafter list\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     column\n\n           inside display after list"),
            "{}",
            report.output
        );
    }

    #[test]
    fn tbl_text_blocks_wrap_at_the_selected_device_field() {
        assert_eq!(
            terminal_table_text_block_lines("This is a very long sentence.", 20),
            ["This is a very long", "sentence."]
        );
        assert_eq!(
            terminal_table_text_block_lines("This is a very long sentence.", 10),
            ["This is a", "very long", "sentence."]
        );
    }

    #[test]
    fn tbl_rows_share_calculated_terminal_columns() {
        let name = SourceName::new("tbl-terminal-columns.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-COLUMNS 1\n.SH DESCRIPTION\nnormal text\n.TS\ntab(:);\nr c l.\n*:*:*\n**:**:**\n.TE\n",
            ))
            .unwrap();
        // The first field is right-aligned, the middle one centered, and
        // the final field left-aligned in the shared five-cell columns.
        assert!(
            report
                .output
                .contains("\n\n        *   *    *\n       **   **   **\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn tbl_ranges_keep_private_boundaries_and_centering() {
        let name = SourceName::new("tbl-terminal-ranges.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd August 27, 2026\n.Dt TBL-RANGES 1\n.Os\n.Sh DESCRIPTION\n.TS\ncenter box; l.\none\n.TE\n.TS\ncenter box; l.\ntwo\n.TE\n",
            ))
            .unwrap();
        // Adjacent source tables are intentionally flat generated Table
        // siblings in the compatible AST. Their private range markers still
        // have to preserve two independent boxed and centered device fields.
        let first = report.output.find("|one |").unwrap();
        let second = report.output.find("|two |").unwrap();
        assert!(
            report.output[first..second].contains("+----+\n\n"),
            "{}",
            report.output
        );
        for line in report.output.lines().filter(|line| line.contains("+----+")) {
            assert_eq!(
                line.bytes().take_while(|byte| *byte == b' ').count(),
                39,
                "{}",
                report.output
            );
        }
    }

    #[test]
    fn tbl_center_uses_one_calculated_offset_for_rules_and_data() {
        let name = SourceName::new("tbl-terminal-centering-grid.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-CENTRE 1\n.SH DESCRIPTION\n.TS\ncenter tab(:); |l||l|.\n_\ntxt:text\n.TE\n",
            ))
            .unwrap();
        // The visual rule has one more intersection glyph than tblcalc's
        // centering width.  Both the rule and content still start at the
        // one precomputed grid offset rather than being centred separately.
        for line in report
            .output
            .lines()
            .filter(|line| line.contains("+----++-----+") || line.contains("|txt ||text |"))
        {
            assert_eq!(
                line.bytes().take_while(|byte| *byte == b' ').count(),
                36,
                "{}",
                report.output
            );
        }
    }

    #[test]
    fn tbl_interior_empty_data_rows_are_terminal_blank_lines() {
        let name = SourceName::new("tbl-empty-data-row.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-EMPTY-ROW 1\n.SH DESCRIPTION\n.TS\nlb\nli\nlb.\nfirst\n\nlast\n.TE\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("\n       f\u{8}fi\u{8}ir\u{8}rs\u{8}st\u{8}t\n\n       l\u{8}la\u{8}as\u{8}st\u{8}t\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_c_continuation_attaches_filled_and_literal_source_lines() {
        let name = SourceName::new("roff-c-continuation.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt ROFF-C 1\n.Os\n.Sh DESCRIPTION\none\\c\nword\n.Bd -literal\none\\c\nword\n.Ed\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("     oneword\n\n     oneword\n"),
            "{}",
            report.output
        );
        let man_name = SourceName::new("roff-c-man-font.1").unwrap();
        let man_report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &man_name,
                b".TH ROFF-C 1\n.SH DESCRIPTION\n.B\none\\c\nword\n",
            ))
            .unwrap();
        assert!(
            man_report
                .output
                .contains("o\u{8}on\u{8}ne\u{8}ew\u{8}wo\u{8}or\u{8}rd\u{8}d"),
            "{}",
            man_report.output
        );
    }

    #[test]
    fn tbl_closing_line_consumes_the_first_following_positive_sp_slot() {
        let name = SourceName::new("tbl-sp-after-table.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-SP 1\n.SH DESCRIPTION\n.TS\nbox;\nl.\nvalue\n.TE\n.sp\nfollowing text\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("+------+\n       following text\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn tbl_layout_horizontal_cells_override_input_and_preserve_following_sp() {
        let name = SourceName::new("tbl-layout-horizontal-input.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-HORIZONTAL 1\n.SH DESCRIPTION\n.TS\ntab(:);\n_ _\nl l\n- -\nl r\n_ ^\nr.\ncolum one:column two\nleft:right\nnot:printed\nright:left\n.TE\n.sp\nfollowing text\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "-----------------------\n       colum one   column two\n       -----------------------\n       left             right\n       -----------\n           right   left\n\n       following text"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn tbl_next_row_vertical_rules_extend_into_the_current_device_row() {
        let name = SourceName::new("tbl-next-row-rules.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-SPACING 1\n.SH DESCRIPTION\n.TS\nbox tab(:);\nl0 l1 |  l2 |  l3 |  l4 |  l5 |  l6 |  l7 |  l8\nl0 l1    l2    l3    l4    l5    l6    l7    l8\nl0 l1 |  l2 || l3 || l4    l5 || l6 |  l7 || l8.\na:b:c:d:e:f:g:h:i\na:b:c:d:e:f:g:h:i\na:b:c:d:e:f:g:h:i\n.TE\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "|ab|c |d ||e    f  || g   |  h   ||  i |\n       |ab|c |d ||e    f  || g   |  h   ||  i |\n       +--+--+--++--------++-----+------++----+"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_bk_keeps_its_body_phrase_without_printing_recovered_head_words() {
        let name = SourceName::new("mdoc-bk-body-keep.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BK-BODY 1\n.Os\n.Sh SYNOPSIS\n.Nm body-keep\n.Ar x x x x x x x x\n.Ar x x x x x x x x\n.Ar x x x x x x x x\n.Ar x x x x x x\n.Bk -invalid ignored\n.Op o Ar a\n.Ek\n.Pp\n.Nm next\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("               [o _\u{8}a]\n\n     n\u{8}ne\u{8}ex\u{8}xt\u{8}t"),
            "{}",
            report.output
        );
        assert!(!report.output.contains("ignored"), "{}", report.output);
    }

    #[test]
    fn mdoc_bk_releases_a_nested_optional_after_its_input_line_break() {
        let name = SourceName::new("mdoc-bk-input-lines.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BK-INPUTLINES 1\n.Os\n.Sh NAME\n.Nm Bk-inputlines\n.Nd input-line word keeps\n.Sh SYNOPSIS\n.Nm\n.Ar x x x x x x x x x x x x x x x x x x x x x x x x x x x\n.Bk -words\n.Oo Oo No a Oc\n.Oo No b Oc Oc Pq input-line boundary\n.Ek\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("[[a]\n                   [b]]"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_synopsis_options_keep_a_complete_later_form_in_its_field() {
        let name = SourceName::new("mdoc-synopsis-options.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SYNOPSIS-OPTIONS 1\n.Os\n.Sh SYNOPSIS\n.Nm ksh\n.Op Fl +abCefhiklmnpruvXx\n.Op Fl +o Ar option\n.Op Fl c Ar string \\*(Ba Fl s \\*(Ba Ar file Op Ar argument ...\n",
            ))
            .unwrap();
        // The final optional form moves as one field to the conventional
        // nine-column continuation rather than leaving `[` on the prior
        // line or breaking the `-s` option at its hyphen.
        assert!(
            report.output.contains("\n         [-\u{8}-c"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains(" | -\u{8}-s\u{8}s | "),
            "{}",
            report.output
        );
        assert!(
            !report.output.contains("\n     [-\u{8}-c"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_bk_keeps_function_argument_boundaries_after_commas() {
        let word = "x".repeat(20);
        let source = format!(
            ".Dd July 4, 2017\n.Dt BK-FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.Fn {word} \"{word} {word}\" {word}\n.Pp\n.Fo {word}\n.Fa \"{word} {word}\" {word}\n.Fc\n.Ek\n"
        );
        let name = SourceName::new("mdoc-bk-functions.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(&name, source.as_bytes()))
            .unwrap();
        let bold = "x\u{8}x".repeat(20);
        let italic = "_\u{8}x".repeat(20);
        let one_request_signature = format!("{bold}({italic}\n     {italic}, {italic})");
        let block_signature = format!("{bold}({italic} {italic}, {italic})");
        assert!(
            report.output.contains(&one_request_signature),
            "{}",
            report.output
        );
        assert!(
            report.output.contains(&block_signature),
            "{}",
            report.output
        );
    }

    #[test]
    fn tbl_device_layout_keeps_box_rules_and_decimal_columns_private_to_rendering() {
        let name = SourceName::new("tbl-device-layout.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-DEVICE 1\n.SH DESCRIPTION\n.TS\nbox tab(:);\nr || n | n .\n1:1.00:+42.0\n_\n10:-10.0:3.14\n.TE\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "+---++-------+--------+\n       | 1 ||  1.00 | +42.0  |\n       +---++-------+--------+\n       |10 ||-10.0  |   3.14 |\n       +---++-------+--------+"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn tbl_layout_vertical_edges_frame_contents_and_horizontal_rules() {
        let name = SourceName::new("tbl-layout-vertical-edges.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-EDGES 1\n.SH DESCRIPTION\n.TS\n|l|l|.\n_\nA\ttest\n_\n.TE\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("+--+------+\n       |A | test |\n       +--+------+"),
            "{}",
            report.output
        );
    }

    #[test]
    fn tbl_leading_layout_metadata_applies_only_to_the_outer_field() {
        let name = SourceName::new("tbl-leading-metadata.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TBL 1\n.SH DESCRIPTION\n.TS\ntab(:);\n  l l\n  l l\n| l l\n  l l.\n11:12\n21:22\n31:32\n41:42\n.TE\n",
            ))
            .unwrap();
        let mut stack = vec![report.document.node(report.document.root()).unwrap()];
        let mut found = false;
        while let Some(node) = stack.pop() {
            if node.kind() == NodeKind::Table {
                let Some(terminal) = node.table_terminal() else {
                    continue;
                };
                if terminal
                    .cells
                    .first()
                    .is_some_and(|cell| cell.before_vertical_rules == 1)
                {
                    assert_eq!(terminal.cells[1].before_vertical_rules, 0);
                    found = true;
                }
            }
            stack.extend(node.children());
        }
        assert!(found);
    }

    #[test]
    fn tbl_badspan_terminal_columns_follow_the_occupied_span() {
        let name = SourceName::new("tbl-badspan-metadata.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TBL 1\n.SH DESCRIPTION\n.TS\nallbox tab(:);\nS L S S\nL L L L L L.\nspan:end\n1:2:3:4:5:6\n.TE\n",
            ))
            .unwrap();
        let mut stack = vec![report.document.node(report.document.root()).unwrap()];
        let mut found = false;
        while let Some(node) = stack.pop() {
            if node.kind() == NodeKind::Table && node.table_cells().len() == 2 {
                assert_eq!(node.table_cells()[0].column_span, 3);
                assert_eq!(node.table_terminal().unwrap().data_columns, [1, 4]);
                found = true;
            }
            stack.extend(node.children());
        }
        assert!(found);
    }

    #[test]
    fn tbl_full_rules_keep_the_preceding_layout_grid() {
        let name = SourceName::new("tbl-complex-metadata.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL 1\n.SH DESCRIPTION\n.TS\ntab(:);\n||l||l||\n|l|l|\nll.\n_\na:b\n_\nc:d\n_\ne:f\n_\n.TE\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "       +--++--+\n       |a ||b |\n       +--++--+\n       |c | d |\n       +--+---+\n        e   f\n       --------\n"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn tbl_standalone_leading_vertical_layout_line_joins_the_next_row() {
        let name = SourceName::new("tbl-standalone-vertical.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-STANDALONE 1\n.SH DESCRIPTION\n.TS\nl\n|\nr.\ntable text\n_\nbar\nright\n.TE\n.PP\nfollowing text\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "        table text\n       +-----------\n       |       bar\n       |     right\n\n       following text"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn tbl_allbox_rules_resume_after_a_spanned_row() {
        let name = SourceName::new("tbl-spanned-allbox.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-SPAN 1\n.SH DESCRIPTION\n.TS\nallbox tab(:);\nL L L\nC S C.\na:b:c\nwide:c\n.TE\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "+--+---+---+\n       |a | b | c |\n       +--+---+---+\n       |wide  | c |\n       +------+---+"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn tbl_empty_layout_retains_an_authored_leading_vertical_rule() {
        let name = SourceName::new("tbl-empty-leading-rule.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-EMPTY 1\n.SH DESCRIPTION\n.TS\n|.\ntable text\n.TE\n",
            ))
            .unwrap();
        // The compatible AST recovers the empty format as one normal left
        // column.  tbl nevertheless prints the authored leading `|` rule.
        assert!(
            report.output.contains("\n       |table text\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_compact_displays_keep_a_line_boundary_without_a_blank_slot() {
        let name = SourceName::new("mdoc-compact-display.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd August 27, 2026\n.Dt COMPACT-DISPLAY 1\n.Os\n.Sh DESCRIPTION\npreceding text\n.Bd -ragged -offset indent\nordinary display\n.Ed\ntext between displays\n.Bd -ragged -offset indent -compact\ncompact display\n.Ed\nfollowing text\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "           ordinary display\n     text between displays\n           compact display\n     following text"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_literal_display_keeps_all_words_from_one_source_line_together() {
        let name = SourceName::new("mdoc-literal-phrase.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd August 27, 2026\n.Dt LITERAL-PHRASE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\nfirst second\nthird\n.Ed\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("     first second\n     third"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_name_blocks_keep_bold_name_and_description_separator() {
        let name = SourceName::new("mdoc-name.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt MDOC-NAME 1\n.Os\n.Sh NAME\n.Nm mdoc-name\n.Nd example description\n",
            ))
            .unwrap();
        assert!(report
            .output
            .contains("     m\u{8}md\u{8}do\u{8}oc\u{8}c-\u{8}-n\u{8}na\u{8}am\u{8}me\u{8}e - example description"));
    }

    #[test]
    fn mdoc_description_blocks_resume_at_an_owned_paragraph() {
        let name = SourceName::new("mdoc-nd-paragraph.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt ND-PAR 1\n.Os\n.Sh NAME\n.Nm nd-par\n.Nd paragraph macro\nafter one-line description\n.Pp\nUsually, there should not be additional text in the NAME section.\n.Sh DESCRIPTION\nThe text belongs here.\n.Nd stray\ndescription macro\n.Pp\nBack to normal state.\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "n\u{8}nd\u{8}d-\u{8}-p\u{8}pa\u{8}ar\u{8}r - paragraph macro after one-line description\n\n     Usually"
            ),
            "{}",
            report.output
        );
        assert!(
            report.output.contains(
                "The text belongs here.  - stray description macro\n\n     Back to normal state."
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_paragraph_and_spacing_elements_create_one_blank_line() {
        let name = SourceName::new("terminal-spacing.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".TH TERMINAL-SPACING 1\n.SH DESCRIPTION\nfirst paragraph\n.sp\nsecond paragraph\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("       first paragraph\n\n       second paragraph")
        );
    }

    #[test]
    fn terminal_vertical_requests_accumulate_across_transparent_anchors() {
        let man_name = SourceName::new("terminal-adjacent-sp.1").unwrap();
        let man = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &man_name,
                b".TH TERMINAL-ADJACENT-SP 1\n.SH DESCRIPTION\nbefore\n.sp\n.sp\nafter\n",
            ))
            .unwrap();
        assert!(
            man.output.contains("       before\n\n\n       after"),
            "{}",
            man.output
        );

        let mdoc_name = SourceName::new("terminal-transparent-spacing.1").unwrap();
        let mdoc = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &mdoc_name,
                b".Dd July 4, 2017\n.Dt TERMINAL-TRANSPARENT-SPACING 1\n.Os\n.Sh DESCRIPTION\nbefore\n.sp\n.Tg anchor\n.Pp\nafter\n",
            ))
            .unwrap();
        assert!(
            mdoc.output.contains("     before\n\n\n     after"),
            "{}",
            mdoc.output
        );
    }

    #[test]
    fn terminal_negative_spacing_suppresses_the_next_paragraph_gap() {
        let name = SourceName::new("terminal-negative-spacing.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".TH TERMINAL-NEGATIVE-SPACING 1\n.SH DESCRIPTION\nfirst line\n.sp -1v\n.PP\nsecond line\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("       first line\n       second line")
        );
    }

    #[test]
    fn terminal_roff_font_requests_persist_across_sibling_text() {
        let name = SourceName::new("terminal-font-requests.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TERMINAL-FONT-REQUESTS 1\n.SH DESCRIPTION\nplain\n.ft I\nitalic\n.ft B\nbold\n.ft P\nitalic-again\n.ft\nbold-again\n.ft R\nroman\n",
            ))
            .unwrap();
        let expected = format!(
            "       plain {} {} {} {} roman",
            super::render_terminal_font("italic", super::TerminalFont::Italic),
            super::render_terminal_font("bold", super::TerminalFont::Bold),
            super::render_terminal_font("italic-again", super::TerminalFont::Italic),
            super::render_terminal_font("bold-again", super::TerminalFont::Bold),
        );
        assert!(report.output.contains(&expected), "{}", report.output);
    }

    #[test]
    fn terminal_page_offsets_are_relative_and_restore_after_invalid_requests() {
        let name = SourceName::new("terminal-page-offsets.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt PAGE-OFFSETS 1\n.Os\n.Sh DESCRIPTION\ninitial\n.Pp\n.po -2n\nleft\n.Pp\n.po +5n\nright\n.Pp\n.po invalid\nleft again\n.Pp\n.po 0\nfinal\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "     initial\n\n   left\n\n        right\n\n   left again\n\n     final"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_spacing_uses_mandoc_scaled_vertical_units() {
        for (source, expected) in [
            ("20u", 0),
            ("21u", 1),
            ("1c", 2),
            ("0.25i", 1),
            ("0.5P", 0),
            ("1P", 1),
            ("6p", 0),
            ("7p", 1),
            ("1n", 1),
            ("3n", 2),
            ("2m", 1),
        ] {
            assert_eq!(
                super::terminal_vertical_span(source),
                Some(expected),
                "{source}"
            );
        }
        assert_eq!(super::terminal_vertical_span("1cx"), Some(2));
        assert_eq!(super::terminal_vertical_span("xxx"), None);
    }

    #[test]
    fn terminal_temporary_indentation_tracks_relative_and_wide_fields() {
        assert_eq!(super::terminal_temporary_indent_target("10n", 7), Some(10));
        assert_eq!(super::terminal_temporary_indent_target("+10n", 7), Some(17));
        assert_eq!(super::terminal_temporary_indent_target("-10n", 7), Some(0));
        assert_eq!(super::terminal_temporary_indent_target("80n", 7), Some(72));
        assert_eq!(super::terminal_temporary_indent_target("+4n", 73), Some(73));
    }

    #[test]
    fn terminal_empty_mdoc_sections_do_not_add_vertical_gaps() {
        let name = SourceName::new("terminal-empty-sections.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt EMPTY 1\n.Os\n.Sh SYNOPSIS\n.Sh DESCRIPTION Xo\n.Sh BUGS\nvisible\n",
            ))
            .unwrap();
        let synopsis = report.output.find("S\u{8}S").unwrap();
        let description = report.output[synopsis..]
            .find("D\u{8}D")
            .map(|offset| synopsis + offset)
            .unwrap();
        assert_eq!(
            report.output[synopsis..description].matches('\n').count(),
            1
        );
    }

    #[test]
    fn terminal_empty_mdoc_name_description_retains_its_dash() {
        let name = SourceName::new("terminal-empty-nd.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt EMPTY-ND 1\n.Os\n.Sh NAME\n.Nm empty-nd\n.Nd\n",
            ))
            .unwrap();
        assert!(report.output.contains("d\u{8}d -\n"), "{}", report.output);
    }

    #[test]
    fn terminal_mdoc_variable_types_use_synopsis_lines_but_prose_spacing() {
        let name = SourceName::new("terminal-vt-layout.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt VT 1\n.Os\n.Sh SYNOPSIS\n.Vt extern int first\n.Vt extern int second\n.Sh DESCRIPTION\n.Vt signed int.\nfollowing prose\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("_\u{8}e_\u{8}x_\u{8}t"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("_\u{8}t_\u{8}. following prose"),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_mdoc_function_macros_render_semantic_prototypes() {
        let name = SourceName::new("terminal-function-prototype.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FUNCTION 1\n.Os\n.Sh SYNOPSIS\n.Ft int\n.Fn abs \"int value\"\n.Sh DESCRIPTION\n.Ft int\n.Fo abs\n.Fa \"int value\"\n.Fc\n",
            ))
            .unwrap();
        let layout = report.output.replace('\u{8}', "");
        assert!(
            layout.contains("_i_n_t\n     aabbss(_i_n_t _v_a_l_u_e);"),
            "{}",
            report.output
        );
        assert!(
            layout.contains("_i_n_t aabbss(_i_n_t _v_a_l_u_e)"),
            "{}",
            report.output
        );
    }

    #[test]
    fn synopsis_function_arguments_wrap_as_whole_argument_phrases() {
        let name = SourceName::new("terminal-function-wrap.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .with_width(30)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FUNCTION 1\n.Os\n.Sh SYNOPSIS\n.Fn function \"verylong argument\" \"other argument\"\n",
            ))
            .unwrap();
        let layout = report.output.replace('\u{8}', "");
        assert!(
            layout.contains(
                "ffuunnccttiioonn(_v_e_r_y_l_o_n_g _a_r_g_u_m_e_n_t,\n         _o_t_h_e_r _a_r_g_u_m_e_n_t);"
            ),
            "{report:?}"
        );
    }

    #[test]
    fn description_fo_arguments_wrap_as_whole_argument_phrases() {
        let name = SourceName::new("terminal-fo-wrap.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .with_width(35)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FO-WRAP 1\n.Os\n.Sh DESCRIPTION\n.Fo function\n.Fa \"verylong argument\"\n.Fa \"other argument\"\n.Fc\n",
            ))
            .unwrap();
        let layout = report.output.replace('\u{8}', "");
        assert!(
            layout.contains("ffuunnccttiioonn(_v_e_r_y_l_o_n_g _a_r_g_u_m_e_n_t,\n     _o_t_h_e_r _a_r_g_u_m_e_n_t)"),
            "{report:?}"
        );
    }

    #[test]
    fn long_synopsis_names_keep_default_argument_phrases_in_the_name_field() {
        let name = SourceName::new("terminal-long-name.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LONG-NAME 1\n.Os\n.Sh SYNOPSIS\n.Nm \"This is a terribly long name, it is so long that it does not fit \\\none one single line -\"\n.Fl o\n.Ar\n",
        ))
            .unwrap();
        let layout = report.output.replace('\u{8}', "");
        let argument_line = layout
            .lines()
            .find(|line| line.trim_start().starts_with("_f_i_l_e"))
            .expect("default Ar argument line");
        assert!(argument_line.starts_with(&" ".repeat(70)), "{layout}");
        assert_eq!(argument_line.trim(), "_f_i_l_e _._._.");
    }

    #[test]
    fn recovered_synopsis_names_keep_function_and_enclosure_fields() {
        let name = SourceName::new("terminal-recovered-synopsis-name.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FUNCTION 1\n.Os\n.Sh SYNOPSIS\n.Ft int\n.Fo function\n.Nm name Fc tail\n.Oo oo\n.Nm nm\n.Bk -words\noc\n.Oc\n.Ek\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "f\u{8}fu\u{8}un\u{8}nc\u{8}ct\u{8}ti\u{8}io\u{8}on\u{8}n(n\u{8}na\u{8}am\u{8}me\u{8}e);\n      tail [oo\n     n\u{8}nm\u{8}m oc]"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_mdoc_include_declarations_complete_device_lines() {
        let name = SourceName::new("terminal-fd-layout.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FD 1\n.Os\n.Sh SYNOPSIS\n.Fd #include <first.h>\n.Fd #include <second.h>\n.Ft int\n.Fn first void\n.Sh DESCRIPTION\n.Fd #include <first.h>\n.Ft int\n.Fn first void\n.Fd #include <second.h>\n",
            ))
            .unwrap();
        let layout = report.output.replace('\u{8}', "");
        assert!(
            layout.contains("##iinncclluuddee <<ffiirrsstt..hh>>\n     ##iinncclluuddee <<sseeccoonndd..hh>>\n\n     _i_n_t"),
            "{}",
            report.output
        );
        assert!(
            layout.contains("##iinncclluuddee <<ffiirrsstt..hh>>\n     _i_n_t ffiirrsstt(_v_o_i_d) ##iinncclluuddee <<sseeccoonndd..hh>>\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_mdoc_include_files_switch_between_synopsis_and_prose_forms() {
        let name = SourceName::new("terminal-in-layout.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt IN 1\n.Os\n.Sh SYNOPSIS\n.In first.h\n.In second.h\n.Ft int\n.Fn first void\n.Sh DESCRIPTION\n.In first.h\n",
            ))
            .unwrap();
        let layout = report.output.replace('\u{8}', "");
        assert!(
            layout.contains("##iinncclluuddee <<ffiirrsstt..hh>>\n     ##iinncclluuddee <<sseeccoonndd..hh>>\n\n     _i_n_t"),
            "{}",
            report.output
        );
        assert!(layout.contains("<_f_i_r_s_t_._h>\n"), "{}", report.output);
    }

    #[test]
    fn terminal_hanging_indentation_uses_its_target_after_the_first_wrap() {
        let input = format!(
            "{TERMINAL_HANGING_INDENT_MARKER}0{TERMINAL_HANGING_INDENT_MARKER}       alpha beta gamma delta"
        );
        assert_eq!(
            wrap_terminal_output(&input, 20, DEFAULT_RENDER_OUTPUT_BYTES, 0, 0).unwrap(),
            "       alpha beta\ngamma delta"
        );
    }

    #[test]
    fn terminal_explicit_enclosures_preserve_empty_and_opening_boundaries() {
        let name = SourceName::new("terminal-eo.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\nbefore\n.Eo\n.Ec\nafter opening\n.Eo <<\n.Ec\nnext\n.No prefix Ns Eo\n.Ec\nclosing\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     before  after opening << next prefix closing"),
            "{}",
            report.output
        );
    }

    #[test]
    fn collected_explicit_enclosures_attach_only_their_matching_tail() {
        let name = SourceName::new("terminal-eo-collected.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\n.Eo <<\neo\n.Bo\nbo\nec\n.Ec >>\nbc\n.Bc\nno closing\n.Eo <<\n.Bo\n.Ec >>\nbc\n.Bc\nopening only\n.Bo\nbo\n.Eo\n.Bc\n.Ec >>\nclosing only\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "     <<eo [bo ec>> bc] no closing <<[>> bc] opening only [bo ]>> closing only"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_nested_enclosures_only_defer_their_own_recovered_closer() {
        let name = SourceName::new("terminal-nested-closers.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt CLOSERS 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Ao ao\n.Bo bo\n.Nd nd\n.Pq pq bc Bc ac\n.Ac Op op\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("<ao [bo - nd (pq bc] ac)> [op]"),
            "{}",
            report.output
        );
    }

    #[test]
    fn explicit_enclosure_attaches_a_line_start_no_body() {
        let name = SourceName::new("terminal-eo-no.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\n.Eo <<\n.No prefix Ns Ec\nstray closing\n.Ec >>\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("     <<prefix stray closing"),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_explicit_enclosures_attach_custom_special_character_delimiters() {
        let name = SourceName::new("terminal-eo-special.1").unwrap();
        let source = b".Dd July 4, 2017\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\n.ds o \\(Fo\n.ds c \\(Fc\n.Eo \\*o\nvalue\n.Ec \\*c\n";
        let ascii = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(&name, source))
            .unwrap();
        let utf8 = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(&name, source))
            .unwrap();
        assert!(ascii.output.contains("     <<value>>"));
        assert!(utf8.output.contains("     «value»"));
    }

    #[test]
    fn terminal_font_blocks_skip_their_retained_validation_head() {
        let name = SourceName::new("terminal-bf-head.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BF 1\n.Os\n.Sh DESCRIPTION\n.Bf Sy ignored\nbody\n.Ef\n",
            ))
            .unwrap();
        assert!(report.output.contains("     b\u{8}bo\u{8}od\u{8}dy\u{8}y"));
        assert!(!report.output.contains("ignored"));
    }

    #[test]
    fn terminal_font_blocks_reset_missing_and_unknown_font_arguments() {
        let name = SourceName::new("terminal-bf-missing-font.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BF 1\n.Os\n.Sh DESCRIPTION\n.Bf -emphasis\nemphasis\n.Bf\nno argument\n.Ef\nback to emphasis\n.Bf badarg\nbad argument\n.Ef\n.Ef\n",
            ))
            .unwrap();
        assert!(report
            .output
            .contains("_\u{8}e_\u{8}m_\u{8}p_\u{8}h_\u{8}a_\u{8}s_\u{8}i_\u{8}s no argument _\u{8}b_\u{8}a_\u{8}c_\u{8}k"));
        assert!(report.output.contains("_\u{8}s bad argument\n"));
    }

    #[test]
    fn terminal_font_block_closure_inside_an_enclosure_resets_later_text() {
        let name = SourceName::new("terminal-bf-enclosure.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BF 1\n.Os\n.Sh DESCRIPTION\n.Bf Em\n.Bo\ninside\n.Ef\nafter\n.Bc\n.Ef\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("_\u{8}[_\u{8}i_\u{8}n_\u{8}s_\u{8}i_\u{8}d_\u{8}e after]")
        );
    }

    #[test]
    fn no_fill_lines_bypass_terminal_width_wrapping() {
        let name = SourceName::new("terminal-no-fill.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .with_width(20)
            .render(Source::new(
                &name,
                b".TH TERMINAL-NO-FILL 1\n.SH DESCRIPTION\n.nf\none two three four five six   \n.fi\n",
            ))
            .unwrap();
        assert!(report.output.contains("       one two three four five six"));
        assert!(!report.output.contains("six   \n"));

        let example = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".TH TERMINAL-NO-FILL 1\n.SH DESCRIPTION\nregular\n.EX ignored\nliteral\n.EE ignored\nagain\n",
            ))
            .unwrap();
        assert!(
            example
                .output
                .contains("       regular\n       literal\n       again"),
            "{}",
            example.output
        );
        assert!(!example.output.contains("ignored"), "{}", example.output);
    }

    #[test]
    fn filled_terminal_tabs_use_the_native_five_column_stops() {
        let name = SourceName::new("terminal-tabs.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".TH TERMINAL-TABS 1\n.SH DESCRIPTION\nsingle\ttab\n.br\ndouble\t\ttab\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("       single    tab"),
            "{}",
            report.output
        );
        assert!(report.output.contains("       double         tab"));
    }

    #[test]
    fn terminal_tab_stops_are_relative_to_the_text_or_display_field() {
        assert_eq!(expand_filled_terminal_tabs("     1\tx"), "     1    x");
        assert_eq!(expand_filled_terminal_tabs("     \ttab"), "          tab");
        assert_eq!(
            expand_literal_terminal_tabs("       1\tx"),
            "       1       x"
        );
    }

    #[test]
    fn terminal_roff_ta_requests_clear_and_repeat_device_tab_stops() {
        let name = SourceName::new("terminal-roff-ta.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt TA 1\n.Os\n.Sh DESCRIPTION\n.Bd -unfilled\n.ta 3n +6n T 4n +2n\n1\t2\t3\t4\t5\t6\t7\n.ta\n1\t2\t3\n.Ed\n.Bd -literal\n1\t2\t3\n.Ed\n1\t2\t3\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     1  2     3   4 5   6 7\n     123\n\n     1       2       3\n     1       2       3"),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_same_line_conditional_body_keeps_its_tab_in_the_current_field() {
        let name = SourceName::new("terminal-inline-condition.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH CONDITION 1\n.SH DESCRIPTION\n.nr name 0\nlabel:\n.ie rname\tvalue\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("       label:    value"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_displays_keep_their_offset_and_distinct_tab_stops() {
        let name = SourceName::new("terminal-display-tabs.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DISPLAY-TABS 1\n.Os\n.Sh DESCRIPTION\n.Bd -unfilled -offset 3n\nsingle\ttab\ndouble\t\ttab\n.Ed\n.Bd -literal -offset 3n\nsingle\ttab\ndouble\t\ttab\n.Ed\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("\n\n        single    tab\n        double         tab\n\n"),
            "{}",
            report.output
        );
        assert!(
            report
                .output
                .contains("\n\n        single  tab\n        double          tab\n\n")
        );
    }

    #[test]
    fn first_literal_mdoc_display_starts_in_the_section_field() {
        let name = SourceName::new("terminal-first-literal-display.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\nfirst\n.Ed\n",
            ))
            .unwrap();
        let lines = report.output.lines().collect::<Vec<_>>();
        let first = lines
            .iter()
            .position(|line| *line == "     first")
            .expect("literal display line");
        assert!(
            first > 0 && !lines[first - 1].is_empty(),
            "{}",
            report.output
        );
    }

    #[test]
    fn first_unoffset_unfilled_mdoc_display_starts_in_the_section_field() {
        let name = SourceName::new("first-unfilled-display.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Bd -unfilled\nfirst\n.Ed\n",
            ))
            .unwrap();
        let lines = report.output.lines().collect::<Vec<_>>();
        let first = lines
            .iter()
            .position(|line| *line == "     first")
            .expect("unfilled display line");
        assert!(
            first > 0 && !lines[first - 1].is_empty(),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_display_closes_on_one_physical_line_without_an_extra_paragraph_gap() {
        let name = SourceName::new("terminal-display-close.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DISPLAY-CLOSE 1\n.Os\n.Sh DESCRIPTION\n.Bd -ragged\ndisplay text\n.Ed\nfollowing text\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     display text\n     following text"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_centered_displays_center_each_wrapped_terminal_line() {
        let name = SourceName::new("terminal-centered-display.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd January 4, 2019\n.Dt CENTERED 1\n.Os\n.Sh DESCRIPTION\n.Bd -centered -offset indent\nThe text in this centered block is wide enough to not fit on one line.\n.Ed\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "            The text in this centered block is wide enough to not fit on one\n                                          line."
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_authors_render_one_an_macro_per_terminal_line() {
        let name = SourceName::new("terminal-authors.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt AUTHORS 1\n.Os\n.Sh AUTHORS\n.An First Author\n.An Second Author\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     First Author\n     Second Author"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_an_layout_directives_are_terminal_state_not_visible_text() {
        let name = SourceName::new("terminal-an-layout.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt AUTHORS 1\n.Os\n.Sh DESCRIPTION\nsplit follows:\n.An -split ignored\n.An First Author\n.An Second Author\n.Sh AUTHORS\ninline: \n.An First Author\n.An -nosplit ignored\n.An Second Author\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     split follows:\n     First Author\n     Second Author"),
            "{}",
            report.output
        );
        assert!(
            report
                .output
                .contains("     inline: First Author Second Author"),
            "{}",
            report.output
        );
        assert!(!report.output.contains("ignored"), "{}", report.output);
    }

    #[test]
    fn mdoc_op_body_keeps_parsed_punctuation_adjacent() {
        let name = SourceName::new("terminal-op-punctuation.1").unwrap();
        let source = b".Dd July 4, 2017\n.Dt OP 1\n.Os\n.Sh DESCRIPTION\n.Op a \"(\" z\n.Op a . z\n.Op ( (\n.Op . .\n";
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(&name, source))
            .unwrap();
        assert!(report.output.contains("[a (z] [a. z]"), "{}", report.output);
        assert!(report.output.contains("(([] [].."), "{}", report.output);
    }

    #[test]
    fn filled_mdoc_source_lines_use_terminal_sentence_spacing() {
        let name = SourceName::new("terminal-sentence-spacing.1").unwrap();
        let source = b".Dd July 4, 2017\n.Dt SENTENCE-SPACING 1\n.Os\n.Sh DESCRIPTION\nFirst sentence.\nSecond sentence.\n";
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(&name, source))
            .unwrap();
        assert!(
            report
                .output
                .contains("     First sentence.  Second sentence."),
            "{}",
            report.output
        );
    }

    #[test]
    fn filled_man_source_lines_use_terminal_sentence_spacing() {
        let name = SourceName::new("terminal-man-sentence-spacing.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".TH SENTENCE-SPACING 1\n.SH DESCRIPTION\nFirst sentence.\nSecond sentence.\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("       First sentence.  Second sentence.")
        );
    }

    #[test]
    fn terminal_wraps_at_a_fitting_hyphen_before_moving_a_whole_word() {
        let name = SourceName::new("terminal-hyphen.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .with_width(32)
            .render(Source::new(
                &name,
                b".TH HYPHEN 1\n.SH DESCRIPTION\nA line whose final break-here word crosses the margin.\n",
            ))
            .unwrap();
        assert!(report.output.contains("final break-\n       here"));
    }

    #[test]
    fn mdoc_semantic_font_macros_override_inline_font_controls() {
        let name = SourceName::new("terminal-mdoc-fonts.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt MDOC-FONTS 1\n.Os\n.Sh DESCRIPTION\n.Fl option \\fR|\\fP tail\n.br\n.Fl \\-long\n.br\n.Sy symbol\n.br\n.Ar argument\n.br\n.Fa parameter\n.br\n.Em emphasis\n.br\n.Ft return\\fBbold\\fPtail\n.br\n.Cd constant\n.br\n.Fd function\n.br\n.Vt plain Sy child Li literal\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("-\u{8}-o\u{8}op\u{8}pt\u{8}ti\u{8}io\u{8}on\u{8}n | -\u{8}-t\u{8}ta\u{8}ai\u{8}il\u{8}l"),
            "{}",
            report.output
        );
        assert!(
            report
                .output
                .contains("-\u{8}--\u{8}-l\u{8}lo\u{8}on\u{8}ng\u{8}g"),
            "{}",
            report.output
        );
        assert!(
            report
                .output
                .contains("s\u{8}sy\u{8}ym\u{8}mb\u{8}bo\u{8}ol\u{8}l")
        );
        assert!(
            report
                .output
                .contains("_\u{8}a_\u{8}r_\u{8}g_\u{8}u_\u{8}m_\u{8}e_\u{8}n_\u{8}t")
        );
        assert!(
            report
                .output
                .contains("_\u{8}p_\u{8}a_\u{8}r_\u{8}a_\u{8}m_\u{8}e_\u{8}t_\u{8}e_\u{8}r")
        );
        assert!(
            report
                .output
                .contains("_\u{8}e_\u{8}m_\u{8}p_\u{8}h_\u{8}a_\u{8}s_\u{8}i_\u{8}s")
        );
        assert!(
            report.output.contains(
                "_\u{8}r_\u{8}e_\u{8}t_\u{8}u_\u{8}r_\u{8}nb\u{8}bo\u{8}ol\u{8}ld\u{8}d_\u{8}t_\u{8}a_\u{8}i_\u{8}l"
            ),
            "{}",
            report.output
        );
        assert!(
            report
                .output
                .contains("c\u{8}co\u{8}on\u{8}ns\u{8}st\u{8}ta\u{8}an\u{8}nt\u{8}t")
        );
        assert!(
            report
                .output
                .contains("f\u{8}fu\u{8}un\u{8}nc\u{8}ct\u{8}ti\u{8}io\u{8}on\u{8}n")
        );
        assert!(
            report.output.contains(
                "_\u{8}p_\u{8}l_\u{8}a_\u{8}i_\u{8}n c\u{8}ch\u{8}hi\u{8}il\u{8}ld\u{8}d literal"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn empty_mdoc_flag_attaches_to_a_same_line_macro() {
        let name = SourceName::new("terminal-empty-flag.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FLAG 1\n.Os\n.Sh DESCRIPTION\n.Op Fl Ux\n",
            ))
            .unwrap();
        assert!(report.output.contains("[-\u{8}-UNIX]"), "{}", report.output);
    }

    #[test]
    fn mdoc_navigation_and_escape_nodes_do_not_emit_terminal_text() {
        let name = SourceName::new("terminal-transparent.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt TRANSPARENT 1\n.Os\n.Sh DESCRIPTION\n.Tg destination\n.Es < >\nvisible text\n",
            ))
            .unwrap();
        assert!(report.output.contains("visible text"), "{}", report.output);
        assert!(!report.output.contains("destination"), "{}", report.output);
        assert!(!report.output.contains("<>"), "{}", report.output);
    }

    #[test]
    fn mdoc_name_description_uses_the_section_field_without_a_preceding_name() {
        let name = SourceName::new("terminal-mdoc-nd-first.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt ND 1\n.Os\n.Sh NAME\n.Nd description without a preceding name\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("\n     - description without a preceding name"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_one_line_displays_complete_their_terminal_lines() {
        let name = SourceName::new("terminal-mdoc-one-line-display.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\nbefore\n.D1 filled display\nafter\n.Dl literal display\nend\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "     before\n           filled display\n     after\n           literal display\n     end"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_cross_references_join_name_and_section() {
        let name = SourceName::new("terminal-mdoc-xr.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt XR 1\n.Os\n.Sh DESCRIPTION\n.Xr echo 1 Ns s\n.br\n.Xr ( echo 1\n.br\n.Xr echo,\n",
            ))
            .unwrap();
        assert!(report.output.contains("echo(1)s"), "{}", report.output);
        assert!(report.output.contains("(echo(1)"), "{}", report.output);
        assert!(report.output.contains("echo,"), "{}", report.output);
    }

    #[test]
    fn mdoc_ns_only_attaches_when_not_at_a_physical_line_start() {
        let name = SourceName::new("terminal-mdoc-ns.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt NS 1\n.Os\n.Sh DESCRIPTION\n.Op before\n.Ns Op after\n.br\n.Oo before\n.Oc Ns Op after\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("[before] [after]\n     [before][after]"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_line_start_macros_do_not_inherit_open_delimiter_attachment() {
        let name = SourceName::new("terminal-mdoc-delimiter-boundary.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BOUNDARY 1\n.Os\n.Sh DESCRIPTION\n.Li a (\n.Li b\n.br\nopening (\n.No word\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("a ( b\n     opening ( word"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_links_render_labels_before_bold_targets() {
        let name = SourceName::new("terminal-mdoc-lk.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LK 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.test/ Example site ,\n.br\n.Lk https://only.example/,\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "_\u{8}E_\u{8}x_\u{8}a_\u{8}m_\u{8}p_\u{8}l_\u{8}e _\u{8}s_\u{8}i_\u{8}t_\u{8}e: h\u{8}ht\u{8}tt\u{8}tp\u{8}ps\u{8}s:\u{8}:/\u{8}//\u{8}/e\u{8}ex\u{8}xa\u{8}am\u{8}mp\u{8}pl\u{8}le\u{8}e.\u{8}.t\u{8}te\u{8}es\u{8}st\u{8}t/"
            ),
            "{}",
            report.output
        );
        assert!(
            report.output.contains(
                "h\u{8}ht\u{8}tt\u{8}tp\u{8}ps\u{8}s:\u{8}:/\u{8}//\u{8}/o\u{8}on\u{8}nl\u{8}ly\u{8}y.\u{8}.e\u{8}ex\u{8}xa\u{8}am\u{8}mp\u{8}pl\u{8}le\u{8}e/\u{8}/,\u{8},"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_debug_requests_are_terminal_invisible() {
        let name = SourceName::new("terminal-mdoc-db.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DB 1\n.Os\n.Sh DESCRIPTION\nbefore\n.Db hidden arguments\nafter\n",
            ))
            .unwrap();
        assert!(report.output.contains("before after"), "{}", report.output);
        assert!(!report.output.contains("hidden"), "{}", report.output);
    }

    #[test]
    fn mdoc_library_macros_complete_line_only_in_library_sections() {
        let name = SourceName::new("terminal-mdoc-lb.3").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LB 3\n.Os\n.Sh LIBRARY\n.Lb mylib\ntext\n.Sh DESCRIPTION\n.Lb mylib\ntext\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("library \"mylib\"\n     text"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("library \"mylib\" text"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_plain_lists_render_item_bodies_with_compact_boundaries() {
        let name = SourceName::new("terminal-mdoc-plain-list.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os\n.Sh DESCRIPTION\n.Bl -item\n.It\nfirst line\n.It ignore\nsecond line\n.It\nthird line\n.El\n.Bl -item -compact\n.It\nfirst compact\n.It ignored\nsecond compact\n.El\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "first line\n\n     second line\n\n     third line\n     first compact\n     second compact"
            ),
            "{}",
            report.output
        );
        assert!(!report.output.contains("ignore"), "{}", report.output);
    }

    #[test]
    fn mdoc_plain_list_completes_its_final_terminal_field() {
        let name = SourceName::new("terminal-plain-list-tail.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -item -offset indent\n.It\nitem body\n.El\nouter text\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("           item body\n     outer text"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_definition_lists_use_tag_width_for_inline_and_continuation_bodies() {
        let name = SourceName::new("terminal-definition-list.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Fl a | b\nlong tag body\n.It Fl c\nshort body\n.El\n.Fl d\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "     -\u{8}-a\u{8}a | -\u{8}-b\u{8}b\n             long tag body\n\n     -\u{8}-c\u{8}c      short body\n     -\u{8}-d\u{8}d"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_definition_lists_handle_overflow_fields_and_quoted_tag_padding() {
        let name = SourceName::new("terminal-definition-overflow.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width 100n\n.It hundred\ntext text\n.El\n.Bl -tag -width 5n\n.It \"a  \"\ntwo\n.El\n",
            ))
            .unwrap();
        let layout = &report.output;
        assert!(
            layout.lines().any(|line| {
                line.trim_start().starts_with("hundred") && line.trim_end().ends_with("text")
            }),
            "{layout}"
        );
        assert!(layout.contains("\n     a      two\n"), "{layout}");
    }

    #[test]
    fn doublebox_uses_two_terminal_rules_and_consumes_two_following_sp_slots() {
        let name = SourceName::new("terminal-doublebox.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ndoublebox;\nL .\none\n.TE\n.sp 2v\nfollowing\n",
            ))
            .unwrap();
        let layout = report.output.replace('\u{8}', "");
        assert_eq!(layout.matches("+----+").count(), 4, "{layout}");
        let final_rule = layout.rfind("+----+").expect("doublebox final rule");
        assert!(
            layout[final_rule..].starts_with("+----+\n       following"),
            "{layout}"
        );
    }

    #[test]
    fn allbox_adds_its_rule_before_an_authored_manual_table_rule() {
        let name = SourceName::new("terminal-allbox-manual.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ntab(:) allbox;\n||l||l||.\na:b\n_\nc:d\n_\n.TE\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("|a ||b |\n       +--++--+\n       +--++--+\n       |c ||d |"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_definition_list_preserves_a_leading_vertical_body_tail() {
        let name = SourceName::new("terminal-definition-list-vertical-tail.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -tag -width 6n\n.It tag\n.sp 2v\nEl sp 2v\n.El\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     tag\n\n\n             El sp 2v"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_definition_list_resumes_structural_display_bodies() {
        let name = SourceName::new("terminal-definition-list-display-tail.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -tag -width 6n\n.It tag\nouter text\n.Bd -ragged -offset 2n\ninner text\n.Ed\nouter text\n.El\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "     tag     outer text\n\n               inner text\n             outer text"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_definition_list_heads_retain_extended_quote_delimiters() {
        let name = SourceName::new("terminal-definition-list-quote.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It prefix Ao\n.No quoted tag\n.Ac\nbody\n.El\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("prefix <quoted tag>"),
            "{}",
            report.output
        );
    }

    #[test]
    fn obsolete_mdoc_es_en_blocks_retain_the_resolved_enclosure() {
        let name = SourceName::new("terminal-obsolete-enclosure.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt ENCLOSURE 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Es << >>\n.En enclosed words\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("<<enclosed words>>"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_marker_lists_retain_private_selector_spelling_and_offset() {
        let name = SourceName::new("terminal-marker-list.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os\n.Sh DESCRIPTION\nbefore\n.Bl -bullet -offset indent\n.It\nfirst\n.It\n.El\n.Bl -dash\n.It\ndash body\n.El\n.Bl -enum\n.It\nfirst enum\n.It\nsecond enum\n.El\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "     before\n\n           +\u{8}+\u{8}o\u{8}o   first\n\n           +\u{8}+\u{8}o\u{8}o\n\n     -\u{8}-   dash body\n\n     1.   first enum\n\n     2.   second enum"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_nested_lists_at_item_starts_keep_the_outer_device_field() {
        let name = SourceName::new("terminal-nested-list-item-start.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -dash\n.It\n.Bl -dash\n.It\ntext\n.El\n.El\n.Bl -inset\n.It outer\n.Bl -inset\n.It inner\ntext\n.El\n.El\n.Bl -tag -width 4n\n.It outer tag\n.Bl -tag -width 4n\n.It inner tag\ntext\n.El\n.El\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "     -\u{8}-\n\n         -\u{8}-   text\n\n     outer\n\n     inner text\n\n     outer tag\n\n           inner tag\n                 text"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_marker_list_width_outdents_wrapped_body_lines() {
        let name = SourceName::new("terminal-marker-width.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .with_width(20)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -bullet -width -4n\n.It\nx x x x x x x x x x\n.El\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     +\u{8}+\u{8}o\u{8}o x x x x x x x\n   x x x"),
            "{}",
            report.output
        );
    }

    #[test]
    fn empty_mdoc_lists_complete_the_current_line_without_vertical_spacing() {
        let name = SourceName::new("terminal-empty-list.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\nbefore\n.Bl -bullet\n.El\nafter\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("     before\n     after"),
            "{}",
            report.output
        );
        assert!(
            !report.output.contains("     before\n\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_tag_list_widths_control_inline_and_outdented_body_fields() {
        let name = SourceName::new("terminal-tag-width.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .with_width(20)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -tag -width -4n\n.It tag\nx x x x x x x x x x\n.El\n.Bl -tag -width 3n\n.It tag\nx x x\n.El\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     tag\n   x x x x x x x x x\n   x"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("     tag  x x x"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_hanging_lists_keep_the_first_body_phrase_on_the_tag_line() {
        let name = SourceName::new("terminal-hanging-list.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .with_width(20)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -hang -width -4n\n.It tag\nx x x x x x x x x x\n.El\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("     tag x x x x x x\n   x x x x"),
            "{}",
            report.output
        );
    }

    #[test]
    fn compact_mdoc_hanging_lists_keep_adjacent_items_on_neighboring_lines() {
        let name = SourceName::new("terminal-compact-hanging-list.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -hang -width 6n -compact\n.It one\nfirst\n.It second\nsecond\n.El\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     one     first\n     second  second"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_overhanging_lists_keep_term_and_body_on_equally_indented_lines() {
        let name = SourceName::new("terminal-overhanging-list.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -ohang\n.It term\nbody\n.El\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("     term\n     body\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_inset_and_diagnostic_lists_keep_their_private_terminal_fields() {
        let name = SourceName::new("terminal-definition-list-variants.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -inset\n.It \"term  \"\nbody\n.El\n.Bl -diag\n.It label\nbody\n.El\n",
            ))
            .unwrap();
        assert!(report.output.contains("term   body"), "{}", report.output);
        assert!(
            report
                .output
                .contains("l\u{8}la\u{8}ab\u{8}be\u{8}el\u{8}l  body"),
            "{}",
            report.output
        );
    }

    #[test]
    fn empty_mdoc_definition_heads_use_each_list_forms_body_margin() {
        let name = SourceName::new("terminal-empty-definition-head.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -tag\n.It\ntag body\n.El\n.Bl -ohang\n.It\nohang body\n.El\n.Bl -inset\n.It\ninset body\n.El\n.Bl -diag\n.It\ndiag body\n.El\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("             tag body"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("     ohang body"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("     inset body"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("       diag body"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_tag_list_a2width_handles_roff_scales_and_visible_fallbacks() {
        let name = SourceName::new("terminal-tag-a2width.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .with_width(20)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -tag -width 4m\n.It tag\nx x x x x x\n.El\n.Bl -tag -width xxx\n.It tag\nx x x\n.El\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("     tag   x x x x x\n           x"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("     tag  x x x"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_synopsis_nm_declarations_are_bold_and_line_separated() {
        let name = SourceName::new("terminal-synopsis-nm.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt NM 1\n.Os OpenBSD\n.Sh SYNOPSIS\n.Nm first\n.Nm second\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "     f\x08fi\x08ir\x08rs\x08st\x08t\n     s\x08se\x08ec\x08co\x08on\x08nd\x08d"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_synopsis_nm_keeps_nested_optional_delimiters() {
        let name = SourceName::new("terminal-synopsis-nm-optional.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt NM 1\n.Os\n.Sh SYNOPSIS\n.Nm before Bo within\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "b\u{8}be\u{8}ef\u{8}fo\u{8}or\u{8}re\u{8}e [\u{8}[w\u{8}wi\u{8}it\u{8}th\u{8}hi\u{8}in\u{8}n]\u{8}]"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_ip_inherits_a_preceding_explicit_tag_field_width() {
        let name = SourceName::new("terminal-ip-field.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH IP 1 \"July 4, 2017\"\n.SH DESCRIPTION\n.IP first 10n\nfirst body\n.IP second\nsecond body\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("       first     first body\n\n       second    second body"),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_tp_uses_its_tag_field_without_rendering_the_width_argument() {
        let name = SourceName::new("terminal-tp.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TP 1 \"July 4, 2017\"\n.SH DESCRIPTION\nbefore\n.TP 10n\n.I \"plain\"\nfilled text\n.nf\n.TP 10n\ntag\nliteral\ntext\n.fi\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "       before\n\n       _\u{8}p_\u{8}l_\u{8}a_\u{8}i_\u{8}n     filled text\n\n       tag       literal\n                 text"
            ),
            "{}",
            report.output
        );
        assert!(!report.output.contains("10n"), "{}", report.output);
    }

    #[test]
    fn man_tp_skips_extra_header_arguments_and_honours_head_indentation() {
        let name = SourceName::new("terminal-tp-head.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.TP 10n ignored\ntag\nbody\n.TP 8n\n.in 3n\nshifted\nbody\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("       tag       body"),
            "{}",
            report.output
        );
        assert!(!report.output.contains("ignored"), "{}", report.output);
        assert!(
            report
                .output
                .contains("          shifted\n               body"),
            "{}",
            report.output
        );

        let invalid_width = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.TP not-a-width\ntag\nbody\n",
            ))
            .unwrap();
        assert!(
            invalid_width.output.contains("       tag    body"),
            "{}",
            invalid_width.output
        );
        assert!(
            !invalid_width.output.contains("not-a-width"),
            "{}",
            invalid_width.output
        );
    }

    #[test]
    fn man_tp_head_font_requests_override_an_open_parent_font() {
        let name = SourceName::new("terminal-tp-nested-font.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.TP\n.B\n.I\nitalic term\nbody\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("       _\u{8}i_\u{8}t_\u{8}a_\u{8}l_\u{8}i_\u{8}c _\u{8}t_\u{8}e_\u{8}r_\u{8}m\n              body"),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_tp_field_width_is_shared_until_a_paragraph_reset() {
        let name = SourceName::new("terminal-tp-shared-field.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.TP 6n\nshort\nbody\n.TP\n20n\nbody\n.PP\nreset\n.TP\n20n\nbody\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "       short body\n\n       20n   body\n\n       reset\n\n       20n    body"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_empty_tp_before_rs_does_not_leave_field_padding_at_line_end() {
        let name = SourceName::new("terminal-empty-tp-rs.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.TP 4n\n*\nitem\n.RS 8n\nindented text\n.RE\nmiddle text\n.TP 4n\n*\n.RS 8n\nindented text\n.RE\ntrailing text\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("       *\n               indented text"),
            "{}",
            report.output
        );
        assert!(!report.output.contains("*  \n"), "{}", report.output);
    }

    #[test]
    fn man_empty_hp_before_sibling_rs_keeps_both_vertical_boundaries() {
        let name = SourceName::new("terminal-empty-hp-rs.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH HP 1\n.SH DESCRIPTION\n.RS\nouter text\n.HP 2n\n.RS 4n\ninner text\n.RE\n.RE\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("              outer text\n\n\n                  inner text"),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_tp_uses_pd_density_and_wraps_long_terms() {
        let name = SourceName::new("terminal-tp-spacing.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .with_width(40)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.PD 2v\n.TP\nfirst-tag\ntext\n.TP\nsecond-tag\ntext\n.TP 6n\nThis tagged paragraph has ridiculously long text in its head\nbody\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("              text\n\n\n       second-tag"),
            "{}",
            report.output
        );
        assert!(
            report
                .output
                .contains("       This tagged paragraph has\n       ridiculously long text"),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_tp_trailing_nonbreaking_blanks_reserve_but_do_not_print_field_cells() {
        let name = SourceName::new("terminal-tp-trailing-space.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.TP\ntag\\ \\&\nfirst body\n.TP\ntag\\ \\ \\ \\ \\ \\&\nsecond body\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("       tag    first body\n\n       tag\n              second body"),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_tp_body_wraps_at_its_field_and_keeps_wide_fields_unfilled() {
        let name = SourceName::new("terminal-tp-wrapping.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .with_width(40)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.TP 12n\ntag\nfirst second third fourth fifth sixth\n.TP 100n\nwide\nbody\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "       tag         first second third\n                   fourth fifth sixth"
            ),
            "{}",
            report.output
        );
        assert!(report.output.contains("       wide"), "{}", report.output);
        assert!(
            !report.output.contains("wide\n       body"),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_font_macro_arguments_do_not_add_sentence_spacing() {
        let name = SourceName::new("terminal-man-font-arguments.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH FONT 1\n.SH DESCRIPTION\nEarlier sentence.\nIt works with\n.B several words\nand with\n.B\nnext line\nscope.\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("Earlier sentence.  It works with"),
            "{}",
            report.output
        );
        assert!(
            report
                .output
                .contains("with s\u{8}se\u{8}ev\u{8}ve\u{8}er\u{8}ra\u{8}al\u{8}l w\u{8}wo\u{8}or\u{8}rd\u{8}ds\u{8}s and with"),
            "{}",
            report.output
        );
        assert!(
            report
                .output
                .contains("and with n\u{8}ne\u{8}ex\u{8}xt\u{8}t l\u{8}li\u{8}in\u{8}ne\u{8}e"),
            "{}",
            report.output
        );
        let alternating = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH FONT 1\n.SH DESCRIPTION\n.BI bold italic bold again\n.IR italic roman\n",
            ))
            .unwrap();
        assert!(
            alternating.output.contains(
                "b\u{8}bo\u{8}ol\u{8}ld\u{8}d_\u{8}i_\u{8}t_\u{8}a_\u{8}l_\u{8}i_\u{8}cb\u{8}bo\u{8}ol\u{8}ld\u{8}d_\u{8}a_\u{8}g_\u{8}a_\u{8}i_\u{8}n"
            ),
            "{}",
            alternating.output
        );
        assert!(
            alternating
                .output
                .contains("_\u{8}i_\u{8}t_\u{8}a_\u{8}l_\u{8}i_\u{8}croman"),
            "{}",
            alternating.output
        );
        let option = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH FONT 1\n.SH DESCRIPTION\nempty\n.OP\nvalue\n.OP -f arg excess\n",
            ))
            .unwrap();
        assert!(option.output.contains("empty []"), "{}", option.output);
        assert!(
            option
                .output
                .contains("value [-\u{8}-f\u{8}f _\u{8}a_\u{8}r_\u{8}g]"),
            "{}",
            option.output
        );
    }

    #[test]
    fn mdoc_no_hyphens_are_not_terminal_break_points() {
        let name = SourceName::new("terminal-mdoc-no-hyphen.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .with_width(32)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt NO-HYPHEN 1\n.Os\n.Sh DESCRIPTION\nA line whose final macro argument is\n.No no-break\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("argument is no-break"),
            "{}",
            report.output
        );
        assert!(
            !report.output.contains("no-\n     break"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_apostrophe_macro_attaches_to_both_neighboring_words() {
        let name = SourceName::new("terminal-apostrophe.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt APOSTROPHE 1\n.Os\n.Sh DESCRIPTION\n.An Ingo Ap s .\n.An Kristaps Ap .\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("Ingo's.  Kristaps'."),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_spacing_controls_are_terminal_invisible() {
        let name = SourceName::new("terminal-sm-control.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SM-CONTROL 1\n.Os\n.Sh DESCRIPTION\n.Sm off\n.No visible\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("     visible\n"),
            "{}",
            report.output
        );
        assert!(!report.output.contains("off"), "{}", report.output);
    }

    #[test]
    fn mdoc_spacing_controls_reach_nested_and_recovered_terminal_phrases() {
        let name = SourceName::new("terminal-sm-phrases.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SM-PHRASES 1\n.Os\n.Sh DESCRIPTION\n.Sm off\n.No toggle Pq now off\n.Sm bad two\n.No restored words\n.Sm on\n.No final words\n.Sm bad\n.No joined words\n.Pp\n.No prefix\n.Sm off\n.Op outer Op inner\n.Sm on\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("toggle(nowoff)"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("bad two restored words"),
            "{}",
            report.output
        );
        assert!(report.output.contains("final words"), "{}", report.output);
        assert!(
            report.output.contains("badjoined words"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("prefix [outer[inner]]"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_sm_off_preserves_sentence_spacing_at_a_new_source_phrase() {
        let name = SourceName::new("terminal-sm-sentence.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SM-SENTENCE 1\n.Os\n.Sh DESCRIPTION\nfirst sentence.\n.Sm off\n.Em following words\n.Sm on\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("first sentence.  _\u{8}f"),
            "{}",
            report.output
        );
    }

    #[test]
    fn plain_mdoc_text_keeps_its_sentence_boundary_inside_a_list() {
        let name = SourceName::new("terminal-mdoc-list-sentence.1").unwrap();
        let source = b".Dd July 4, 2017\n.Dt LIST-SENTENCE 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It tag\nFirst sentence.\nFollowing text.\n.El\n";
        let parsed = Parser::default().parse(Source::new(&name, source)).unwrap();
        let first_sentence = parsed
            .document
            .preorder()
            .find(|node| node.text() == Some("First sentence."))
            .unwrap();
        assert!(terminal_mdoc_plain_text_sentence(first_sentence));
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(&name, source))
            .unwrap();
        assert!(
            report.output.contains("First sentence.  Following text."),
            "{}",
            report.output
        );
    }

    #[test]
    fn list_sentence_spacing_survives_a_following_explicit_enclosure() {
        let name = SourceName::new("terminal-mdoc-list-enclosure-sentence.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST-SENTENCE 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It tag\nFirst sentence.\n.Ao\nquoted text\n.Ac\n.El\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("First sentence.  <quoted text>"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_inline_macro_periods_do_not_create_terminal_sentence_spacing() {
        let name = SourceName::new("terminal-mdoc-inline-period.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt MDOC-PERIOD 1\n.Os\n.Sh DESCRIPTION\n.Ad example.\nfollowing prose\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "_\u{8}e_\u{8}x_\u{8}a_\u{8}m_\u{8}p_\u{8}l_\u{8}e_\u{8}. following prose"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_exit_and_return_expansions_start_below_their_labels() {
        let name = SourceName::new("terminal-mdoc-ex-rv.1").unwrap();
        for (source, expected) in [
            (
                b".Dd July 4, 2017\n.Dt EX 1\n.Os\n.Sh EXIT STATUS\nlabel:\n.Ex -std\n".as_slice(),
                "     label:\n     The utility exits 0 on success",
            ),
            (
                b".Dd July 4, 2017\n.Dt RV 3\n.Os\n.Sh RETURN VALUES\nlabel:\n.Rv -std\n"
                    .as_slice(),
                "     label:\n     Upon successful completion, the value 0",
            ),
        ] {
            let report = Renderer::new(RenderFormat::Ascii)
                .render(Source::new(&name, source))
                .unwrap();
            assert!(report.output.contains(expected), "{}", report.output);
        }
    }

    #[test]
    fn mdoc_nm_keeps_inline_font_changes_inside_its_bold_base() {
        let name = SourceName::new("terminal-nm-font.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt NM 1\n.Os\n.Sh DESCRIPTION\nnormal text\n.Nm bold\\fIemphasis\\fPback\ntrailing text\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "normal text b\u{8}bo\u{8}ol\u{8}ld\u{8}d_\u{8}e_\u{8}m_\u{8}p_\u{8}h_\u{8}a_\u{8}s_\u{8}i_\u{8}sb\u{8}ba\u{8}ac\u{8}ck\u{8}k trailing text"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn quoted_mdoc_arguments_keep_their_significant_trailing_blanks() {
        let name = SourceName::new("terminal-quoted-trailing.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt QUOTED 1\n.Os\n.Sh DESCRIPTION\n.Fl \"one \" \"two \"\ntext\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("-\u{8}-o\u{8}on\u{8}ne\u{8}e  -\u{8}-t\u{8}tw\u{8}wo\u{8}o  text"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_cd_sentence_ending_punctuation_keeps_normal_spacing() {
        let name = SourceName::new("terminal-cd-punctuation.1").unwrap();
        let input = b".Dd July 4, 2017\n.Dt CD 1\n.Os\n.Sh DESCRIPTION\n.Cd literal .\n.Cd next\n";
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(&name, input))
            .unwrap();
        assert!(
            report.output.contains(
                "l\u{8}li\u{8}it\u{8}te\u{8}er\u{8}ra\u{8}al\u{8}l.  n\u{8}ne\u{8}ex\u{8}xt\u{8}t"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn argumentless_man_th_retains_its_terminal_footer() {
        let name = SourceName::new("terminal-th-noarg.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(&name, b".TH\n.SH DESCRIPTION\ntext\n"))
            .unwrap();
        assert!(
            report.output.ends_with("\n\nOpenBSD                                                                     ()\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn empty_man_section_recovery_keeps_orphaned_blocks_in_the_body_column() {
        let name = SourceName::new("terminal-sh-noarg.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH SH-NOARG 1\n.SH DESCRIPTION\nfirst\n.SH\n.nf\nsecond\n.SH\n.fi\nthird\n.SH\n.TP 6n\ntag\ntagged text\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "       first\n\n       second\n\n       third\n\n       tag   tagged text"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn leading_man_section_spacing_does_not_create_a_body_blank_line() {
        let name = SourceName::new("terminal-leading-sp.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH LEADING-SP 1\n.SH DESCRIPTION\n.sp\n.PP\ntext\n",
            ))
            .unwrap();
        let text_start = report.output.find("\n       text\n").unwrap();
        assert_ne!(
            report.output.as_bytes()[text_start - 1],
            b'\n',
            "{}",
            report.output
        );
    }

    #[test]
    fn synopsis_pretty_mdoc_paragraphs_continue_below_the_name_field() {
        let name = SourceName::new("terminal-nm-par.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt NM-PAR 1\n.Os\n.Sh SYNOPSIS\n.Nm\n.Fl a\n.Pp\n.Fl b\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("-\u{8}-a\u{8}a\n\n            -\u{8}-b\u{8}b"),
            "{}",
            report.output
        );
    }

    #[test]
    fn synopsis_paragraphs_inside_optional_name_blocks_keep_their_field() {
        let name = SourceName::new("terminal-nm-parns.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt NM-PARNS 1\n.Os\n.Sh DESCRIPTION\n.nr nS 1\n.Nm\n.Oo Fl a\n.nr nS 0\n.Pp\n.Fl b Oc\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("-\u{8}-a\u{8}a\n\n            -\u{8}-b\u{8}b]"),
            "{}",
            report.output
        );
    }

    #[test]
    fn recovered_bf_closer_ends_the_enclosure_and_font_scope_in_place() {
        let name = SourceName::new("terminal-bf-broken.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BF-BROKEN 1\n.Os\n.Sh DESCRIPTION\nbefore both\n.Bo before font block\n.Bf Em\ninside both\n.Bc\nafter bracket\n.Ef\nafter both\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "before both [before font block _\u{8}i_\u{8}n_\u{8}s_\u{8}i_\u{8}d_\u{8}e _\u{8}b_\u{8}o_\u{8}t_\u{8}h] after bracket after both"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn recovered_display_closer_resumes_an_open_quote_at_its_outer_margin() {
        let name = SourceName::new("terminal-bd-break.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BD-BREAK 1\n.Os\n.Sh DESCRIPTION\nbefore both\n.Bd -ragged -offset indent\nbefore bracket\n.Bo inside both\n.Ed\nafter display\n.Bc\nafter both\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("           before bracket [inside both\n     after display] after both"),
            "{}",
            report.output
        );
    }

    #[test]
    fn display_opened_inside_a_quote_retains_its_vertical_offset_when_closed() {
        let name = SourceName::new("terminal-bd-broken.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BD-BROKEN 1\n.Os\n.Sh DESCRIPTION\nbefore both\n.Bo before display\n.Bd -ragged -offset indent\ninside both\n.Bc\nafter bracket\n.Ed\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("before both [before display\n\n           inside both] after bracket"),
            "{}",
            report.output
        );
    }

    #[test]
    fn incomplete_man_titles_retain_blank_date_terminal_footers() {
        for (source, left, right) in [
            (
                b".TH ONEARG\n.SH DESCRIPTION\ntext\n".as_slice(),
                "OpenBSD",
                "ONEARG()",
            ),
            (
                b".TH EMPTYDATE 1 \"\" source\n.SH DESCRIPTION\ntext\n".as_slice(),
                "source",
                "EMPTYDATE(1)",
            ),
        ] {
            let name = SourceName::new("terminal-th-incomplete.1").unwrap();
            let report = Renderer::new(RenderFormat::Ascii)
                .render(Source::new(&name, source))
                .unwrap();
            let footer = report.output.trim_end().rsplit('\n').next().unwrap();
            assert!(footer.starts_with(left), "{}", report.output);
            assert!(footer.ends_with(right), "{}", report.output);
        }
    }

    #[test]
    fn overwide_man_header_and_footer_fields_keep_terminal_columns() {
        let cases = [
            (
                b".TH TH-LONGTIT-23456789012345678901234567890123456789012345678901234567890123456789 1 \"November 20, 2014\" source\n.SH DESCRIPTION\nSome text.\n".as_slice(),
                "TH-LONGTIT-23456789012345678901234567890123456789012345678901234567890123456789(1)\n                                                       General Commands Manual",
                "source                         November 20, 2014\nTH-LONGTIT-23456789012345678901234567890123456789012345678901234567890123456789(1)",
            ),
            (
                b".TH TH-LONGDATE 1 \"1234567890123456789012345678901234567890123456789012345678901234567890123456789012\" source\n.SH DESCRIPTION\nSome text.\n".as_slice(),
                "TH-LONGDATE(1)              General Commands Manual             TH-LONGDATE(1)",
                "source\n1234567890123456789012345678901234567890123456789012345678901234567890123456789012\n                                                                TH-LONGDATE(1)",
            ),
        ];
        for (source, expected_header, expected_footer) in cases {
            let name = SourceName::new("terminal-th-overwide.1").unwrap();
            let report = Renderer::new(RenderFormat::Ascii)
                .render(Source::new(&name, source))
                .unwrap();
            assert!(
                report.output.starts_with(expected_header),
                "{}",
                report.output
            );
            assert!(
                report.output.trim_end().ends_with(expected_footer),
                "{}",
                report.output
            );
        }
    }

    #[test]
    fn overwide_mdoc_system_footer_still_centres_a_fitting_date() {
        let name = SourceName::new("terminal-os-long.1").unwrap();
        let system =
            "1234567890123456789012345678901234567890123456789012345678901234567890123456789";
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                format!(".Dd July 4, 2017\n.Dt OS-LONG 1\n.Os {system}\n.Sh DESCRIPTION\ntext\n")
                    .as_bytes(),
            ))
            .unwrap();
        assert!(
            report.output.ends_with(&format!(
                "{system}\n                                 July 4, 2017\n{system}\n"
            )),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_argumentless_date_retains_the_blank_date_footer() {
        let name = SourceName::new("terminal-dd-noarg.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd\n.Dt DD-NOARG 1\n.Os\n.Sh DESCRIPTION\ntext\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .ends_with("\n\nOpenBSD                                                                OpenBSD\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_title_without_section_uses_the_local_header_volume() {
        let name = SourceName::new("terminal-dt-nosec.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DT-NOSEC\n.Os\n.Sh DESCRIPTION\ntext\n",
            ))
            .unwrap();
        assert!(
            report.output.starts_with(
                "DT-NOSEC                             LOCAL                            DT-NOSEC\n"
            ),
            "{}",
            report.output
        );
        assert!(report.output.ends_with("OpenBSD\n"), "{}", report.output);
    }

    #[test]
    fn man_pd_controls_later_paragraph_vertical_density_without_visible_text() {
        let name = SourceName::new("terminal-pd.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH PD 1 \"July 4, 2017\"\n.SH DESCRIPTION\n.PD 2v\n.PP\nfirst\n.PP\nsecond\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("D\u{8}DE\u{8}ES\u{8}SC\u{8}CR\u{8}RI\u{8}IP\u{8}PT\u{8}TI\u{8}IO\u{8}ON\u{8}N\n       first\n\n\n       second"),
            "{}",
            report.output
        );
        assert!(!report.output.contains("2v"), "{}", report.output);
    }

    #[test]
    fn man_pd_bare_numeric_argument_adds_terminal_blank_lines() {
        let name = SourceName::new("terminal-pd-bare.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH PD 1 \"July 4, 2017\"\n.SH DESCRIPTION\ninitial\n.PP\ndefault\n.PD 2\n.PP\nnext\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("       default\n\n\n       next"),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_pd_controls_following_section_heading_density() {
        let name = SourceName::new("terminal-pd-section.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH PD 1\n.SH DESCRIPTION\nfirst\n.PD 2\n.SH DOUBLE\nsecond\n.PD 0\n.SS NONE\nthird\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("       first\n\n\nD\u{8}DO\u{8}OU\u{8}UB\u{8}BL\u{8}LE\u{8}E"),
            "{}",
            report.output
        );
        assert!(
            report
                .output
                .contains("       second\n   N\u{8}NO\u{8}ON\u{8}NE\u{8}E"),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_uri_blocks_render_text_before_the_bracketed_resource() {
        let name = SourceName::new("terminal-uri.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH URI 1 \"July 4, 2017\"\n.SH DESCRIPTION\nsee:\n.UR https://example.test/\nexample site\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("see: example site <https://example.test/>"),
            "{}",
            report.output
        );
        let mailto = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH URI 1 \"July 4, 2017\"\n.SH DESCRIPTION\n.MT test@example.test\nmail text\n.ME tail\n.MT\nno-address\n.ME\n",
            ))
            .unwrap();
        assert!(
            mailto
                .output
                .contains("mail text <test@example.test>tail no-address <>"),
            "{}",
            mailto.output
        );
        let empty_uri = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH URI 1 \"July 4, 2017\"\n.SH DESCRIPTION\n.UR\nlink text\n.UE\n",
            ))
            .unwrap();
        assert!(
            empty_uri.output.contains("link text <>"),
            "{}",
            empty_uri.output
        );
    }

    #[test]
    fn man_synopsis_blocks_keep_filled_and_literal_argument_fields() {
        let name = SourceName::new("terminal-sy.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH SY 1\n.SH DESCRIPTION\nbefore\n.SY command\n.I argument\n.YS\n.nf\n.SY literal\n.I argument\n.YS\n.fi\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "       before\n\n       c\u{8}co\u{8}om\u{8}mm\u{8}ma\u{8}an\u{8}nd\u{8}d _\u{8}a_\u{8}r_\u{8}g_\u{8}u_\u{8}m_\u{8}e_\u{8}n_\u{8}t\n\n       l\u{8}li\u{8}it\u{8}te\u{8}er\u{8}ra\u{8}al\u{8}l\n               _\u{8}a_\u{8}r_\u{8}g_\u{8}u_\u{8}m_\u{8}e_\u{8}n_\u{8}t"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_rs_uses_signed_n_and_i_indentation_units() {
        let name = SourceName::new("terminal-rs.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH RS 1 \"July 4, 2017\"\n.SH DESCRIPTION\n.RS -14n\nleft\n.RE\n.RS -0.36i\nthree\n.RE\n.RS 0.36i\neleven\n.RE\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("\nleft\n   three\n           eleven\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn widthless_man_rs_restores_the_current_field_margin() {
        let name = SourceName::new("terminal-rs-field-margin.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH RS 1\n.SH DESCRIPTION\n.TP 2n\n\\(bu\nbullet list\n.RS\nindented text\n.RE\nregular text\n.RS\ntop-level indented list\n.RE\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "       +\u{8}o bullet list\n         indented text\n       regular text\n         top-level indented list"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_rs_truncates_unsuffixed_fractional_widths_to_terminal_cells() {
        let name = SourceName::new("terminal-rs-decimal.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH RS 1\n.SH DESCRIPTION\n.RS 0.0\nzero\n.RS 3.5\nthree\n.RE\nzero again\n.RE\nplain\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("       zero\n          three\n       zero again\n       plain"),
            "{}",
            report.output
        );
    }

    #[test]
    fn man_stray_re_after_ip_consumes_the_field_paragraph_slot() {
        let name = SourceName::new("terminal-lonely-re.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH LONELY-RE 1\n.SH DESCRIPTION\n.IP tag 6n\nbody\n.RE\nout of body\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("       tag   body\n       out of body"),
            "{}",
            report.output
        );
        assert!(
            !report
                .output
                .contains("       tag   body\n\n       out of body"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_closing_delimiters_attach_without_source_spacing() {
        let name = SourceName::new("terminal-mdoc-delimiter.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DELIMITER 1\n.Os\n.Sh DESCRIPTION\n.Dv value \";\"\n",
            ))
            .unwrap();
        assert!(report.output.contains("value;"), "{}", report.output);
        assert!(!report.output.contains("value ;"), "{}", report.output);
    }

    #[test]
    fn man_subsections_and_paragraph_blocks_have_terminal_geometry() {
        let name = SourceName::new("terminal-subsection.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".TH SUBSECTION 1\n.SH DESCRIPTION\n.SS nested heading\nfirst paragraph\n.PP\nsecond paragraph\n",
            ))
            .unwrap();
        assert!(report
            .output
            .contains("   n\u{8}ne\u{8}es\u{8}st\u{8}te\u{8}ed\u{8}d h\u{8}he\u{8}ea\u{8}ad\u{8}di\u{8}in\u{8}ng\u{8}g\n       first paragraph\n\n       second paragraph"));
    }

    #[test]
    fn man_pd_controls_before_empty_subsections_do_not_create_blank_lines() {
        let name = SourceName::new("terminal-ss-pd.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH SS 1 \"July 4, 2017\"\n.SH DESCRIPTION\n.PD 2v\n.SS First\n.PD 1v\n.SS Second\ntext\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "D\u{8}DE\u{8}ES\u{8}SC\u{8}CR\u{8}RI\u{8}IP\u{8}PT\u{8}TI\u{8}IO\u{8}ON\u{8}N\n   F\u{8}Fi\u{8}ir\u{8}rs\u{8}st\u{8}t\n   S\u{8}Se\u{8}ec\u{8}co\u{8}on\u{8}nd\u{8}d\n       text"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_system_names_keep_optional_versions_in_one_terminal_word() {
        let name = SourceName::new("terminal-system-version.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .with_width(20)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SYSTEM 1\n.Os\n.Sh DESCRIPTION\none two three\n.Ox 6.1\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     one two three\n     OpenBSD 6.1"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_word_keep_holds_a_system_macro_and_its_line_tail_together() {
        let name = SourceName::new("terminal-system-word-keep.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SYSTEM 1\n.Os\n.Sh DESCRIPTION\nBecause we use a keep,\n.Bk -words\n.Ox 4.9 must be at the beginning of a new line.\n.Ek\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "     Because we use a keep,\n     OpenBSD 4.9 must be at the beginning of a new line."
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_parenthetical_blocks_emit_structural_delimiters() {
        let name = SourceName::new("terminal-parenthetical.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt PARENTHETICAL 1\n.Os\n.Sh DESCRIPTION\nBefore\n.Pq nested words .\nafter\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("     Before (nested words).  after"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_quote_blocks_include_explicit_opening_delimiters() {
        let name = SourceName::new("terminal-quote-blocks.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt QUOTE-BLOCKS 1\n.Os\n.Sh DESCRIPTION\n.Dq \"(\" value)\n.Brq\n.Sq\n",
            ))
            .unwrap();
        assert!(report.output.contains("(\"value)\""), "{}", report.output);
        assert!(report.output.contains("{}"), "{}", report.output);
        assert!(report.output.contains("`'"), "{}", report.output);
    }

    #[test]
    fn mdoc_quote_bodies_keep_nested_lists_structural() {
        let name = SourceName::new("terminal-quote-list.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt QUOTE-LIST 1\n.Os\n.Sh DESCRIPTION\n.Bo before list\n.Bl -enum -offset indent\n.It\ninside both\n.Bc\nafter bracket\n.El\nafter list\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "     [before list\n\n           1.   inside both] after bracket\n     after list"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_quote_bodies_restore_recovered_list_breaks() {
        let name = SourceName::new("terminal-quote-list-break.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt QUOTE-LIST-BREAK 1\n.Os\n.Sh DESCRIPTION\n.Bl -enum -offset indent\n.It\nbefore bracket\n.Bo inside both\n.El\n.It\nstray item\n.Bc\nafter both\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "           1.   before bracket [inside both\n                stray item]\n     after both"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_bf_body_uses_its_normalized_font_as_the_terminal_base() {
        let name = SourceName::new("terminal-bf.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BF 1\n.Os\n.Sh DESCRIPTION\n.Bf -emphasis\nvalue\\fBbold\\fPtail\n.Ef\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("_\u{8}v_\u{8}a_\u{8}l_\u{8}u_\u{8}eb\u{8}bo\u{8}ol\u{8}ld\u{8}d_\u{8}t_\u{8}a_\u{8}i_\u{8}l"),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_renderer_retains_adjacent_authored_and_escaped_spaces() {
        let name = SourceName::new("terminal-multiple-space.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt MULTIPLE 1\n.Os\n.Sh DESCRIPTION\ntwo spaces  here\n.Pp\ntwo escaped spaces\\ \\ here\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("     two spaces  here"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("     two escaped spaces  here"),
            "{}",
            report.output
        );
    }

    #[test]
    fn terminal_nonbreaking_spaces_move_the_entire_phrase_to_the_next_line() {
        assert!(!TERMINAL_NONBREAKING_SPACE_MARKER.is_whitespace());
        let input = format!("     123456789012 x{TERMINAL_NONBREAKING_SPACE_MARKER}x");
        assert_eq!(
            wrap_terminal_output(&input, 20, DEFAULT_RENDER_OUTPUT_BYTES, 0, 0).unwrap(),
            "     123456789012\n     x x"
        );
    }

    #[test]
    fn terminal_sentence_flags_survive_attached_closing_delimiters() {
        let name = SourceName::new("terminal-sentence-delimiter.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH SENTENCE 1\n.SH DESCRIPTION\nShe said: \"A sentence.\"\nAnd continued.\nA parenthesized dot (.) is not terminal punctuation.\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("She said: \"A sentence.\"  And continued."),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("parenthesized dot (.) is not"),
            "{}",
            report.output
        );
    }

    #[test]
    fn filled_leading_source_space_retains_a_terminal_line_and_column() {
        let name = SourceName::new("terminal-leading-space.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LEADING-SPACE 1\n.Os\n.Sh DESCRIPTION\nfirst line\n leading line\nfollowing words\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("     first line\n      leading line following words")
        );
    }

    #[test]
    fn mdoc_dl_preserves_its_indentation_and_can_wrap_as_terminal_prose() {
        let name = SourceName::new("terminal-dl.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DL 1\n.Os\n.Sh DESCRIPTION\n.Dl one-line display\n",
            ))
            .unwrap();
        assert!(report.output.contains("\n           one-line display\n"));
    }

    #[test]
    fn mdoc_dl_uses_a_discretionary_break_only_when_the_line_overflows() {
        let name = SourceName::new("terminal-dl-break.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .with_width(20)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DL-BREAK 1\n.Os\n.Sh DESCRIPTION\n.Dl alpha,\\:beta\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("\n           alpha,\n           beta\n")
        );
    }

    #[test]
    fn man_ip_separates_a_tabbed_tag_from_its_indented_body() {
        let name = SourceName::new("terminal-ip.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH IP 1\n.SH DESCRIPTION\n.IP single\ttab 3n\ntext\n.PP\n.B single\\ttab\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("\n\n       single    tab\n          text\n\n"),
            "{}",
            report.output
        );
        assert!(report.output.contains(
            "       s\u{8}si\u{8}in\u{8}ng\u{8}gl\u{8}le\u{8}e    t\u{8}ta\u{8}ab\u{8}b"
        ));
    }

    #[test]
    fn man_field_after_a_recovered_section_blank_is_detected() {
        let name = SourceName::new("terminal-ip-section-blank.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH IP 1\n.SH DESCRIPTION\n\n.IP tag\nbody\n",
            ))
            .unwrap();
        let field = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("IP"))
            .unwrap();
        assert!(super::terminal_follows_empty_section_paragraph(field));
    }

    #[test]
    fn man_ip_uses_the_default_tag_field_and_ignores_trailing_tag_blanks() {
        let name = SourceName::new("terminal-ip-field.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH IP 1\n.SH DESCRIPTION\n.IP tag\nbody\n.IP \"tag    \"\nbody\n.IP seseven\nbody\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "       tag    body\n\n       tag    body\n\n       seseven\n              body"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn empty_man_ip_body_does_not_leave_unused_tag_field_padding() {
        let name = SourceName::new("terminal-empty-ip.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH IP 1\n.SH DESCRIPTION\n.IP tag1 10n\n.IP tag2\nbody\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("       tag1\n\n       tag2      body")
        );
        assert!(!report.output.contains("tag1      \n"));
    }

    #[test]
    fn man_ip_inside_rs_closes_without_an_extra_vertical_gap() {
        let name = SourceName::new("terminal-ip-in-rs.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH IP 1\n.SH DESCRIPTION\n.IP\n.RS\n.IP tag\ninside\n.RE\nafter\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("              tag    inside\n       after")
        );
        assert!(!report.output.contains("tag    inside\n\n       after"));
    }

    #[test]
    fn man_ip_uses_only_its_tag_and_optional_scaled_width() {
        assert_eq!(super::terminal_signed_roff_en_prefix("-10n"), Some(-10));
        assert_eq!(super::terminal_signed_roff_en_prefix("-0.36i"), Some(-4));
        assert_eq!(super::terminal_signed_roff_en_prefix("1cx"), Some(4));
        assert_eq!(super::terminal_signed_roff_en_prefix("xxx"), None);

        let name = SourceName::new("terminal-ip-arguments.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH IP 1\n.SH DESCRIPTION\n.nf\n.IP tag 4n ignored\nliteral\n.fi\n",
            ))
            .unwrap();
        assert!(report.output.contains("       tag literal"));
        assert!(!report.output.contains("ignored"));
    }

    #[test]
    fn man_pd_density_applies_to_ip_field_boundaries() {
        let name = SourceName::new("terminal-ip-density.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH IP 1\n.SH DESCRIPTION\n.PD 2v\n.IP tag\nfirst\n.IP tag\nsecond\n",
            ))
            .unwrap();
        assert!(report.output.contains("N\x08N\n       tag    first"));
        assert!(
            report
                .output
                .contains("       tag    first\n\n\n       tag    second"),
            "{}",
            report.output
        );

        let zero_density = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH IP 1\n.SH DESCRIPTION\n.PD 0\n.IP tag\nfirst\n.TP\nnext\ntext\n",
            ))
            .unwrap();
        assert!(
            zero_density
                .output
                .contains("       tag    first\n       next   text"),
            "{}",
            zero_density.output
        );
    }

    #[test]
    fn long_man_ip_tags_wrap_without_losing_the_body_field() {
        let name = SourceName::new("terminal-long-ip-tag.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH IP 1\n.SH DESCRIPTION\n.IP \"This indented paragraph has ridiculously long text in its head, such that it doesn't even fit on the line\" 6n\nbody\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "       This indented paragraph has ridiculously long text in its head, such\n       that it doesn't even fit on the line\n             body"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn roff_center_and_right_adjust_requests_own_no_fill_input_lines() {
        let name = SourceName::new("terminal-adjusted-input.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH ADJUST 1\n.SH DESCRIPTION\nbefore\n.ce 2\ncenter\nsecond\n.rj 1\nright\nafter\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(&format!(
                "       before\n{}center\n{}second\n{}right\n       after",
                " ".repeat(39),
                " ".repeat(39),
                " ".repeat(73),
            )),
            "{}",
            report.output
        );
    }

    #[test]
    fn roff_line_length_requests_are_stateful_and_reset_to_renderer_width() {
        let name = SourceName::new("terminal-line-length.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .with_width(20)
            .render(Source::new(
                &name,
                b".ll 8n\none two three four\n.br\n.ll\none two three four\n",
            ))
            .unwrap();
        assert_eq!(report.output, "one two\nthree\nfour\none two three four\n");
    }

    #[test]
    fn roff_indent_requests_start_new_fields_and_reset_at_a_paragraph() {
        let name = SourceName::new("terminal-indent.1").unwrap();
        let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".TH INDENT 1\n.SH DESCRIPTION\nbefore\n.in 4n\nafter\n.PP\nreset\n",
            ))
            .unwrap();
        assert!(
            report
                .output
                .contains("       before\n    after\n\n       reset"),
            "{}",
            report.output
        );
    }

    #[test]
    fn mdoc_reference_blocks_apply_bibliography_punctuation_and_fonts() {
        let name = SourceName::new("terminal-reference.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd January 4, 2019\n.Dt REFERENCE 1\n.Os\n.Sh AUTHORS\n.Rs\n.%A first\n.%A second\n.%A third\n.%T title\n.%J journal\n.Re\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "first, second, and third, \"title\", _\u{8}j_\u{8}o_\u{8}u_\u{8}r_\u{8}n_\u{8}a_\u{8}l."
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn html_reference_blocks_keep_citations_inline_except_in_see_also() {
        let name = SourceName::new("html-reference.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".Dd January 7, 2019\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\ninitial reference:\n.Rs\n.%A author name\n.%B book title\n.Re\n.Pp\nin a paragraph:\n.Rs\n.%A another author\n.%B another book\n.Re\n.Sh SEE ALSO\ninitial reference:\n.Rs\n.%A author name\n.%B book title\n.Re\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "initial reference: <cite class=\"Rs\"><span class=\"RsA\">author\n    name</span>, <i class=\"RsB\">book title</i>.</cite></p>\n<p class=\"Pp\">in a paragraph: <cite class=\"Rs\"><span class=\"RsA\">another\n    author</span>, <i class=\"RsB\">another book</i>.</cite></p>"
            ),
            "{}",
            report.output
        );
        assert!(
            report.output.contains(
                "<a class=\"permalink\" href=\"#SEE_ALSO\">SEE\n  ALSO</a></h1>\n<p class=\"Pp\">initial reference:</p>\n<p class=\"Pp\"><cite class=\"Rs\"><span class=\"RsA\">author name</span>,\n    <i class=\"RsB\">book title</i>.</cite></p>"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn html_ft_requests_keep_font_state_in_one_paragraph() {
        let name = SourceName::new("html-ft.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".TH FT 1\n.SH DESCRIPTION\ndefault\n.ft I\nitalic\n.ft CR\nliteral\n.ft B\nbold\n.ft I bogus\nitalic again\n.ft P\nstill italic\n.ft\nstill italic\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "default <i>italic</i> <span class=\"Li\">literal</span>\n    <b>bold</b> <i>italic again</i> <i>still italic</i> <i>still italic</i>"
            ),
            "{}",
            report.output
        );
        assert!(!report.output.contains("<p class=\"Pp\">I"));
    }

    #[test]
    fn html_tbl_layout_metadata_merges_rows_and_keeps_fonts() {
        let name = SourceName::new("html-tbl-layout.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".TH TBL 1\n.SH DESCRIPTION\n.TS\nbox tab(:);\nlb r\nl ri.\nbold:roman\n_\nroman:italic\n.TE\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "<table class=\"tbl\" style=\"border-style: solid;\">\n  <tr style=\"border-bottom-style: solid;\">\n    <td><b>bold</b></td>\n    <td style=\"text-align: right;\">roman</td>\n  </tr>\n  <tr>\n    <td>roman</td>\n    <td style=\"text-align: right;\"><i>italic</i></td>\n  </tr>\n</table>"
            ),
            "{}",
            report.output
        );
        assert_eq!(report.output.matches("<table").count(), 1);
    }

    #[test]
    fn html_escapes_visible_text_and_preserves_parse_diagnostics() {
        let name = SourceName::new("render.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(&name, b".TH RENDER 1\n.SH NAME\n<&>\n"))
            .unwrap();
        assert!(report.output.contains("&lt;&amp;&gt;"));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn html_source_lines_are_not_synthetic_break_elements() {
        let name = SourceName::new("html-source-lines.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b"first source line\nsecond source line\n",
            ))
            .unwrap();
        assert_eq!(report.output, "first source line\nsecond source line");
    }

    #[test]
    fn html_font_blocks_wrap_only_their_body_and_keep_nested_paragraphs() {
        let name = SourceName::new("html-font-block.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FONT-BLOCK 1\n.Os\n.Sh DESCRIPTION\n.Pp\nnormal text\n.Bf -literal\nliteral text\n.Pp\nliteral paragraph\n.Ef\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "<div class=\"Bf Li\">literal text\n<p class=\"Pp\">literal paragraph</p>\n</div>"
            ),
            "{}",
            report.output
        );
        assert!(!report.output.contains("-literal"), "{}", report.output);
    }

    #[test]
    fn html_one_line_displays_keep_their_phrase_break_and_literal_wrapper() {
        let name = SourceName::new("html-one-line-display.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Tg display\n.D1 spacing  in  and around one-line displays\nempty display:\n.D1\n.Tg literal\n.Dl literal  display\n.Dl\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "<div class=\"Bd\n  Bd-indent\" id=\"display\"><a class=\"permalink\" href=\"#display\">spacing</a> in\n  and around one-line displays</div>"
            ),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("<div class=\"Bd Bd-indent\"></div>"),
            "{}",
            report.output
        );
        assert!(
            report.output.contains(
                "<div class=\"Bd\n  Bd-indent\" id=\"literal\"><code class=\"Li\"><a class=\"permalink\" href=\"#literal\">literal</a>\n  display</code></div>"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn html_man_blocks_keep_field_indent_synopsis_and_literal_boundaries() {
        let name = SourceName::new("html-man-blocks.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".TH BLOCKS 1\n.SH DESCRIPTION\n.PD 2v\n.TP 10n\ntag\nbody\n.HP 10n\nhanging body\n.RS\nindented body\n.PP\nnested body\n.RE\n.SY command\n.I arguments\n.YS\n.PP\nregular paragraph\n.nf\nliteral\ntext\n.fi\nregular tail\n.br\n",
            ))
            .unwrap();
        assert!(report.output.contains(
            "<dl class=\"Bl-tag\">\n  <dt id=\"tag\"><a class=\"permalink\" href=\"#tag\">tag</a></dt>\n  <dd>body</dd>\n</dl>"
        ));
        assert!(
            report
                .output
                .contains("<p class=\"Pp HP\">hanging body</p>")
        );
        assert!(report.output.contains(
            "<div class=\"Bd-indent\">indented body\n<p class=\"Pp\">nested body</p>\n</div>"
        ));
        assert!(report.output.contains(
            "<table class=\"Nm\">\n  <tr>\n    <td><code class=\"Nm\">command</code></td>\n    <td><i>arguments</i></td>\n  </tr>\n</table>"
        ));
        assert!(report.output.contains(
            "<p class=\"Pp\">regular paragraph</p>\n<pre>literal\ntext</pre>\nregular tail\n<br/>"
        ));
        assert!(!report.output.contains("2v"), "{}", report.output);
    }

    #[test]
    fn html_mdoc_displays_keep_nested_blocks_literal_flow_and_targets() {
        let name = SourceName::new("html-mdoc-displays.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Tg outer\n.Bd -ragged -offset indent\nouter text\n.Pq default indent\n.Tg inner\n.Bd -ragged -offset indent\ninner text\n.Ed\nouter text\n.Ed\n.Bl -tag\n.It term\nouter text\n.Bd -ragged -offset 2n\ninner text\n.Ed\nouter text\n.El\n.Tg literal\n.Bd -literal\nliteral display\n.Tg paragraph\n.Pp\nliteral paragraph\n.Ed\n",
            ))
            .unwrap();
        assert!(report.output.contains(
            "<div class=\"Bd Pp\n  Bd-indent\" id=\"outer\"><a class=\"permalink\" href=\"#outer\">outer</a> text\n  (default indent)\n<div class=\"Bd Pp\n  Bd-indent\" id=\"inner\"><a class=\"permalink\" href=\"#inner\">inner</a> text</div>\nouter text</div>"
        ), "{}", report.output);
        assert!(report.output.contains(
            "<dd>outer text\n    <div class=\"Bd Pp Bd-indent\">inner text</div>\n    outer text</dd>"
        ), "{}", report.output);
        assert!(report.output.contains(
            "<div class=\"Bd Pp Li\" id=\"literal\">\n<pre><a class=\"permalink\" href=\"#literal\">literal</a> display\n<mark id=\"paragraph\"></mark>\n<a class=\"permalink\" href=\"#paragraph\">literal</a> paragraph</pre>\n</div>"
        ), "{}", report.output);
    }

    #[test]
    fn html_roff_font_escapes_emit_semantic_and_literal_spans() {
        let name = SourceName::new("html-font-escape.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".TH FONT 1\n.SH DESCRIPTION\n.nf\n\\f4bolditalic\\f3bold\\f2italic\\f1roman\n\\f(CWliteral\\f(CBbold\\f(CIitalic\\fRroman\n",
            ))
            .unwrap();
        assert!(
            report.output.contains(
                "<b><i>bolditalic</i></b><b>bold</b><i>italic</i>roman\n<span class=\"Li\">literal</span><span class=\"Li\"><b>bold</b></span><span class=\"Li\"><i>italic</i></span>roman"
            ),
            "{}",
            report.output
        );
    }

    #[test]
    fn html_plain_paragraphs_fold_at_the_device_output_field() {
        assert_eq!(
            wrap_html_plain_paragraph(
                "We are using the html device. It can also be written as the html device.",
                "<p class=\"Pp\">".len(),
            ),
            "We are using the html device. It can also be written as the html\n    device."
        );
        assert_eq!(
            wrap_html_plain_paragraph("<i>semantic markup stays intact</i>", 14),
            "<i>semantic markup stays intact</i>"
        );
    }

    #[test]
    fn html_tg_marks_and_inline_semantic_macros_stay_in_their_paragraph() {
        let name = SourceName::new("html-tg.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt TAG 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Tg paragraph\ninitial text\n.Tg macro\n.Ic macro\nfollowing text\n.Tg marker\n.Tg subsection\n.Ss next\ntext\n",
            ))
            .unwrap();
        assert!(report.output.contains(
            "<p class=\"Pp\" id=\"paragraph\">initial text\n    <a class=\"permalink\" href=\"#macro\"><code class=\"Ic\" id=\"macro\">macro</code></a>\n    following text <mark id=\"marker\"></mark></p>"
        ), "{}", report.output);
    }

    #[test]
    fn html_function_macros_keep_callable_links_and_fo_arguments_together() {
        let name = SourceName::new("html-functions.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Pp\nautomatic:\n.Fn first\nand\n.Fn second\n.Pp\n.Fn second\nand\n.Fn first\n.Pp\nexplicit:\n.Tg e3\n.Fn third\nand\n.Tg e4\n.Fo fourth\n.Fa void\n.Fc\n",
            ))
            .unwrap();
        assert!(report.output.contains(
            "<p class=\"Pp\" id=\"first\">automatic:\n    <a class=\"permalink\" href=\"#first\"><code class=\"Fn\">first</code></a>() and\n    <code class=\"Fn\">second</code>()</p>"
        ), "{}", report.output);
        assert!(report.output.contains(
            "<p class=\"Pp\" id=\"e3\">explicit:\n    <a class=\"permalink\" href=\"#e3\"><code class=\"Fn\">third</code></a>() and\n    <a class=\"permalink\" href=\"#e4\"><code class=\"Fn\" id=\"e4\">fourth</code></a>(<var class=\"Fa\">void</var>);</p>"
        ), "{}", report.output);
    }

    #[test]
    fn html_no_fill_spacing_request_stays_inside_one_preformatted_region() {
        let name = SourceName::new("html-no-fill-space.1").unwrap();
        let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".TH SPACE 1\n.SH DESCRIPTION\n.nf\nfirst\n.sp\nsecond\n.fi\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("<pre>first\n\nsecond</pre>"),
            "{}",
            report.output
        );
    }

    #[test]
    fn html_text_escapes_required_characters_and_non_ascii_scalars() {
        assert_eq!(
            escape_html("'\"<&>\u{a1}\u{1f642}"),
            "'&quot;&lt;&amp;&gt;&#x00A1;&#x1F642;"
        );
    }

    #[test]
    fn terminal_two_character_math_escapes_use_catalog_ascii_fallbacks() {
        let limits = Limits::default();
        assert_eq!(
            render_visible_text(r"\(<<", RenderFormat::Ascii, &limits),
            "<<"
        );
        assert_eq!(
            render_visible_text(r"\(>>", RenderFormat::Ascii, &limits),
            ">>"
        );
        assert_eq!(
            render_visible_text(r"\(~=", RenderFormat::Ascii, &limits),
            "~="
        );
    }

    #[test]
    fn mdoc_prefix_attaches_only_to_the_next_same_line_token() {
        let name = SourceName::new("terminal-prefix.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .with_width(200)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt PREFIX 1\n.Os\n.Sh DESCRIPTION\nClosing\n.Pf . right .\nOpening\n.Pf ( left .\nNormal\n.Pf pre fixed .\nIncomplete\n.Pf prefixed\nto next line.\n.Po enclosure Pf . Pc\n",
            ))
            .unwrap();
        assert!(
            report.output.contains("Closing .right."),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("Opening (left."),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("Normal prefixed."),
            "{}",
            report.output
        );
        assert!(
            report.output.contains("Incomplete prefixed to next line."),
            "{}",
            report.output
        );
        assert!(report.output.contains("enclosure .)"), "{}", report.output);
    }

    #[test]
    fn man_layout_requests_are_not_visible_tagged_field_bodies() {
        let name = SourceName::new("terminal-layout-only-field.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH FIELD 1\n.SH DESCRIPTION\n.IP tag 6n\n.sp 2v\nfollowing IP text\n.TP 6n\ntag\n.sp 2v\nfollowing TP text\n",
            ))
            .unwrap();
        assert!(!report.output.contains("tag  \n"), "{}", report.output);
        assert!(
            report.output.contains("       tag\n\n\n"),
            "{}",
            report.output
        );
    }

    #[test]
    fn output_limit_never_returns_a_partial_report() {
        let name = SourceName::new("render-limit.1").unwrap();
        let error = Renderer::new(RenderFormat::Utf8)
            .with_max_output_bytes(1)
            .render(Source::new(&name, b"plain text\n"))
            .unwrap_err();
        assert_eq!(error.kind, RenderErrorKind::OutputLimit);
    }

    #[test]
    fn parser_configuration_is_reused() {
        let renderer = Renderer::new(RenderFormat::Ascii).with_parser(Parser::default());
        assert_eq!(renderer.width(), 78);
        assert_eq!(renderer.format(), RenderFormat::Ascii);
    }
}
