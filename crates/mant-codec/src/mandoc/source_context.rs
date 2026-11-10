//! Source facts and operation-local services; no hidden formatter registers.
use super::{
    Diagnostic, HashMap, HashSet, MacroSet, Node, RefCell, SourceLineIndex, formatter, inline,
};

mod table_source;
pub(super) use table_source::TableTextBlock;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum MdocSectionContext {
    #[default]
    Other,
    Synopsis,
    Authors,
}

pub(super) struct LoweringContext<'a> {
    pub(super) annotations: inline::ownership::annotations::AnnotationPool,
    pub(super) macro_set: MacroSet,
    pub(super) native_heads: RefCell<crate::definitions::NativeHeadEvidence>,
    // Immutable source services. Formatter execution is passed separately;
    // the RefCells below are memoization and diagnostic collection only.
    pub(super) default_name: Option<&'a str>,
    pub(super) source_lines: Option<SourceLineIndex<'a>>,
    native_table_requests: RefCell<HashMap<String, bool>>,
    /// Optional T{} enhancement work is cumulative across this document,
    /// independent of speculative formatter/diagnostic transactions.
    pub(super) table_recovery_budget: RefCell<super::table_recovery_budget::TableRecoveryBudget>,
    pub(super) table_layout_budget: RefCell<super::table_layout_budget::TableLayoutBudget>,
    pub(super) section_ids: HashMap<String, usize>,
    pub(super) assigned_section_ids: HashSet<String>,
    pub(super) authored_section_targets: HashMap<String, Option<String>>,
    /// Immutable authored namespace reserved for normalized identities.
    pub(super) explicit_targets: HashSet<String>,
    /// Immutable `NODE_ID` owners collected before structural lowering.
    pub(super) native_target_owners: HashSet<String>,
    pub(super) diagnostics: RefCell<Vec<Diagnostic>>,
    pub(super) escape_coverage: super::escape_coverage::EscapeCoverage,
    /// Private pre/children/post execution shared by detached output buffers.
    pub(super) scope_posts: super::containers::ScopePostState,
    active_mdoc_section: std::cell::Cell<MdocSectionContext>,
}

/// Output and explicit execution carried from a run-in HEAD to its BODY.
pub(super) struct RunInDefinitionHead {
    pub(super) output: Vec<mant_ir::Inline>,
    pub(super) execution: inline::PreservedInlineState,
    pub(super) term_breaks: Vec<usize>,
    pub(super) surviving_cells: usize,
    pub(super) generated_word: bool,
}

/// Borrowed source order at one structural HEAD entry. Direct siblings run
/// before HEAD pre, while accepted output keeps the same content owner.
#[derive(Clone, Copy)]
pub(super) struct HeadNodes<'a> {
    pub(super) before: &'a [Node],
    pub(super) content: &'a [Node],
    pub(super) source_origin_units: Option<usize>,
}

