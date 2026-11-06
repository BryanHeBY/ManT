//! Compose semantic wrappers in output order, not by inspecting AST tails.
use super::font::coalesce_font_runs;
use super::links::{append_bsd_reference, append_link, append_mail_addresses};
use super::{
    Font, Inline, InlineBuilder, Node, NodeKind, append_include, append_inline_nodes,
    authored_section_phrase, inline_children, lower_equation_node, navigation_anchor,
};
mod nested;
use nested::{append_column_list, append_nested_list};

use libmandoc_rs::{
    MacroToken::{Man, Mdoc},
    ManMacro, MdocMacro,
};

pub(super) fn append(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Tg)) {
        if let Some(anchor) = navigation_anchor(node) {
            builder.append(vec![anchor]);
        }
        return;
    }
    if node.flags.no_print || node.kind == NodeKind::Comment {
        return;
    }
    if append_structural_scope(builder, node, name) {
        return;
    }
    // Atomic reconstructions own their generated punctuation (references,
    // function declarations, etc.); their output cannot carry a guessed tail
    // effect. Transparent and styled scopes below share the caller's flow.
    if append_atomic(builder, node, name) {
        return;
    }
    if let Some(anchor) = navigation_anchor(node) {
        builder.append(vec![anchor]);
    }
    let posts = builder.scope_posts.clone();
    let mut sink = InlineContainerSink {
        builder,
        name,
        root_id: node.id,
    };
    if crate::mandoc::containers::drive(node, &posts, &mut sink) {
        return;
    }
    append_scope_children(builder, node, name);
}

fn append_structural_scope(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) -> bool {
    if node.kind == NodeKind::Block && node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Rs)) {
        if let Some(anchor) = navigation_anchor(node) {
            builder.append(vec![anchor]);
        }
        append_bibliography(builder, node, name);
        return true;
    }
    if node.kind == NodeKind::Block
        && node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bl))
        && node.list_kind.is_some()
        && (node.list_kind == Some(libmandoc_rs::NormalizedListKind::Column)
            || builder.has_definition_list_execution())
    {
        if node.list_kind == Some(libmandoc_rs::NormalizedListKind::Column) {
            append_column_list(builder, node, name);
        } else {
            append_nested_list(builder, node, name);
        }
        return true;
    }
    if node.kind == NodeKind::Block
        && matches!(
            node.macro_token.as_ref(),
            Some(Mdoc(MdocMacro::D1 | MdocMacro::Dl | MdocMacro::Bd))
        )
    {
        if let Some(anchor) = navigation_anchor(node) {
            builder.append(vec![anchor]);
        }
        append_display(builder, node, name);
        return true;
    }
    false
}

fn append_bibliography(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    // Rs has no terminal post or BODY font pre. Its actual field children
    // still own conjunctions and punctuation, even inside a detached HEAD.
    // Reuse the block walker's logical sibling rules and the same builder.
    for part in &node.children {
        builder.begin_executed_node(part);
        let geometry = builder.definition_geometry_checkpoint(part);
        let font = builder.font.checkpoint();
        if part.kind == NodeKind::Body {
            let posts = builder.scope_posts.clone();
            posts.enter_body(part.id, font);
            append_reference_body(builder, &part.children, name);
            if let Some(saved) = posts.exit_body(part.id) {
                builder.font.pop_scope(saved);
            }
        } else {
            append_inline_nodes(builder, &part.children, name);
            builder.font.pop_scope(font);
        }
        builder.execution.finish_executed_node(part);
        builder.restore_definition_geometry(geometry);
    }
}

fn append_reference_body(builder: &mut InlineBuilder, nodes: &[Node], name: Option<&str>) {
    use crate::mandoc::blocks::mdoc_macros::{reference_author_conjunction, reference_field_post};

    // Validation may retain non-bibliography nodes with warnings. They still
    // execute in the original Rs BODY chain; generated field punctuation is
    // emitted once around that execution, never as a replacement traversal.
    for (index, (node, siblings)) in crate::mandoc::adjacency::with_siblings(nodes).enumerate() {
        if reference_author_conjunction(nodes, index) {
            builder.append_text("and");
        }
        super::append_inline_node_with_source(builder, node, siblings, name);
        if let Some(punctuation) = reference_field_post(nodes, index) {
            builder.tighten_next_boundary();
            // termp____post() writes one real punctuation word. Its word
            // entry consumes Tight and leaves NOSPACE according to Sm; it
            // does not execute Eo/En's separate native release operation.
            builder.append_text(punctuation.glyph());
            // termp____post() explicitly arms SENTENCE after the final
            // generated period. This is not inferred from its glyph.
            if punctuation.ends_sentence() {
                builder.execution.arm_sentence_end();
            }
        }
    }
}

