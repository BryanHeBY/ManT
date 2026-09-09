//! Reconstructs sections and semantic blocks from the copied mandoc tree.

use libmandoc_rs::{AuthorMode, DisplayKind, Node, NodeKind};
use mant_ir::{Block, Inline, Section};

use super::{
    LoweringContext, first_part_children,
    inline::{
        FilledBoundary, FontState, InlineBuilder, append_inline_node_with_next, is_enclosure_macro,
        lower_inline_nodes, lower_inline_nodes_with_font_state, lower_inline_nodes_with_spacing,
        lower_man_link, plain_text, updated_spacing,
    },
    layout::{
        add_leading_spacing, layout, layout_with_spacing, section_spacing, set_block_spacing,
        update_paragraph_distance, vertical_distance_lines,
    },
    part_child_groups,
    roff_escape::visible_text,
    source_span, targets,
};

mod sections;
use sections::is_section;
pub(super) use sections::{lower_root_blocks, lower_sections};
mod structural;
use structural::StructuralLowerer;
mod synopsis;
use synopsis::{SynopsisDeclarationRole, lower_synopsis_head, mdoc_synopsis_declaration_role};
mod man_nofill;
use man_nofill::lower_no_fill_lines;
mod flow;
use flow::BlockState;
mod lists;
mod preformatted;
mod tables;

use lists::man::ordered::{ManListState, append_relative_continuation};
use lists::{
    ManDefinitionState, lower_man_definition as lower_man_definition_block, lower_mdoc_list,
};
use preformatted::preformatted_blocks;
use tables::{TableEmbedding, TableEmbeddingPlan, append_table_row};

fn lower_blocks(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
) -> Vec<Block> {
    let mut formatter = crate::mandoc::formatter::FormatterState::default();
    lower_blocks_with_spacing(
        nodes,
        context,
        indent_columns,
        paragraph_distance,
        true,
        &mut formatter,
    )
}

