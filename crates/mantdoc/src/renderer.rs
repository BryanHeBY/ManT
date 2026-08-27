//! Bounded native reference rendering built on the public arena view.

// This module deliberately keeps terminal state machines and the pinned device
// character catalogue contiguous. Splitting either by arbitrary line count or
// merging equal catalogue spellings obscures source-order device semantics.
#![allow(clippy::too_many_lines)]

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

mod equation;
mod html;
mod terminal;
mod text;
use equation::{
    ascii_terminal_character, render_html_equation, render_terminal_equation,
    render_terminal_equation_text,
};
use html::render_html_document;
#[cfg(test)]
use html::wrap_html_plain_paragraph;
#[cfg(test)]
use terminal::layout::{
    expand_filled_terminal_tabs, expand_literal_terminal_tabs, terminal_character_width,
    wrap_terminal_output,
};
#[cfg(test)]
use terminal::table::terminal_table_text_block_lines;
use terminal::{
    layout::{ascii_terminal_named_scalar_is_known, display_width, render_visible_text},
    table::table_terminal_cell_starts,
};
#[cfg(test)]
use terminal::{
    render_terminal_bold, terminal_default_volume, terminal_follows_empty_section_paragraph,
    terminal_mdoc_plain_text_sentence, terminal_temporary_indent_target, terminal_vertical_span,
};
use terminal::{
    render_terminal_document, render_terminal_font, terminal_mdoc_section_named,
    terminal_previous_sibling, terminal_signed_roff_en_prefix,
};
use text::{
    append, escape_html, html_request_font_before, render_html_visible_text_with_font,
    render_numeric_character_escapes, render_terminal_visible_text,
    render_terminal_visible_text_with_font, render_terminal_whitespace_escapes,
    render_unicode_character_escapes,
};

#[cfg(test)]
mod tests;