impl<'a> LoweringContext<'a> {
    pub(super) fn new(default_name: Option<&'a str>, source: Option<&'a str>) -> Self {
        Self {
            annotations: inline::ownership::annotations::AnnotationPool::deferred(),
            macro_set: MacroSet::None,
            native_heads: RefCell::default(),
            default_name,
            source_lines: source.map(SourceLineIndex::new),
            native_table_requests: RefCell::new(HashMap::new()),
            table_recovery_budget: RefCell::default(),
            table_layout_budget: RefCell::default(),
            section_ids: HashMap::new(),
            assigned_section_ids: HashSet::new(),
            authored_section_targets: HashMap::new(),
            explicit_targets: HashSet::new(),
            native_target_owners: HashSet::new(),
            diagnostics: RefCell::new(Vec::new()),
            escape_coverage: super::escape_coverage::EscapeCoverage::default(),
            scope_posts: super::containers::ScopePostState::default(),
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
        // An absent source slice has no node pre/post or term_newln(). In
        // particular, empty list control groups must not replace the active
        // column flags and close its still-open device row before an It
        // HEAD enters (mdoc_term.c::termp_bl_pre/termp_it_pre).
        if nodes.is_empty() {
            return Vec::new();
        }
        formatter.execution.set_spacing_enabled(spacing);
        let mut output = Vec::new();
        // A group of source controls is a traversal destination, not an It
        // HEAD/BODY. print_mdoc_node() executes each control but there is no
        // synthetic termp_it_post() when this helper returns. Borrowing the
        // active execution keeps the surrounding column's viscol/minbl and
        // pad flags until the next actual node pre/post consumes them.
        formatter.with_output_builder(&mut output, |builder| {
            builder.scope_posts = self.scope_posts.clone();
            builder.observe_no_fill_source_lines(true);
            inline::append_inline_nodes(builder, nodes, self.default_name);
            builder.observe_no_fill_source_lines(false);
        });
        output
    }

    pub(super) fn lower_inline_with_author_break_preserving_rows(
        &self,
        nodes: HeadNodes<'_>,
        spacing: bool,
        formatter: &mut formatter::FormatterState,
        author_break_effect: inline::AuthorBreakEffect,
        native_head_field_units: Option<usize>,
    ) -> formatter::FinishedInlineLine {
        self.lower_inline_with_author_break_rows(
            nodes,
            spacing,
            formatter,
            author_break_effect,
            true,
            native_head_field_units,
        )
    }

    fn lower_inline_with_author_break_rows(
        &self,
        nodes: HeadNodes<'_>,
        spacing: bool,
        formatter: &mut formatter::FormatterState,
        author_break_effect: inline::AuthorBreakEffect,
        preserve_rows: bool,
        native_head_field_units: Option<usize>,
    ) -> formatter::FinishedInlineLine {
        let mut builder = formatter.begin_inline_session(
            spacing,
            self.active_mdoc_section() == MdocSectionContext::Authors,
            if nodes.before.is_empty() {
                author_break_effect
            } else {
                inline::AuthorBreakEffect::Line
            },
        );
        let operand_capture = std::rc::Rc::new(RefCell::new(inline::HeadOperandCapture::default()));
        builder.head_operand_capture = Some(operand_capture.clone());
        if preserve_rows {
            builder.begin_definition_head_consumption();
        }
        builder.scope_posts = self.scope_posts.clone();
        // The HEAD can execute .nf after this session begins. Each entered
        // NODE_LINE/NODE_NOFILL pair, not the inbound fill-mode snapshot,
        // decides whether to end the current formatter row.
        builder.observe_no_fill_source_lines(true);
        if !nodes.before.is_empty() {
            // blk_full() can emit direct TEXT before allocating It HEAD.
            // print_mdoc_node() visits it before termp_it_pre(HEAD) selects
            // NOSPACE, the field flags, margin, or diagnostic font.
            inline::append_inline_nodes(&mut builder, nodes.before, self.default_name);
        }
        if let Some(origin) = nodes.source_origin_units {
            builder.execution.enter_definition_head_origin(origin);
        }
        builder.enter_definition_head_effect(author_break_effect);
        if preserve_rows && let Some(units) = native_head_field_units {
            builder.set_definition_native_margin(units);
        }
        inline::append_inline_nodes(&mut builder, nodes.content, self.default_name);
        builder.observe_no_fill_source_lines(false);
        let finished = formatter.finish_inline_line_with_rows(builder, preserve_rows);
        self.native_heads.borrow_mut().capture_operands(
            nodes.content.as_ptr() as usize,
            operand_capture.borrow_mut().take(),
        );
        if preserve_rows && self.macro_set == MacroSet::Mdoc {
            // This path is the actual detached It HEAD post, whose native
            // flush above precedes clearing NOBREAK/BRTRSP/BRIND/HANG and
            // trailspace (mdoc_term.c::termp_it_post(), 936-963). Keep that
            // node lifecycle separate from generic output-helper returns.
            formatter.execution.clear_native_list_part_flags();
        }
        finished
    }

    /// Lower a definition head without executing a formatter line boundary.
    ///
    /// mdoc inset and diagnostic bodies remain in the same native formatter
    /// stream as their heads.  The returned state owns pending `\z`, `\p`,
    /// and source-continuation effects until the generated run-in cells and
    /// first body word execute.
    pub(super) fn lower_run_in_definition_head<'n>(
        &self,
        groups: impl IntoIterator<Item = HeadNodes<'n>>,
        spacing: bool,
        formatter: &mut crate::mandoc::formatter::FormatterState,
        strong_scope: bool,
        generated_cells: usize,
        author_break_effect: inline::AuthorBreakEffect,
    ) -> RunInDefinitionHead {
        let mut builder = formatter.begin_inline_session(
            spacing,
            self.active_mdoc_section() == MdocSectionContext::Authors,
            author_break_effect,
        );
        let operand_capture = std::rc::Rc::new(RefCell::new(inline::HeadOperandCapture::default()));
        builder.head_operand_capture = Some(operand_capture.clone());
        let mut operand_key = None;
        builder.begin_definition_head_consumption();
        builder.scope_posts = self.scope_posts.clone();
        builder.observe_no_fill_source_lines(true);
        let mut saved_font = None;
        for nodes in groups {
            if !nodes.content.is_empty() {
                operand_key.get_or_insert(nodes.content.as_ptr() as usize);
            }
            if !nodes.before.is_empty() {
                // Direct It siblings are outside the diagnostic HEAD font
                // and NOBREAK field, but remain in its live input buffer.
                builder.enter_definition_head_effect(inline::AuthorBreakEffect::Line);
                inline::append_inline_nodes(&mut builder, nodes.before, self.default_name);
            }
            if let Some(origin) = nodes.source_origin_units {
                builder.execution.enter_definition_head_origin(origin);
            }
            builder.enter_definition_head_effect(author_break_effect);
            if strong_scope && saved_font.is_none() {
                saved_font = Some(
                    builder
                        .font
                        .push_scope(super::roff_escape::RoffFont::Strong),
                );
            }
            inline::append_inline_nodes(&mut builder, nodes.content, self.default_name);
        }
        builder.observe_no_fill_source_lines(false);
        // The generated cells execute FIRST (append_run_in_cells routes
        // them through the zero-advance projection): a pending HEAD glyph
        // is overstruck by the first generated cell (encode1 consumes
        // BACKBEFORE, term.c:901-908), so no cell is deducted and no glyph
        // is settled before the run-in word runs.
        let surviving_cells = generated_cells;
        builder.project_definition_owner_prefix();
        // The It HEAD post runs here (mdoc_term.c:961-962): field flags and
        // trailspace clear before any BODY word executes, while the run-in
        // kinds keep the shared input buffer (939-945).
        builder.note_definition_head_flags_cleared();
        builder.settle_provisional_definition_break();
        if let Some(saved_font) = saved_font {
            builder.font.pop_scope(saved_font);
        }
        let (mut output, execution) = formatter.finish_inline_scope(builder);
        let breaks = inline::take_definition_term_breaks(&mut output);
        if let Some(key) = operand_key {
            self.native_heads
                .borrow_mut()
                .capture_operands(key, operand_capture.borrow_mut().take());
        }
        RunInDefinitionHead {
            output,
            execution,
            term_breaks: breaks,
            surviving_cells,
            generated_word: generated_cells > 0,
        }
    }