fn lower_blocks_with_spacing(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    spacing_enabled: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Block> {
    lower_blocks_with_predecessor(
        nodes,
        context,
        indent_columns,
        paragraph_distance,
        spacing_enabled,
        false,
        formatter,
    )
}

/// Lower a detached structural body without discarding its predecessor.
/// Native display spacing walks through first-child containers to find an
/// earlier source sibling. An empty child output buffer is not evidence that
/// the body immediately follows a section heading.
fn lower_blocks_with_predecessor(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    spacing_enabled: bool,
    paragraph_predecessor: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Block> {
    let mut lowerer = BlockLowerer::new(
        context,
        indent_columns,
        paragraph_distance,
        spacing_enabled,
        Vec::new(),
        *formatter,
    );
    lowerer.paragraph_predecessor = paragraph_predecessor;
    lowerer.push_nodes(nodes);
    lowerer.formatter.spacing = lowerer.state.spacing_enabled();
    *formatter = lowerer.formatter;
    lowerer.finish()
}

const DEFAULT_MAN_TAG_WIDTH: i32 = 7;

struct BlockLowerer<'a, 'source> {
    context: &'a LoweringContext<'source>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &'a mut u16,
    state: BlockState,
    formatter: crate::mandoc::formatter::FormatterState,
    // man(7) starts each section or relative-indent scope with a seven-column
    // hanging margin. Explicit `.TP`/`.IP` widths update it for following
    // tagged paragraphs, exactly as mandoc's terminal renderer does.
    definition_hanging_width: crate::mandoc::layout::Distance,
    split_authors: bool,
    synopsis_return_type_open: bool,
    // Source-proven `.IP`/`.TP` ordinals form lists immediately; this state
    // joins only adjacent, consecutively numbered items of the same style.
    man_list_state: ManListState,
    // Transparent `.RS` scopes retain their native predecessor even when
    // their IR is collected separately for attachment to an ordered item.
    paragraph_predecessor: bool,
}

impl<'a, 'source> BlockLowerer<'a, 'source> {
    fn new(
        context: &'a LoweringContext<'source>,
        indent_columns: crate::mandoc::layout::SourceIndent,
        paragraph_distance: &'a mut u16,
        spacing_enabled: bool,
        output: Vec<Block>,
        formatter: crate::mandoc::formatter::FormatterState,
    ) -> Self {
        Self {
            context,
            indent_columns,
            paragraph_distance,
            state: BlockState::with_output(indent_columns, spacing_enabled, output),
            formatter,
            definition_hanging_width: crate::mandoc::layout::Distance::cells(DEFAULT_MAN_TAG_WIDTH),
            split_authors: false,
            synopsis_return_type_open: false,
            man_list_state: ManListState::None,
            paragraph_predecessor: false,
        }
    }

    fn push_nodes(&mut self, nodes: &[Node]) {
        let table_plan = TableEmbeddingPlan::new(nodes, self.context);
        for (index, node) in nodes.iter().enumerate() {
            if table_plan.consumes(index) || is_inline_equation_quote_artifact(nodes, index) {
                continue;
            }
            if follows_inline_equation_punctuation(nodes, index) {
                self.state.tighten_next_boundary();
            }
            self.push(node, nodes.get(index + 1), table_plan.embedding(index));
            if self.context.macro_set == libmandoc_rs::MacroSet::Mdoc
                && super::adjacency::is_logical_sibling(node)
            {
                self.paragraph_predecessor = true;
            }
        }
    }

    fn push(
        &mut self,
        node: &Node,
        next: Option<&Node>,
        table_embedding: Option<&TableEmbedding<'_>>,
    ) {
        if matches!(
            node.macro_name.as_deref(),
            Some("PP" | "HP" | "IP" | "TP" | "TQ" | "RS" | "SY")
        ) {
            self.formatter.font = FontState::new();
        }
        if node.macro_name.as_deref() == Some("ft") {
            lower_inline_nodes_with_font_state(
                std::slice::from_ref(node),
                self.context.default_name,
                self.state.spacing_enabled(),
                &mut self.formatter.font,
            );
            return;
        }
        if self.push_container(node) || self.consume_control_or_empty_block(node) {
            return;
        }
        if self.push_no_fill_synopsis(node) {
            return;
        }
        let structural_targets = targets::structural_targets(node);
        if self.push_executed_spacing(node) {
            self.state
                .queue_targets(structural_targets, source_span(node));
            return;
        }
        if self.push_no_fill_lines(node) {
            self.state
                .queue_targets(structural_targets, source_span(node));
            return;
        }
        self.state.flush_preformatted();
        if self.push_mdoc_synopsis_declaration(node) {
            return;
        }
        if node.flags.delimiter_close
            && participates_in_inline_flow(node)
            && self.state.paragraph.is_empty()
        {
            let tail = lower_inline_nodes(std::slice::from_ref(node), self.context.default_name);
            if append_to_last_inline_block(&mut self.state.output, &tail) {
                return;
            }
        }
        if node.macro_name.as_deref() == Some("Pp") {
            self.state.flush_paragraph();
            self.state
                .queue_targets(structural_targets, source_span(node));
            if !self.state.output.is_empty() {
                self.state.output.push(Block::VerticalSpace {
                    lines: 1,
                    source: source_span(node),
                });
            }
        } else if node.macro_name.as_deref() == Some("br") {
            self.state.hard_break();
        } else if matches!(node.macro_name.as_deref(), Some("UR" | "MT")) {
            let spacing_enabled = self.state.spacing_enabled();
            push_man_link(
                &mut self.state,
                node,
                self.context.default_name,
                spacing_enabled,
            );
        } else if participates_in_inline_flow(node) {
            self.push_inline_node(node, next);
        } else {
            self.state.flush_paragraph();
            let output_start = self.state.output.len();
            let spacing_enabled = self.state.spacing_enabled();
            StructuralLowerer {
                context: self.context,
                indent_columns: if restores_macro_indent(node) {
                    self.indent_columns.macro_origin()
                } else {
                    self.state.source_indent()
                },
                paragraph_distance: self.paragraph_distance,
                output: &mut self.state.output,
                paragraph_predecessor: self.paragraph_predecessor,
                definition_hanging_width: &mut self.definition_hanging_width,
                man_list_state: &mut self.man_list_state,
                spacing_enabled,
                formatter: &mut self.formatter,
            }
            .push(node, table_embedding);
            if restores_macro_indent(node) {
                self.state
                    .set_source_indent(self.indent_columns.macro_origin());
            }
            self.state.inherit_spacing(self.formatter.spacing);
            self.state
                .queue_targets(structural_targets, source_span(node));
            self.state.attach_pending_to_structural_output(output_start);
        }
    }

    fn push_container(&mut self, node: &Node) -> bool {
        if !super::containers::is_container(node) {
            return false;
        }
        if node.macro_name.as_deref() != Some("Bf")
            && !super::containers::has_structural_payload(node)
        {
            return false;
        }
        let mut saved_font = None;
        let mut started = false;
        let handled = super::containers::walk(node, |event| {
            use super::containers::Event;
            if !started {
                self.state
                    .queue_targets(targets::structural_targets(node), source_span(node));
                started = true;
            }
            match event {
                // In filled structural flow input-line wrappers alone are
                // not paragraph breaks. Literal DisplayFlow consumes them.
                Event::BeginNode(_) => {}
                Event::Break => {
                    self.state.flush_preformatted();
                    self.state.flush_paragraph();
                    self.state.consume_hanging_first_line();
                }
                Event::FlushLine => self.state.flush_requested_line(source_span(node)),
                Event::Children(nodes) => self.push_nodes(nodes),
                Event::EnterFont(font) => saved_font = Some(self.formatter.font.push_scope(font)),
                Event::ExitFont => {
                    if let Some(saved) = saved_font.take() {
                        self.formatter.font.pop_scope(saved);
                    }
                }
                event => self
                    .state
                    .push_inline_with(source_span(node), false, false, |builder| {
                        builder.font = self.formatter.font;
                        match event {
                            Event::Glyph(value) => builder.append_text(&value),
                            Event::Tight => builder.tighten_next_boundary(),
                            Event::Release => builder.release_next_boundary(),
                            Event::EmptyWord => builder.append_word(Vec::new()),
                            _ => unreachable!("container children and font scopes handled above"),
                        }
                    }),
            }
        });
        if !handled {
            return false;
        }
        true
    }

    /// Consume execution requests before the no-fill word fallback can
    /// mistake numeric operands for printable text.
    fn push_executed_spacing(&mut self, node: &Node) -> bool {
        let space = node.macro_name.as_deref() == Some("sp");
        if !(space || node.flags.no_fill && node.macro_name.as_deref() == Some("br")) {
            return false;
        }
        self.state.flush_preformatted();
        self.state.flush_paragraph();
        self.state.consume_hanging_first_line();
        if space && let Some(lines) = vertical_distance_lines(node) {
            self.state.output.push(Block::VerticalSpace {
                lines,
                source: source_span(node),
            });
        }
        true
    }

    fn consume_control_or_empty_block(&mut self, node: &Node) -> bool {
        if node.macro_name.as_deref() == Some("in")
            && self.context.macro_set == libmandoc_rs::MacroSet::Man
        {
            self.state.flush_preformatted();
            self.state.flush_paragraph();
            let current = self.state.source_indent();
            let next = node
                .children
                .first()
                .and_then(|node| node.text.as_deref())
                .map_or(self.indent_columns.macro_origin(), |argument| {
                    let Some(distance) = self.context.checked_distance(node, argument) else {
                        return current;
                    };
                    if argument.starts_with(['+', '-']) {
                        self.context.offset_indent(node, current, distance)
                    } else {
                        current.absolute(distance)
                    }
                });
            self.state.set_source_indent(next);
            return true;
        }
        if consume_block_control(
            node,
            self.context,
            &mut self.state,
            self.paragraph_distance,
            &mut self.split_authors,
        ) || node.flags.no_print
            || node.kind == NodeKind::Comment
            || is_section(node, false)
            || super::controls::operand_control(node.macro_name.as_deref()).is_some()
        {
            return true;
        }
        // A configuration-only `.EQ delim XX .EN` produces an empty equation
        // syntax node. It changes parser state but owns no printable block.
        if node.kind == NodeKind::Equation
            && node
                .equation
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return true;
        }
        if node.kind == NodeKind::Text
            && node.text.as_deref().is_some_and(str::is_empty)
            && !node.flags.no_fill
        {
            self.state.flush_paragraph();
            self.state.output.push(Block::VerticalSpace {
                lines: 1,
                source: source_span(node),
            });
            return true;
        }
        false
    }

    fn finish(self) -> Vec<Block> {
        let blocks = self.state.finish();
        self.context.check_gap_bounds(&blocks);
        blocks
    }

    fn push_no_fill_lines(&mut self, node: &Node) -> bool {
        let Some(lines) =
            lower_no_fill_lines(node, self.context.default_name, &mut self.formatter.font)
        else {
            return false;
        };
        for line in lines {
            self.state.push_preformatted(
                line.nodes,
                line.source,
                line.continues_line,
                line.starts_line,
                line.occupies_row,
            );
        }
        true
    }

    fn push_no_fill_synopsis(&mut self, node: &Node) -> bool {
        let body = first_part_children(node, NodeKind::Body);
        if node.macro_name.as_deref() != Some("SY") || !body.iter().any(|child| child.flags.no_fill)
        {
            return false;
        }
        self.state.flush_paragraph();
        self.state.flush_preformatted();
        self.state
            .queue_targets(targets::structural_targets(node), source_span(node));
        let head = first_part_children(node, NodeKind::Head);
        let saved = self
            .formatter
            .font
            .push_scope(crate::mandoc::roff_escape::RoffFont::Strong);
        let nodes = lower_inline_nodes_with_font_state(
            head,
            self.context.default_name,
            self.state.spacing_enabled(),
            &mut self.formatter.font,
        );
        self.formatter.font.pop_scope(saved);
        if !nodes.is_empty() {
            self.state.push_preformatted(
                nodes,
                source_span(node),
                head.last().is_some_and(ends_with_line_continuation),
                true,
                true,
            );
        }
        // SY is a scope, not a promise that its whole body is no-fill.
        // Execute each child through normal block dispatch so fi/nf, spacing
        // and structural children cannot become flattened pseudo-text.
        self.push_nodes(body);
        self.state.flush_paragraph();
        self.state.flush_preformatted();
        self.formatter.font = FontState::new();
        true
    }

    fn push_inline_node(&mut self, node: &Node, next: Option<&Node>) {
        let source = source_span(node);
        self.state.push_source_inline_with(
            source,
            starts_indented_filled_line(node),
            ends_with_line_continuation(node),
            node.kind == NodeKind::Text && node.flags.line_start,
            |builder| {
                builder.font = self.formatter.font;
                append_inline_node_with_next(builder, node, next, self.context.default_name);
                self.formatter.font = builder.font;
                self.formatter.spacing = builder.spacing_enabled();
            },
        );
        self.state.inherit_spacing(self.formatter.spacing);
    }

    /// Preserve declaration boundaries selected by mdoc's SYNOPSIS grammar.
    ///
    /// libmandoc marks the declaration-oriented `Fd`, `In`, `Ft`, `Fn`,
    /// `Fo`, and `Vt` nodes with `synopsis_pretty`.  Their terminal layout is
    /// intentionally richer than `ManT`'s IR, but the declaration boundary is
    /// semantic: an include directive must not merge into the next return
    /// type, and each function declaration must remain independently
    /// addressable.  A return type and the immediately following function
    /// macro form one useful IR paragraph; all other marked declarations own
    /// a paragraph of their own.
    fn push_mdoc_synopsis_declaration(&mut self, node: &Node) -> bool {
        let Some(role) = mdoc_synopsis_declaration_role(node) else {
            if self.synopsis_return_type_open {
                self.state.flush_paragraph();
                self.synopsis_return_type_open = false;
            }
            return false;
        };

        match role {
            SynopsisDeclarationRole::ReturnType => {
                self.state.flush_paragraph();
                self.push_inline_node(node, None);
                self.synopsis_return_type_open = true;
            }
            SynopsisDeclarationRole::Function => {
                if !self.synopsis_return_type_open {
                    self.state.flush_paragraph();
                }
                self.push_inline_node(node, None);
                self.state.flush_paragraph();
                self.synopsis_return_type_open = false;
            }
            SynopsisDeclarationRole::Standalone => {
                self.state.flush_paragraph();
                self.push_inline_node(node, None);
                self.state.flush_paragraph();
                self.synopsis_return_type_open = false;
            }
        }
        true
    }
}

/// Consume state-only roff requests before printable block lowering.
fn consume_block_control(
    node: &Node,
    context: &LoweringContext<'_>,
    state: &mut BlockState,
    paragraph_distance: &mut u16,
    split_authors: &mut bool,
) -> bool {
    match node.macro_name.as_deref() {
        Some("PD") => update_paragraph_distance(node, paragraph_distance),
        Some("nf" | "fi" | "EX" | "EE") if context.macro_set == libmandoc_rs::MacroSet::Man => {
            // roff_term_pre_br and man pre_literal end the current line and
            // switch HP from its temporary first-line origin to its permanent
            // body origin, including when no text preceded the request.
            state.literal_mode_boundary();
        }
        Some("An") => match node.author_mode {
            Some(AuthorMode::Split) => *split_authors = true,
            Some(AuthorMode::NoSplit) => *split_authors = false,
            None if *split_authors => {
                state.hard_break();
                return false;
            }
            None => return false,
        },
        Some("Sm") => {
            let setting = plain_text(&lower_inline_nodes(&node.children, context.default_name));
            state.set_spacing(setting.trim());
        }
        _ => return false,
    }
    true
}

/// Keep man-ext links inside the surrounding filled flow.
fn push_man_link(
    state: &mut BlockState,
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
) {
    state.push_inline(
        lower_man_link(node, default_name, spacing_enabled),
        source_span(node),
        starts_indented_filled_line(node),
        ends_with_line_continuation(node),
    );
}

/// Lower one source-level no-fill line owner without letting structural macros
/// split the surrounding preformatted flow into ordinary paragraphs.
///
/// Most no-fill input arrives as text or inline elements. GNU man-ext also
/// permits a complete `.SY` block inside `.EX`; its printable head and body
/// still represent adjacent source lines and must join the same verbatim block.
fn equation_block(node: &Node, indent_columns: crate::mandoc::layout::SourceIndent) -> Block {
    Block::Equation {
        // Equation boxes carry the same named-character escapes as ordinary
        // roff text, but they bypass inline-node lowering. Decode them here so
        // values such as `\[*p]` and `\[mi]` cannot leak into every output
        // projection.
        value: visible_text(node.equation.as_deref().unwrap_or_default()),
        display: true,
        layout: layout(indent_columns),
        source: source_span(node),
    }
}

fn is_inline_equation(node: &Node) -> bool {
    node.kind == NodeKind::Equation
        && !node.flags.line_start
        && node
            .equation
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
}

/// libmandoc terminates a quoted man-macro argument containing inline eqn by
/// moving the equation beside the macro and retaining the closing source quote
/// as a `\&"` text sibling. The quote is parser scaffolding, not output.
fn is_inline_equation_quote_artifact(nodes: &[Node], index: usize) -> bool {
    let Some(node) = nodes.get(index) else {
        return false;
    };
    let Some(previous) = index.checked_sub(1).and_then(|index| nodes.get(index)) else {
        return false;
    };
    is_inline_equation(previous)
        && previous.line == node.line
        && node.kind == NodeKind::Text
        && node
            .text
            .as_deref()
            .is_some_and(|text| visible_text(text).trim() == "\"")
}

fn follows_inline_equation_punctuation(nodes: &[Node], index: usize) -> bool {
    let Some(node) = nodes.get(index) else {
        return false;
    };
    let Some(previous) = index.checked_sub(1).and_then(|index| nodes.get(index)) else {
        return false;
    };
    is_inline_equation(previous)
        && previous.line == node.line
        && node.kind == NodeKind::Text
        && node
            .text
            .as_deref()
            .map(visible_text)
            .and_then(|text| text.chars().next())
            .is_some_and(|character| matches!(character, '.' | ',' | ':' | ';' | '!' | '?'))
}

/// Join a synopsis command head to the body that libmandoc groups beneath it.
///
/// Both man(7) `.SY command` and an mdoc(7) `.Nm` in SYNOPSIS become
/// structural blocks. Their printable command lives in the head while all
/// following options live in the body. The generic structural fallback keeps
/// only the body, which would silently turn a command synopsis into a bare
/// option list.
fn append_to_last_inline_block(blocks: &mut [Block], tail: &[Inline]) -> bool {
    for block in blocks.iter_mut().rev() {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                children.extend_from_slice(tail);
                return true;
            }
            Block::List { items, .. } => {
                if items
                    .last_mut()
                    .is_some_and(|item| append_to_last_inline_block(&mut item.blocks, tail))
                {
                    return true;
                }
            }
            Block::DefinitionList { items, .. } => {
                if items.last_mut().is_some_and(|item| {
                    append_to_last_inline_block(&mut item.description, tail)
                        || item.terms.last_mut().is_some_and(|term| {
                            term.extend_from_slice(tail);
                            true
                        })
                }) {
                    return true;
                }
            }
            Block::Table { rows, .. } => {
                if rows
                    .last_mut()
                    .and_then(|row| row.cells.last_mut())
                    .is_some_and(|cell| append_to_last_inline_block(&mut cell.blocks, tail))
                {
                    return true;
                }
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
    false
}

/// Match the filled-text line-break rule used by libmandoc's terminal and
/// HTML renderers: a text node beginning an input line with whitespace starts
/// a new output line.  The first printable text can sit below an inline macro
/// wrapper, so inspect the semantic subtree rather than only direct text
/// siblings.
fn starts_indented_filled_line(node: &Node) -> bool {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        return false;
    }
    if node.kind == NodeKind::Text {
        return node.flags.line_start
            && node
                .text
                .as_deref()
                .is_some_and(|text| text.starts_with(char::is_whitespace));
    }
    node.children
        .iter()
        .find(|child| !child.flags.no_print && child.kind != NodeKind::Comment)
        .is_some_and(starts_indented_filled_line)
}

/// Whether the final printable fragment in this syntax subtree ends with the
/// roff `\c` escape. The parser retains this as source-boundary semantics so
/// filled and no-fill flows can make the same join decision.
pub(super) fn ends_with_line_continuation(node: &Node) -> bool {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        return false;
    }
    if node.kind == NodeKind::Text {
        return node.flags.line_continuation;
    }
    node.children
        .iter()
        .rev()
        .find(|child| !child.flags.no_print && child.kind != NodeKind::Comment)
        .is_some_and(ends_with_line_continuation)
}

