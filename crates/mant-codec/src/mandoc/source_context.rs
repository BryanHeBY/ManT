//! Source facts and operation-local services; no hidden formatter registers.
use super::{
    BTreeMap, Diagnostic, EquationDelimiterChange, HashMap, HashSet, MacroSet, Node, RefCell,
    SourceLineIndex, equation_delimiter_changes, formatter, inline,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum MdocSectionContext {
    #[default]
    Other,
    Synopsis,
    Authors,
}

pub(super) struct LoweringContext<'a> {
    pub(super) content: super::content::LegacyContent,
    pub(super) macro_set: MacroSet,
    pub(super) native_heads: RefCell<crate::definitions::NativeHeadEvidence>,
    // Immutable source services. Formatter execution is passed separately;
    // the RefCells below are memoization and diagnostic collection only.
    pub(super) default_name: Option<&'a str>,
    pub(super) source_lines: Option<SourceLineIndex<'a>>,
    pub(super) equation_delimiters: Vec<EquationDelimiterChange>,
    pub(super) normalized_equations: RefCell<BTreeMap<String, String>>,
    native_table_requests: RefCell<HashMap<String, bool>>,
    pub(super) section_ids: HashMap<String, usize>,
    pub(super) assigned_section_ids: HashSet<String>,
    pub(super) authored_section_targets: HashMap<String, Option<String>>,
    pub(super) explicit_targets: HashSet<String>,
    pub(super) diagnostics: RefCell<Vec<Diagnostic>>,
    active_mdoc_section: std::cell::Cell<MdocSectionContext>,
}

#[derive(Debug)]
pub(super) struct TableTextBlock {
    pub(super) source: String,
    pub(super) escape: Option<u8>,
}

/// Apply the lexical comment rule that roff executes before handing text to
/// tbl.
///
/// This intentionally is not a second roff interpreter.  Source-backed tbl
/// recovery only needs the one lexical transformation that both CVS mandoc
/// and groff perform before parsing a table cell: `\\\"` discards the rest
/// of the physical line and `\\#` discards it while continuing with the next
/// line.  Keeping it at the source-recovery boundary prevents comment prose
/// from becoming a synthetic table cell when libmandoc has correctly omitted
/// that cell from its owned table snapshot.
fn table_execution_source(source: &str, initial_escape: Option<u8>) -> String {
    let mut output = String::with_capacity(source.len());
    let escape = initial_escape;
    for physical_line in source.split_inclusive('\n') {
        let (line, had_newline) = physical_line.strip_suffix("\r\n").map_or_else(
            || {
                physical_line
                    .strip_suffix('\n')
                    .map_or((physical_line, false), |line| (line, true))
            },
            |line| (line, true),
        );
        let (visible, continues) = roff_line_without_comment(line, escape);
        output.push_str(visible);
        if had_newline && !continues {
            output.push('\n');
        }
    }
    output
}

/// Return the printable prefix of one roff input line and whether `\\#`
/// suppresses its trailing newline.
///
/// CVS mandoc's `roff_parse_comment()` scans a physical line before
/// `tbl_read()`: an escaped escape skips both bytes, while `\\\"` and `\\#`
/// start a comment.  This mirrors only that lexical contract; callers supply
/// the already-executed `.ec`/`.eo` state for their source coordinate.
fn roff_line_without_comment(line: &str, escape: Option<u8>) -> (&str, bool) {
    let Some(escape) = escape else {
        return (line, false);
    };
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != escape {
            index += 1;
            continue;
        }
        match bytes.get(index.saturating_add(1)).copied() {
            Some(next) if next == escape => index = index.saturating_add(2),
            Some(b'"' | b'#') => {
                let mut end = index;
                // Match mandoc's deliberate whitespace rule: remove only
                // literal spaces immediately preceding a comment, unless
                // that space was itself escaped.
                while end > 0 && bytes[end - 1] == b' ' && (end == 1 || bytes[end - 2] != escape) {
                    end -= 1;
                }
                return (&line[..end], bytes[index + 1] == b'#');
            }
            _ => index += 1,
        }
    }
    (line, false)
}