    /// Execute a section heading in the surrounding formatter stream.
    ///
    /// CVS renders `Sh`/`Ss` heads as a scoped bold font, then calls
    /// `term_newln()`.  The semantic heading supplies its own presentation,
    /// so inherited font state is deliberately not projected into its IR;
    /// spacing and zero-advance execution remain document-global.
    pub(super) fn lower_section_heading(
        &self,
        nodes: HeadNodes<'_>,
        formatter: &mut formatter::FormatterState,
        authors_section: bool,
        previous_output: &mut Vec<mant_ir::Inline>,
    ) -> (Vec<mant_ir::Inline>, u16) {
        let shared = !previous_output.is_empty();
        let mut builder = formatter.begin_inline_session_on(
            previous_output,
            formatter.spacing_enabled(),
            authors_section,
            inline::AuthorBreakEffect::Line,
        );
        if shared {
            builder.mark_section_handoff();
        }
        builder.scope_posts = self.scope_posts.clone();
        inline::append_inline_nodes(&mut builder, nodes.before, self.default_name);
        match self.macro_set {
            MacroSet::Man | MacroSet::None => {
                builder.font.begin_man_heading();
                inline::append_inline_nodes(&mut builder, nodes.content, self.default_name);
                builder.font.end_man_heading();
            }
            MacroSet::Mdoc => {
                let heading_font = builder.font.push_heading_scope();
                inline::append_inline_nodes(&mut builder, nodes.content, self.default_name);
                builder.font.pop_heading_scope(heading_font);
            }
        }
        // mdoc_term.c::termp_sh_post()/termp_ss_post() execute term_newln
        // at the HEAD post. Consume the live native receipt before draining
        // this semantic owner; a rejected later pass still ended its row.
        builder.hard_break();
        if shared {
            *previous_output = builder.take_section_handoff_prefix();
        }
        let completed_empty_rows = builder.completed_empty_rows();
        let mut output = formatter.finish_inline_line_with_rows(builder, true).output;
        // The heading consumer supplies the post's final line ending. Only
        // that one terminator is redundant; earlier executed empty rows are
        // part of the heading and must not be trimmed with it. If no glyph
        // survived a native rejection, project the completed rows as
        // spacing at this owner rather than opening an extra inline row.
        let blank_heading_rows =
            if completed_empty_rows > 0 && !inline::has_rendered_formatter_glyph(&output) {
                inline::retain_inline_identities(&mut output);
                completed_empty_rows
            } else {
                inline::consume_one_row_ending(&mut output);
                0
            };
        (remove_structural_heading_bold(output), blank_heading_rows)
    }