/// Attach a macro's leading distance to its first visible nested block.
fn extend_blocks_with_spacing(output: &mut Vec<Block>, mut nested: Vec<Block>, lines: u16) {
    add_leading_spacing(&mut nested, lines);
    output.extend(nested);
}

/// Whether a parsed node contributes to the current filled inline flow.
///
/// AST block shape is not itself a paragraph boundary. Fo is an inline
/// function scope in prose; the earlier synopsis declaration policy owns its
/// structural breaks. Nm retains its separate head/body handling.
fn participates_in_inline_flow(node: &Node) -> bool {
    matches!(node.kind, NodeKind::Text | NodeKind::Element)
        || is_inline_equation(node)
        || is_enclosure_macro(node.macro_name.as_deref())
        || matches!(node.macro_name.as_deref(), Some("Nd" | "Fo"))
}

/// These man macros explicitly assign the formatter's macro base. Passive
/// structures such as tables and equations inherit the current `.in` position.
fn restores_macro_indent(node: &Node) -> bool {
    matches!(
        node.macro_name.as_deref(),
        Some("PP" | "P" | "LP" | "HP" | "TP" | "TQ" | "IP" | "RS" | "SY")
    )
}

#[cfg(test)]
mod tests {
    use mant_ir::{Block, Inline, SourceSpan};