impl<'a> LoweringContext<'a> {
    pub(super) fn new(default_name: Option<&'a str>, source: Option<&'a str>) -> Self {
        Self {
            content: super::content::LegacyContent::default(),
            macro_set: MacroSet::None,
            native_heads: RefCell::default(),
            default_name,
            source_lines: source.map(SourceLineIndex::new),
            equation_delimiters: source.map_or_else(Vec::new, equation_delimiter_changes),
            normalized_equations: RefCell::new(BTreeMap::new()),
            native_table_requests: RefCell::new(HashMap::new()),
            section_ids: HashMap::new(),
            assigned_section_ids: HashSet::new(),
            authored_section_targets: HashMap::new(),
            explicit_targets: HashSet::new(),
            diagnostics: RefCell::new(Vec::new()),
            active_mdoc_section: std::cell::Cell::new(MdocSectionContext::Other),
        }
    }

    pub(super) fn active_mdoc_section(&self) -> MdocSectionContext {
        self.active_mdoc_section.get()
    }

    pub(super) fn replace_mdoc_section(&self, section: MdocSectionContext) -> MdocSectionContext {
        self.active_mdoc_section.replace(section)
    }

    pub(super) fn lower_inline_with_spacing(
        &self,
        nodes: &[Node],
        spacing: bool,
        formatter: &mut formatter::FormatterState,
    ) -> Vec<mant_ir::Inline> {
        self.lower_inline_with_author_break(
            nodes,
            spacing,
            formatter,
            inline::AuthorBreakEffect::Line,
        )
        .0
    }

    pub(super) fn lower_inline_with_author_break(
        &self,
        nodes: &[Node],
        spacing: bool,
        formatter: &mut formatter::FormatterState,
        author_break_effect: inline::AuthorBreakEffect,
    ) -> (Vec<mant_ir::Inline>, bool, bool) {
        let (output, definition_field_exited, definition_body_gap_consumed) = self
            .lower_inline_draft_with_author_break(nodes, spacing, formatter, author_break_effect);
        (
            self.content.lower(
                mant_ir::ContentRootKind::Body,
                nodes.first().and_then(super::source_span),
                output,
            ),
            definition_field_exited,
            definition_body_gap_consumed,
        )
    }

    pub(super) fn lower_inline_draft_with_author_break(
        &self,
        nodes: &[Node],
        spacing: bool,
        formatter: &mut formatter::FormatterState,
        author_break_effect: inline::AuthorBreakEffect,
    ) -> (Vec<inline::DraftInline>, bool, bool) {
        self.lower_inline_draft_with_option_evidence(
            nodes,
            spacing,
            formatter,
            author_break_effect,
            false,
        )
    }

    pub(super) fn lower_inline_draft_with_option_evidence(
        &self,
        nodes: &[Node],
        spacing: bool,
        formatter: &mut formatter::FormatterState,
        author_break_effect: inline::AuthorBreakEffect,
        head_evidence: bool,
    ) -> (Vec<inline::DraftInline>, bool, bool) {
        let mut builder = formatter.begin_inline_session(
            spacing,
            self.active_mdoc_section() == MdocSectionContext::Authors,
            author_break_effect,
        );
        if head_evidence {
            builder.enable_native_head_marks();
        }
        inline::append_inline_nodes(&mut builder, nodes, self.default_name);
        let finished = formatter.finish_inline_line(builder);
        (
            finished.output,
            finished.definition_field_exited,
            finished.definition_body_gap_consumed,
        )
    }

