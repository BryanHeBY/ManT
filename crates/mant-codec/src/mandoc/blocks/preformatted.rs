//! Preserves no-fill and literal display content as preformatted blocks.

use libmandoc_rs::{Node, NodeKind};
use mant_ir::{Block, Inline};

use super::super::{
    LoweringContext,
    inline::{InlineBuilder, append_inline_node_with_next},
    layout::{layout, vertical_space_delta},
    source_span,
};
use super::tables::{TableEmbeddingPlan, append_table_row};

pub(super) fn preformatted_blocks(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    spacing_enabled: bool,
    paragraph_predecessor: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Block> {
    let inbound_no_fill = formatter.no_fill;
    let mut flow = DisplayFlow {
        output: Vec::new(),
        line: InlineBuilder::with_spacing(spacing_enabled),
        source: None,
        context,
        indent_columns,
        paragraph_predecessor,
        literal: true,
        honor_node_fill: node.macro_name.as_deref() == Some("Bd"),
        formatter: std::mem::take(formatter),
    };
    flow.line.font = flow.formatter.font;
    flow.line.scope_posts = context.scope_posts.clone();
    flow.line.inherit_author_execution(
        flow.formatter.author_flow(),
        context.active_mdoc_section() == crate::mandoc::source_context::MdocSectionContext::Authors,
    );
    flow.line
        .inherit_vertical_space_debt(flow.formatter.vertical_space_debt);
    flow.line
        .inherit_zero_advance_armed(std::mem::take(&mut flow.formatter.zero_advance_armed));
    flow.line.track_executed_lines();
    let body_index = node
        .children
        .iter()
        .position(|child| child.kind == NodeKind::Body);
    let children = body_index.map_or(node.children.as_slice(), |index| {
        node.children[index].children.as_slice()
    });
    flow.append_nodes(children);
    if let Some(body_index) = body_index {
        for child in node.children[body_index + 1..]
            .iter()
            .take_while(|child| child.line == node.line && child.flags.delimiter_close)
        {
            flow.append_inline(child, None);
        }
    }
    flow.flush();
    flow.commit_line_execution();
    *formatter = flow.formatter;
    // CVS mdoc_macro.c::blk_exp_close() restores the fill mode saved before
    // Bd at `.Ed`, so requests inside the display cannot leak a literal
    // channel to the containing enclosure's generated post.
    formatter.no_fill = inbound_no_fill;
    flow.output
}

/// No-fill controls line geometry, not which AST payloads are reachable.
/// Transparent font scopes retain the same cursor; tables/lists re-enter the
/// structural lowerer after flushing the current inline run.
struct DisplayFlow<'a, 'source> {
    output: Vec<Block>,
    line: InlineBuilder,
    source: Option<mant_ir::SourceSpan>,
    context: &'a LoweringContext<'source>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_predecessor: bool,
    literal: bool,
    /// Bd uses roff fill mode. D1/Dl are single-line display constructs whose
    /// ordinary operands have no `NODE_NOFILL` flag but retain one display row.
    honor_node_fill: bool,
    formatter: crate::mandoc::formatter::FormatterState,
}

impl DisplayFlow<'_, '_> {
    /// A display line executes in the same formatter as the surrounding
    /// document.  CVS `mdoc_term.c::termp_an_pre` changes global SPLIT flags;
    /// `mdoc_term.c::termp_bd_pre/post` do not scope those flags to Bd.
    fn commit_line_execution(&mut self) {
        self.formatter.font = self.line.font;
        self.formatter.spacing = self.line.spacing_enabled();
        self.formatter.vertical_space_debt = self.line.vertical_space_debt();
        self.formatter.zero_advance_armed = self.line.take_zero_advance_armed();
        if let Some(author_flow) = self.line.author_flow() {
            self.formatter.set_author_flow(author_flow);
        }
    }

    fn resume_line_execution(&mut self) {
        self.line.font = self.formatter.font;
        self.line.inherit_spacing(self.formatter.spacing);
        self.line
            .inherit_vertical_space_debt(self.formatter.vertical_space_debt);
        self.line
            .inherit_zero_advance_armed(std::mem::take(&mut self.formatter.zero_advance_armed));
        self.line.inherit_author_execution(
            self.formatter.author_flow(),
            self.context.active_mdoc_section()
                == crate::mandoc::source_context::MdocSectionContext::Authors,
        );
        self.line.reset_source_cursor();
        self.source = None;
    }

    fn append_container(&mut self, node: &Node) -> bool {
        let mut saved_font = None;
        let mut started = false;
        crate::mandoc::containers::walk(node, &self.context.scope_posts, |event| {
            use crate::mandoc::containers::Event;
            if !started {
                self.line.begin_executed_node(node);
                for target in super::targets::structural_targets(node) {
                    self.line
                        .append(vec![Inline::anchor_at(target, source_span(node))]);
                }
                started = true;
            }
            match event {
                Event::At(_, _) => {}
                Event::BeginNode(part) => {
                    self.line.begin_executed_node(part);
                    if part.kind == NodeKind::Head
                        && part.macro_name.as_deref() == Some("Fo")
                        && let Some(target) = super::targets::raw_target(part)
                    {
                        self.line
                            .append(vec![Inline::anchor_at(target, source_span(part))]);
                    }
                }
                Event::Break => {
                    self.flush();
                    self.line.reset_source_cursor();
                }
                Event::FlushLine => {
                    let start = self.output.len();
                    self.flush();
                    if !super::flow::has_flushed_row(&self.output[start..]) {
                        self.output.push(Block::Preformatted {
                            children: vec![Inline::Text {
                                value: String::new(),
                            }],
                            language: None,
                            layout: layout(self.indent_columns),
                            source: source_span(node),
                        });
                    }
                    self.line.reset_source_cursor();
                }
                Event::Children(nodes) => self.append_nodes(nodes),
                Event::Glyph(value) => {
                    self.line.append_text(&value);
                }
                Event::Tight => self.line.tighten_next_boundary(),
                Event::Release => self.line.release_next_boundary(),
                Event::EmptyWord => self.line.execute_empty_word(),
                Event::EnterKeep => self.line.enter_keep_words(),
                Event::ExitKeep => self.line.exit_keep_words(),
                Event::EnterFont(font, body_id) => {
                    let saved = self.line.font.push_scope(font);
                    if let Some(body_id) = body_id {
                        self.context.scope_posts.enter_font(body_id, saved);
                    } else {
                        saved_font = Some(saved);
                    }
                }
                Event::FunctionArgument(argument, comma_after) => {
                    // This direct Fo BODY child bypasses append_nodes(); it
                    // still executes its own NODE_NOFILL mode after a crossed
                    // `.Ed`, just like print_mdoc_node() does before Fa pre.
                    if self.honor_node_fill && self.literal != argument.flags.no_fill {
                        self.set_literal_mode(argument.flags.no_fill);
                    }
                    crate::mandoc::inline::function_argument(
                        &mut self.line,
                        argument,
                        comma_after,
                        self.context.default_name,
                    );
                }
                Event::ExitFont(body_id) => {
                    let saved = body_id
                        .and_then(|body_id| self.context.scope_posts.exit_font(body_id))
                        .or_else(|| body_id.is_none().then(|| saved_font.take()).flatten());
                    if let Some(saved) = saved {
                        self.line.font.pop_scope(saved);
                    }
                }
            }
        })
    }

    fn flush(&mut self) {
        self.flush_with(false);
    }

    fn flush_for_vertical_request(&mut self) {
        self.flush_with(true);
    }

    fn flush_with(&mut self, vertical_request: bool) {
        let mut next = InlineBuilder::with_spacing(self.line.spacing_enabled());
        next.font = self.line.font;
        next.scope_posts = self.line.scope_posts.clone();
        self.line.transfer_source_cursor(&mut next);
        if vertical_request {
            self.line.transfer_vertical_request_execution(&mut next);
        } else {
            self.line.transfer_container_execution(&mut next);
        }
        let empty_word_end_break = self.line.take_unrepresented_word_end_break();
        let children = std::mem::replace(&mut self.line, next).finish();
        if !children.is_empty() {
            self.output.push(if self.literal {
                Block::Preformatted {
                    children,
                    language: None,
                    layout: layout(self.indent_columns),
                    source: self.source.take(),
                }
            } else {
                Block::Paragraph {
                    children,
                    layout: layout(self.indent_columns),
                    source: self.source.take(),
                }
            });
        }
        if empty_word_end_break {
            self.output.push(Block::VerticalSpace {
                lines: 1,
                source: None,
            });
        }
    }

    fn set_literal_mode(&mut self, literal: bool) {
        self.flush();
        self.literal = literal;
        let mut next = InlineBuilder::with_spacing(self.line.spacing_enabled());
        next.font = self.line.font;
        next.scope_posts = self.line.scope_posts.clone();
        self.line.transfer_container_execution(&mut next);
        if literal {
            next.track_executed_lines();
        }
        self.line = next;
        self.source = None;
    }

    fn append_inline(&mut self, node: &Node, next: Option<&Node>) {
        if self.source.is_none() {
            self.source = source_span(node);
        }
        match node.macro_name.as_deref() {
            // Bd chooses the initial mode; actual roff requests can switch
            // it inside the body. Both mandoc node flags and formatter
            // execution distinguish these from another literal source row.
            Some("fi") => self.set_literal_mode(false),
            Some("nf") => self.set_literal_mode(true),
            Some("Pp") if !node.flags.no_print => {
                // mdoc_term.c termp_pp_pre executes term_vspace even after
                // an explicit sp or a continued word. This is a structural
                // request, not the inline break used in definition heads.
                let lines = self.line.resolve_vertical_space(1);
                self.flush_for_vertical_request();
                self.output.push(Block::VerticalSpace {
                    lines,
                    source: source_span(node),
                });
                self.line.reset_source_cursor();
                // Native Pp tags point after its vertical request.
                for target in super::targets::structural_targets(node) {
                    self.line
                        .append(vec![Inline::anchor_at(target, source_span(node))]);
                }
            }
            Some("sp") => {
                let lines = self.line.resolve_vertical_space(vertical_space_delta(node));
                self.flush_for_vertical_request();
                self.output.push(Block::VerticalSpace {
                    lines,
                    source: source_span(node),
                });
                self.line.reset_source_cursor();
            }
            Some("br") => {
                if self.literal {
                    self.flush();
                    self.line.reset_source_cursor();
                } else {
                    self.line.hard_break();
                }
            }
            _ => {
                if !self.literal
                    && node.kind == NodeKind::Text
                    && node.flags.line_start
                    && !self.line.has_tight_boundary()
                {
                    // A display switched to fill uses the same authored
                    // text-line policy as ordinary filled block flow. Macro
                    // operands must not acquire this source-word boundary.
                    if super::starts_indented_filled_line(node) {
                        self.line.hard_break();
                    } else {
                        self.line.preserve_source_word_boundary();
                    }
                }
                append_inline_node_with_next(&mut self.line, node, next, self.context.default_name);
            }
        }
    }

    fn append_nodes(&mut self, nodes: &[Node]) {
        let plan = TableEmbeddingPlan::new(nodes, self.context);
        for (index, node) in nodes.iter().enumerate() {
            // mdoc_html.c::print_mdoc_node switches fill mode before every
            // node, including a Bl/table following a crossed `.Ed` inside
            // an open inline scope. The BODY end marker itself still carries
            // NOFILL, so switching unconditionally at `.Ed` would be early.
            if self.honor_node_fill
                && !matches!(node.macro_name.as_deref(), Some("fi" | "nf"))
                && self.literal != node.flags.no_fill
            {
                self.set_literal_mode(node.flags.no_fill);
            }
            if self.append_container(node) {
                self.paragraph_predecessor |= super::super::adjacency::is_logical_sibling(node);
                continue;
            }
            if node.kind == NodeKind::Block
                && matches!(node.macro_name.as_deref(), Some("Bd" | "D1" | "Dl"))
            {
                // A display changes the formatter's origin and mode even
                // inside another literal display. Re-enter structural
                // lowering with the complete node instead of flattening its
                // body into the outer fixed-origin inline buffer. mandoc's
                // termp_bd_pre()/post() restore geometry at this boundary;
                // font and spacing changes still follow their normal scope.
                self.flush();
                self.commit_line_execution();
                let nested = super::lower_blocks_with_predecessor(
                    std::slice::from_ref(node),
                    self.context,
                    self.indent_columns,
                    &mut 1,
                    self.line.spacing_enabled(),
                    self.paragraph_predecessor || !self.output.is_empty(),
                    &mut self.formatter,
                );
                self.output.extend(nested);
                self.paragraph_predecessor = true;
                self.resume_line_execution();
            } else if node.kind == NodeKind::Table
                || (node.kind == NodeKind::Block
                    && matches!(node.macro_name.as_deref(), Some("Bl" | "Rs")))
            {
                self.flush();
                self.commit_line_execution();
                if node.kind == NodeKind::Table {
                    append_table_row(
                        &mut self.output,
                        node,
                        self.context,
                        self.indent_columns,
                        plan.embedding(index),
                        &mut self.formatter,
                    );
                } else {
                    self.output.extend(super::lower_blocks_with_predecessor(
                        std::slice::from_ref(node),
                        self.context,
                        self.indent_columns,
                        &mut 1,
                        self.line.spacing_enabled(),
                        self.paragraph_predecessor || !self.output.is_empty(),
                        &mut self.formatter,
                    ));
                }
                self.resume_line_execution();
            } else if node.kind != NodeKind::Text && node.macro_name.is_none() {
                self.append_nodes(&node.children);
            } else {
                self.append_inline(node, nodes.get(index + 1));
            }
            self.paragraph_predecessor |= super::super::adjacency::is_logical_sibling(node);
        }
    }
}

#[cfg(test)]
mod tests {
    use mant_ir::Inline;

    #[test]
    fn font_styling_preserves_line_boundaries() {
        let mut builder = super::InlineBuilder::new();
        builder.with_font_scope(crate::mandoc::roff_escape::RoffFont::Strong, |builder| {
            builder.append_text("first");
            builder.hard_break();
            builder.append_text("second");
        });
        let styled = builder.finish();

        assert!(matches!(
            styled.as_slice(),
            [
                Inline::Strong { children: first },
                Inline::LineBreak,
                Inline::Strong { children: second },
            ] if mant_ir::inline_plain_text(first) == "first" && mant_ir::inline_plain_text(second) == "second"
        ));
    }
}