fn append_scope_children(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    let children = inline_children(node);
    match node.macro_token.as_ref() {
        Some(Mdoc(MdocMacro::Fn | MdocMacro::Fo)) => {
            super::generated::function(builder, node, name);
        }
        Some(Mdoc(MdocMacro::Xr) | Man(ManMacro::Mr)) => {
            super::generated::manual_reference(builder, node, name);
        }
        Some(Mdoc(MdocMacro::Nm)) if node.kind == NodeKind::Block => {
            super::names::append_block(builder, node, None, name);
        }
        Some(Mdoc(MdocMacro::Nm)) => {
            builder.with_font_scope(Font::Strong, |builder| append_name(builder, children, name));
        }
        Some(Mdoc(MdocMacro::Fl)) => append_option(builder, children, name),
        // mdoc_term.c routes Cd and Fd through termp_fd_pre = termp_bold_pre
        // (dispatch lines 142/149): unconditional Strong in every section.
        Some(Mdoc(
            MdocMacro::Cm
            | MdocMacro::Ic
            | MdocMacro::Sy
            | MdocMacro::Ms
            | MdocMacro::Cd
            | MdocMacro::Fd,
        )) => {
            builder.with_font_scope(Font::Strong, |builder| {
                append_inline_nodes(builder, children, name);
            });
        }
        Some(Mdoc(MdocMacro::Ar)) => builder.with_native_operand_role(
            crate::definitions::NativeOperandRole::Argument,
            |builder| {
                // `Ar` dispatches through termp_under_pre, then its actual
                // children. A child's font escape does not rename the
                // argument macro as another literal declaration.
                builder.with_font_scope(Font::Emphasis, |builder| {
                    append_inline_nodes(builder, children, name);
                });
            },
        ),
        Some(Mdoc(
            MdocMacro::Pa
            | MdocMacro::Em
            | MdocMacro::Va
            | MdocMacro::Vt
            | MdocMacro::Ft
            | MdocMacro::Fa
            | MdocMacro::Ad
            | MdocMacro::Fr,
        )) => {
            builder.with_font_scope(Font::Emphasis, |builder| {
                append_inline_nodes(builder, children, name);
            });
        }
        Some(Mdoc(MdocMacro::No | MdocMacro::Dv)) => {
            builder.with_font_scope(Font::Regular, |builder| {
                append_inline_nodes(builder, children, name);
            });
        }
        Some(Mdoc(MdocMacro::Li)) => builder.with_font_scope(Font::Code, |builder| {
            append_inline_nodes(builder, children, name);
        }),
        Some(Mdoc(MdocMacro::Sx)) => append_section_reference(builder, children, name),
        Some(Mdoc(MdocMacro::Nd)) => {
            // mdoc_term.c::termp_nd_pre() prints `\(en` (U+2013), not an em
            // dash. Its next term_word() supplies the separate automatic
            // separator; there is no authored blank in this generated word.
            builder.append_text(&super::catalog_glyph("en").to_string());
            append_inline_nodes(builder, children, name);
        }
        // mdoc_html.c::mdoc__x_pre() enriches these fields with typed
        // external targets while the visible terminal word is unchanged.
        Some(Mdoc(MdocMacro::PercentU)) => {
            super::links::append_reference_field_link(builder, children, name, false);
        }
        Some(Mdoc(MdocMacro::PercentR)) => {
            super::links::append_reference_field_link(builder, children, name, true);
        }
        Some(Mdoc(MdocMacro::PercentT)) if node.reference_quotes_title => {
            append_quoted_title(builder, children, name);
        }
        // mdoc_term.c maps these reference fields through termp_under_pre();
        // quoted %T takes the separate branch above when a journal is present.
        Some(Mdoc(
            MdocMacro::PercentB | MdocMacro::PercentI | MdocMacro::PercentJ | MdocMacro::PercentT,
        )) => {
            builder.with_font_scope(Font::Emphasis, |builder| {
                append_inline_nodes(builder, children, name);
            });
        }
        _ => append_inline_nodes(builder, children, name),
    }
}

fn append_option(builder: &mut InlineBuilder, children: &[Node], name: Option<&str>) {
    builder.with_native_operand_role(
        crate::definitions::NativeOperandRole::ExplicitOption,
        |builder| {
            // mdoc_term.c::termp_fl_pre() emits the bold dash and then visits
            // children in the same scope, proving each option spelling.
            builder.append_scope(
                |builder| {
                    builder.with_font_scope(Font::Strong, |builder| {
                        builder.append_text("-");
                        builder.with_prefix_join(|builder| {
                            append_inline_nodes(builder, children, name);
                        });
                    });
                },
                coalesce_font_runs,
            );
        },
    );
}