    use super::{BlockState, layout, plain_text};

    fn text(value: &str) -> Vec<Inline> {
        vec![Inline::Text {
            value: value.to_owned(),
        }]
    }

    const fn source(line: u32) -> SourceSpan {
        SourceSpan {
            byte_range: None,
            line,
            column: 1,
            end_line: None,
            end_column: None,
        }
    }

    #[test]
    fn block_state_preserves_filled_line_boundaries_and_continuations() {
        let mut state = BlockState::with_output(3.into(), true, Vec::new());
        state.push_inline(text("alpha"), Some(source(1)), false, false);
        state.push_inline(text("beta"), Some(source(2)), false, false);
        state.push_inline(text("gamma"), Some(source(3)), true, false);
        state.push_inline(text("delta"), Some(source(4)), false, true);
        state.push_inline(text("epsilon"), Some(source(5)), false, false);

        let output = state.finish();
        let [
            Block::Paragraph {
                children,
                layout: paragraph_layout,
                source: paragraph_source,
            },
        ] = output.as_slice()
        else {
            panic!("expected one filled paragraph, got {output:?}");
        };
        assert_eq!(plain_text(children), "alpha beta\ngamma deltaepsilon");
        assert_eq!(
            children
                .iter()
                .filter(|inline| matches!(inline, Inline::LineBreak))
                .count(),
            1
        );
        assert_eq!(*paragraph_layout, layout(3.into()));
        assert_eq!(*paragraph_source, Some(source(1)));
    }