    /// Lower a definition head without executing a formatter line boundary.
    ///
    /// mdoc inset and diagnostic bodies remain in the same native formatter
    /// stream as their heads.  The returned state owns pending `\z`, `\p`,
    /// and source-continuation effects until the generated run-in cells and
    /// first body word execute.
    pub(super) fn lower_run_in_definition_head<'n>(
        &self,
        groups: impl IntoIterator<Item = &'n [Node]>,
        spacing: bool,
        formatter: &mut formatter::FormatterState,
        strong_scope: bool,
    ) -> (Vec<inline::DraftInline>, inline::PreservedInlineState) {
        let groups = groups.into_iter().collect::<Vec<_>>();
        let mut builder = formatter.begin_inline_session(
            spacing,
            self.active_mdoc_section() == MdocSectionContext::Authors,
            inline::AuthorBreakEffect::Line,
        );
        if self.macro_set == libmandoc_rs::MacroSet::Mdoc {
            builder.enable_native_head_marks();
        }
        let saved_font = strong_scope.then(|| {
            builder
                .font
                .push_scope(super::roff_escape::RoffFont::Strong)
        });
        for nodes in groups {
            inline::append_inline_nodes(&mut builder, nodes, self.default_name);
        }
        if let Some(saved_font) = saved_font {
            builder.font.pop_scope(saved_font);
        }
        formatter.finish_inline_scope(builder)
    }

    /// Execute a section heading in the surrounding formatter stream.
    ///
    /// CVS renders `Sh`/`Ss` heads as a scoped bold font, then calls
    /// `term_newln()`.  The semantic heading supplies its own presentation,
    /// so inherited font state is deliberately not projected into its IR;
    /// spacing and zero-advance execution remain document-global.
    pub(super) fn lower_section_heading(
        &self,
        nodes: &[Node],
        formatter: &mut formatter::FormatterState,
        authors_section: bool,
    ) -> Vec<mant_ir::Inline> {
        let mut builder = formatter.begin_inline_session(
            formatter.spacing,
            authors_section,
            inline::AuthorBreakEffect::Line,
        );
        match self.macro_set {
            MacroSet::Man | MacroSet::None => {
                builder.font.begin_man_heading();
                inline::append_inline_nodes(&mut builder, nodes, self.default_name);
                builder.font.end_man_heading();
            }
            MacroSet::Mdoc => {
                let heading_font = builder.font.push_heading_scope();
                inline::append_inline_nodes(&mut builder, nodes, self.default_name);
                builder.font.pop_heading_scope(heading_font);
            }
        }
        let output = remove_structural_heading_bold(formatter.finish_inline_line(builder).output);
        self.content.lower(
            mant_ir::ContentRootKind::Heading,
            nodes.first().and_then(super::source_span),
            output,
        )
    }

    pub(super) fn lower_text(
        &self,
        source: &str,
        formatter: &mut formatter::FormatterState,
    ) -> Vec<mant_ir::Inline> {
        let output = self.lower_text_draft(source, formatter);
        self.content
            .lower(mant_ir::ContentRootKind::Cell, None, output)
    }

    pub(super) fn lower_text_draft(
        &self,
        source: &str,
        formatter: &mut formatter::FormatterState,
    ) -> Vec<inline::DraftInline> {
        let mut zero_advance = inline::ZeroAdvanceState::new();
        zero_advance.inherit_armed(std::mem::take(&mut formatter.zero_advance_armed));
        let execution = inline::parse_roff_text_with_zero_advance(
            source,
            &mut formatter.font,
            self.macro_set == MacroSet::Mdoc,
            &mut zero_advance,
            false,
        );
        let mut output = execution.output;
        if execution.pending_word_end_break {
            output.push(inline::DraftInline::LineBreak);
        }
        // Even a control-only tbl word enters term_word(): it clears
        // formatter-global skipvsp and can leave a bare BACKAFTER request for
        // the next cell-external word.
        formatter.execute_word();
        formatter.zero_advance_armed = zero_advance.take_armed();
        zero_advance.finish_into(&mut output);
        output
    }

    /// Execute interleaved native source and generated equation glyphs as one
    /// tbl formatter word. Generated Code styling is presentation metadata;
    /// it must not mutate roff's current/previous font registers.
    pub(super) fn lower_formatter_word_parts(
        &self,
        parts: &[inline::FormatterWordPart<'_>],
        formatter: &mut formatter::FormatterState,
    ) -> Vec<mant_ir::Inline> {
        let mut zero_advance = inline::ZeroAdvanceState::new();
        zero_advance.inherit_armed(std::mem::take(&mut formatter.zero_advance_armed));
        let execution = inline::parse_formatter_word_parts_with_zero_advance(
            parts,
            &mut formatter.font,
            self.macro_set == MacroSet::Mdoc,
            &mut zero_advance,
            false,
        );
        let mut output = execution.output;
        if execution.pending_word_end_break {
            output.push(inline::DraftInline::LineBreak);
        }
        formatter.execute_word();
        formatter.zero_advance_armed = zero_advance.take_armed();
        zero_advance.finish_into(&mut output);
        self.content
            .lower(mant_ir::ContentRootKind::Cell, None, output)
    }

    pub(super) fn table_execution_source(source: &str, escape: Option<u8>) -> String {
        table_execution_source(source, escape)
    }

    /// Query libmandoc's pinned roff registry once per distinct table request.
    ///
    /// `tbl_read()` receives raw operands only for unknown or high-level
    /// man/mdoc macros. Native roff requests have already been handled by the
    /// parser and must not leak their operands into recovered cell text.
    pub(super) fn is_native_table_request(&self, name: &str) -> bool {
        if let Some(native) = self.native_table_requests.borrow().get(name) {
            return *native;
        }
        let native = libmandoc_rs::is_native_roff_request(name);
        self.native_table_requests
            .borrow_mut()
            .insert(name.to_owned(), native);
        native
    }

    pub(super) fn table_text_blocks(
        &self,
        line: u32,
        maximum: usize,
        escape: Option<u8>,
    ) -> Vec<TableTextBlock> {
        // Ordinary tbl rows must not scan forward for `T{` markers.  Besides
        // wasting work, that used to let a commented-out multiline-cell
        // marker claim later real rows as its embedded semantic children.
        if maximum == 0 {
            return Vec::new();
        }
        let Some(source_lines) = self.source_lines.as_ref() else {
            return Vec::new();
        };
        let mut blocks = Vec::new();
        let mut current = None::<String>;
        for (_, line) in source_lines.lines_from(line) {
            // Interpret tbl's `T{` / `T}` sentinels after roff has removed
            // inline comments.  A comment can follow a real sentinel, while
            // a comment-only request can mention a disabled sentinel without
            // claiming a later text block.
            let (visible_line, _) = roff_line_without_comment(line, escape);
            let trimmed = visible_line.trim_start();
            if let Some(content) = current.as_mut() {
                if let Some(remainder) = trimmed.strip_prefix("T}") {
                    blocks.push(TableTextBlock {
                        source: std::mem::take(content),
                        escape,
                    });
                    current = None;
                    if blocks.len() == maximum {
                        break;
                    }
                    // tbl serializes adjacent multiline cells as `T}\tT{`.
                    // Closing the first cell must not hide the next opening
                    // marker carried by the same physical source line.
                    if remainder.trim_end().ends_with("T{") {
                        current = Some(String::new());
                    }
                } else {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(line);
                }
            } else if trimmed.trim_end().ends_with("T{") {
                current = Some(String::new());
            }
        }
        blocks
    }

    /// Whether a source-level `.IP` marker uses roff's pre-increment form.
    ///
    /// libmandoc resolves number registers before exposing the owned AST, so
    /// `\n+[step]` and a literal value such as `1` otherwise become
    /// indistinguishable.  Retain only this narrow source fact: it proves an
    /// author-controlled sequence without teaching lowering a second roff
    /// parser or reinterpreting literal numeric option values.
    pub(super) fn man_ip_uses_incrementing_register(&self, line: u32) -> bool {
        let Some(source_line) = self
            .source_lines
            .as_ref()
            .and_then(|source| source.line(line))
        else {
            return false;
        };
        let Some(request) = source_line
            .trim_start()
            .strip_prefix(['.', '\''])
            .map(str::trim_start)
        else {
            return false;
        };
        let Some(arguments) = request
            .strip_prefix("IP")
            .filter(|rest| rest.chars().next().is_none_or(char::is_whitespace))
        else {
            return false;
        };
        inline::roff_macro_arguments(arguments)
            .first()
            .is_some_and(|head| head.contains("\\n+"))
    }
}