fn append_section_reference(builder: &mut InlineBuilder, children: &[Node], name: Option<&str>) {
    let authored_target = authored_section_phrase(children, name);
    let registered = builder
        .zero_advance
        .new_annotation(mant_ir::LinkTarget::Section {
            id: authored_target.into(),
        });
    let link_target = registered.target;
    let (annotated, membership) = builder.append_semantic_scope(
        registered.key,
        false,
        |builder| {
            builder.with_font_scope(Font::Emphasis, |builder| {
                append_inline_nodes(builder, children, name);
            });
        },
        |children| {
            vec![Inline::Link {
                target: link_target.clone(),
                title: None,
                children,
            }]
        },
    );
    builder.zero_advance.bind_deferred(
        membership,
        &super::ownership::Annotation::Link {
            target: link_target.clone(),
            title: None,
        },
        annotated,
    );
}

fn append_display(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    // D1/Dl pre and Bd's print_bvspace() both execute term_newln() before
    // changing tab configuration (mdoc_term.c:589,1328,1436). This is the
    // ordinary newline, not roff_pre_br(): temporary BRIND flags survive.
    builder.no_fill_source_line();
    if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd))
        && !node.compact
        && builder.has_definition_head()
    {
        // print_bvspace() reaches the enclosing non-item It even when Bd
        // is its first HEAD child (mdoc_term.c:600-618). Its term_vspace()
        // is independent of roff .sp: keep BRIND/NOBREAK and consume any
        // negative vertical-space debt before asserting an output row.
        builder.native_vertical_space(1);
    }
    super::display_tabs::enter_pre(&mut builder.execution, node);
    if matches!(
        node.macro_token.as_ref(),
        Some(Mdoc(MdocMacro::D1 | MdocMacro::Dl))
    ) {
        // termp_d1_pre changes state only at BLOCK; the complete native
        // child stream still executes every HEAD/BODY/TAIL and direct TEXT
        // in order. Each part restores its font before its next sibling.
        append_inline_nodes(builder, &node.children, name);
        builder.no_fill_source_line();
        return;
    }
    let Some(body) = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Body && child.scope_end.is_none())
    else {
        return;
    };
    let posts = builder.scope_posts.clone();
    posts.enter_display_body(
        body.id,
        &builder.font,
        (node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd))).then_some(node.flags.no_fill),
    );
    builder.begin_executed_node(body);
    super::display_tabs::enter_pre(&mut builder.execution, body);
    append_inline_nodes(builder, &body.children, name);
    posts.restore_body_font(body.id, &mut builder.font);
    if !posts.ended(body.id) {
        // Bd BODY post and D1/Dl BLOCK post call term_newln(), exactly
        // once for the original scope (mdoc_term.c:1131,1482).
        if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd)) {
            builder.finish_display_body(node.display_kind);
        } else {
            builder.no_fill_source_line();
        }
        posts.finish(body.id);
    }
    if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd)) {
        posts.exit_display_fill(body.id);
    }
}

struct InlineContainerSink<'a> {
    builder: &'a mut InlineBuilder,
    name: Option<&'a str>,
    root_id: u32,
}