    #[test]
    fn block_state_flushes_paragraph_before_tight_preformatted_lines() {
        let mut state = BlockState::with_output(2.into(), true, Vec::new());
        state.push_inline(text("prose"), Some(source(1)), false, false);
        state.push_preformatted(text("first"), Some(source(3)), false, true, true);
        state.push_preformatted(text("second"), Some(source(4)), true, true, true);
        state.push_preformatted(text("third"), Some(source(5)), false, true, true);

        let output = state.finish();
        let [
            Block::Paragraph {
                children: paragraph,
                layout: paragraph_layout,
                source: paragraph_source,
            },
            Block::Preformatted {
                children: preformatted,
                language,
                layout: preformatted_layout,
                source: preformatted_source,
            },
        ] = output.as_slice()
        else {
            panic!("expected prose followed by one preformatted block, got {output:?}");
        };
        assert_eq!(plain_text(paragraph), "prose");
        assert_eq!(plain_text(preformatted), "first\nsecondthird");
        assert_eq!(
            preformatted
                .iter()
                .filter(|inline| matches!(inline, Inline::LineBreak))
                .count(),
            1
        );
        assert_eq!(*paragraph_layout, layout(2.into()));
        assert_eq!(*preformatted_layout, layout(2.into()));
        assert_eq!(*paragraph_source, Some(source(1)));
        assert_eq!(*preformatted_source, Some(source(3)));
        assert_eq!(*language, None);
    }
}