/// Heading strength is carried by `Heading`, not by its inline children.
/// Remove exactly the implicit terminal bold layer while retaining explicit
/// emphasis/code information produced inside that scope.
fn remove_structural_heading_bold(nodes: Vec<inline::DraftInline>) -> Vec<inline::DraftInline> {
    let mut output = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            inline::DraftInline::Strong { children } => {
                output.extend(remove_structural_heading_bold(children));
            }
            inline::DraftInline::Emphasis { children } => {
                output.push(inline::DraftInline::Emphasis {
                    children: remove_structural_heading_bold(children),
                });
            }
            inline::DraftInline::Link {
                target,
                title,
                children,
            } => output.push(inline::DraftInline::Link {
                target,
                title,
                children: remove_structural_heading_bold(children),
            }),
            node => output.push(node),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{LoweringContext, table_execution_source};

    #[test]
    fn table_execution_source_matches_roff_inline_comment_and_continuation_rules() {
        assert_eq!(
            table_execution_source("visible \\\" ignored\nnext", Some(b'\\')),
            "visible\nnext"
        );
        assert_eq!(
            table_execution_source("joined \\# ignored\nnext", Some(b'\\')),
            "joinednext"
        );
        assert_eq!(
            table_execution_source(r#"literal \\" remains"#, Some(b'\\')),
            r#"literal \\" remains"#
        );
        assert_eq!(
            table_execution_source(r#"escaped\ \" comment"#, Some(b'\\')),
            r"escaped\ "
        );
    }
    #[test]
    fn table_execution_source_uses_the_native_escape_state_at_the_cell() {
        assert_eq!(
            LoweringContext::table_execution_source("visible @\" ignored", Some(b'@')),
            "visible"
        );
        assert_eq!(
            LoweringContext::table_execution_source("visible \\\" literal", Some(b'@')),
            "visible \\\" literal"
        );
    }
}