impl<'node> crate::mandoc::containers::ContainerSink<'node> for InlineContainerSink<'_> {
    fn font(&mut self) -> &mut super::FontState {
        &mut self.builder.font
    }

    fn finish_node(&mut self, node: &Node) {
        // The caller finishes the root after its handler returns. Parts
        // entered directly by the walker finish after their own post here.
        if node.id != self.root_id {
            self.builder.execution.finish_executed_node(node);
        }
    }

    fn geometry_checkpoint(&self, node: &Node) -> Option<super::DefinitionGeometryCheckpoint> {
        self.builder.definition_geometry_checkpoint(node)
    }

    fn restore_geometry(&mut self, checkpoint: Option<super::DefinitionGeometryCheckpoint>) {
        self.builder.restore_definition_geometry(checkpoint);
    }

    fn restore_fill(&mut self, _fill: bool, marker: &Node) {
        // An explicit .Ed restores the original display at its source
        // marker, before subsequent children execute. Inline execution
        // derives their fill flags from AST nodes; its post still owes the
        // same term_newln() as a normal return (mdoc_term.c:1474-1486).
        // mdoc_endbody_alloc shares the original BODY norm, including its
        // display kind. The marker's post consumes that original scope.
        self.builder.finish_display_body(marker.display_kind);
    }

    fn source_node(&mut self, node: &'node Node, starts_line: bool) {
        // The caller entered the root before scope dispatch. Function BODY
        // children enter through their complete parent-local walker; nested
        // wrappers use the same cursor as ordinary text nodes.
        if starts_line
            && node.id != self.root_id
            && node.macro_token.as_ref() != Some(&Mdoc(MdocMacro::Fa))
        {
            self.builder.begin_executed_node(node);
        }
    }

    fn event(&mut self, event: crate::mandoc::containers::Event<'node>) {
        use crate::mandoc::containers::Event;
        match event {
            Event::BeginNode(node) => self.builder.begin_executed_node(node),
            Event::Alignment(alignment) => self.builder.set_text_alignment(alignment),
            Event::Anchor(target, source) => {
                self.builder.append(vec![Inline::anchor_at(target, source)]);
            }
            Event::Break => {
                // roff_term_pre_ce()/pre_br() request term_newln(). A HANG
                // field can flush under TERMP_NOBREAK without ending the
                // device row, so execute the field transition first.
                self.builder.control_line_break();
            }
            Event::FlushLine => {
                // ce/rj explicitly set NOSPACE before term_flushln for
                // each grouped line (roff_term.c:103). Ordinary direct
                // Fn/Nm flushes do not own this caller-side transition.
                self.builder.tighten_next_boundary();
                self.builder.flush_execution_line();
            }
            Event::Children(nodes) => append_inline_nodes(self.builder, nodes, self.name),
            Event::AlignedChildren(nodes) => self.builder.with_grouped_line_dispatch(|builder| {
                append_inline_nodes(builder, nodes, self.name);
            }),
            Event::Glyph(value) => self.builder.append_text(&value),
            Event::Tight => self.builder.tighten_next_boundary(),
            Event::Release => self.builder.release_next_boundary(),
            Event::Word(super::super::containers::NativeWord::Empty) => {
                self.builder.execute_empty_word();
            }
            Event::Word(super::super::containers::NativeWord::Invisible) => {
                super::append_normalized_word(self.builder, "\\&");
            }
            Event::Word(super::super::containers::NativeWord::Roff(value)) => {
                super::append_normalized_word(self.builder, value);
            }
            Event::EnterKeep => self.builder.enter_keep_words(),
            Event::ExitKeep => self.builder.exit_keep_words(),
            Event::FunctionChildren(nodes) => {
                super::append_function_children(self.builder, nodes, self.name);
            }
            Event::FunctionBodyBegin(pretty) => self.builder.begin_function_body(pretty),
            Event::FunctionBodyEnd(pretty) => self.builder.finish_function_body(pretty),
            _ => unreachable!("the shared container driver owns source and font events"),
        }
    }
}

fn append_quoted_title(builder: &mut InlineBuilder, children: &[Node], name: Option<&str>) {
    // post_rs() sets `norm.Rs.quote_T` after reordering fields. CVS
    // mdoc_html.c::mdoc__x_pre/post encloses only the title field.
    builder.append_text("“");
    builder.tighten_next_boundary();
    append_inline_nodes(builder, children, name);
    builder.tighten_next_boundary();
    builder.append_text("”");
}

/// Atomic syntax owns a semantic wrapper, but its visible components still
/// execute in the caller's formatter stream.  Keep that dispatch separate
/// from ordinary scope lowering so new atomic forms cannot accidentally
/// recreate an isolated zero-advance state.
fn append_atomic(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) -> bool {
    match node.macro_token.as_ref() {
        Some(Mdoc(MdocMacro::In)) => append_include(builder, node, name),
        Some(Mdoc(MdocMacro::Bx)) => append_bsd_reference(builder, node, name),
        Some(Mdoc(MdocMacro::Lk)) => {
            if let Some(anchor) = navigation_anchor(node) {
                builder.append(vec![anchor]);
            }
            // termp_lk_pre() calls term_word directly, then returns0: its
            // operand NODE_EOS flags never execute print_mdoc_node's exit.
            builder.with_direct_word_operands(|builder| append_link(builder, node, name));
        }
        Some(Mdoc(MdocMacro::Mt)) => {
            if let Some(anchor) = navigation_anchor(node) {
                builder.append(vec![anchor]);
            }
            append_mail_addresses(builder, node, name);
        }
        _ if node.kind == NodeKind::Equation => {
            builder.append(lower_equation_node(node));
        }
        _ => return false,
    }
    true
}

fn append_name(builder: &mut InlineBuilder, nodes: &[Node], name: Option<&str>) {
    if nodes.is_empty() {
        if let Some(name) = name {
            builder.append_text(name);
        }
    } else {
        append_inline_nodes(builder, nodes, name);
    }
}