    pub(super) fn lower_text(
        &self,
        source: &str,
        formatter: &mut formatter::FormatterState,
    ) -> Vec<mant_ir::Inline> {
        let mut zero_advance = inline::ZeroAdvanceState::new();
        zero_advance.annotations = Some(self.annotations.clone());
        zero_advance.inherit_armed(formatter.take_zero_advance_armed());
        let record_native_cells = formatter.execution.table_buffer_written.is_some();
        let execution = inline::parse_roff_text_with_zero_advance(
            source,
            inline::TextExecutionContext {
                font: &mut formatter.font,
                zero_advance: &mut zero_advance,
                defer_leading_blanks: false,
                policy: inline::TextExecutionPolicy {
                    recognize_generated_references: self.macro_set == MacroSet::Mdoc,
                    record_native_cells,
                    field_authoritative: false,
                    word_space: crate::mandoc::roff_escape::WordSpace::Breakable,
                },
            },
        );
        formatter
            .execution
            .escape_coverage
            .record(execution.escape_scan);
        formatter
            .execution
            .observe_table_word_writes(&execution.native_writes, false);
        if let Some(buffered) = &mut formatter.execution.table_buffer_written {
            *buffered |= execution.table_device_buffer_written;
            formatter.execution.table_line_needs_width |= execution.table_line_needs_width;
        }
        let mut output = execution.output;
        if execution.pending_word_end_break {
            output.push(mant_ir::Inline::line_break());
        }
        // Even a control-only tbl word enters term_word(): it clears
        // formatter-global skipvsp and can leave a bare BACKAFTER request for
        // the next cell-external word.
        formatter.execute_word();
        formatter.inherit_zero_advance_armed(zero_advance.take_armed());
        zero_advance.finish_into(&mut output);
        output
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
fn remove_structural_heading_bold(nodes: Vec<mant_ir::Inline>) -> Vec<mant_ir::Inline> {
    let mut output = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            mant_ir::Inline::Strong { children } => {
                output.extend(remove_structural_heading_bold(children));
            }
            mant_ir::Inline::Emphasis { children } => output.push(mant_ir::Inline::Emphasis {
                children: remove_structural_heading_bold(children),
            }),
            mant_ir::Inline::Link {
                target,
                title,
                children,
            } => output.push(mant_ir::Inline::Link {
                target,
                title,
                children: remove_structural_heading_bold(children),
            }),
            node => output.push(node),
        }
    }
    output
}
