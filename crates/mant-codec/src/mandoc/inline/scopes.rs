//! Compose semantic wrappers in output order, not by inspecting AST tails.
use super::font::coalesce_font_runs;
use super::links::{append_bsd_reference, append_link, append_mail_addresses};
use super::{
    Font, Inline, InlineBuilder, Node, NodeKind, append_include, append_inline_nodes,
    authored_section_phrase, first_part_children, inline_children, lower_equation_node,
    navigation_anchor,
};

pub(super) fn append(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    if node.macro_name.as_deref() == Some("Tg") {
        if let Some(anchor) = navigation_anchor(node) {
            builder.append(vec![anchor]);
        }
        return;
    }
    if node.flags.no_print || node.kind == NodeKind::Comment {
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
    let children = inline_children(node);
    match node.macro_name.as_deref() {
        Some("Fn" | "Fo") => super::generated::function(builder, node, name),
        Some("Xr" | "MR") => super::generated::manual_reference(builder, node, name),
        Some("Nm") if node.kind == NodeKind::Block => {
            builder.with_font_scope(Font::Strong, |builder| {
                append_name(builder, first_part_children(node, NodeKind::Head), name);
            });
            append_inline_nodes(builder, first_part_children(node, NodeKind::Body), name);
        }
        Some("Nm") => {
            builder.with_font_scope(Font::Strong, |builder| append_name(builder, children, name));
        }
        Some("Fl") => builder.append_scope(
            |builder| {
                builder.with_font_scope(Font::Strong, |builder| {
                    builder.append_text("-");
                    builder
                        .with_prefix_join(|builder| append_inline_nodes(builder, children, name));
                });
            },
            coalesce_font_runs,
        ),
        Some("Cm" | "Ic" | "Sy" | "Ms") => builder.with_font_scope(Font::Strong, |builder| {
            append_inline_nodes(builder, children, name);
        }),
        Some("Ar" | "Pa" | "Em" | "Va" | "Vt" | "Ft" | "Fa" | "Ad" | "Fr") => builder
            .with_font_scope(Font::Emphasis, |builder| {
                append_inline_nodes(builder, children, name);
            }),
        Some("No" | "Dv") => builder.with_font_scope(Font::Regular, |builder| {
            append_inline_nodes(builder, children, name);
        }),
        Some("Li") => builder.with_font_scope(Font::Code, |builder| {
            append_inline_nodes(builder, children, name);
        }),
        Some("Sx") => {
            let authored_target = authored_section_phrase(children, name);
            builder.append_scope(
                |builder| {
                    builder.with_font_scope(Font::Emphasis, |builder| {
                        append_inline_nodes(builder, children, name);
                    });
                },
                |children| section_reference(authored_target, children),
            );
        }
        Some("Nd") => {
            builder.append_text("— ");
            append_inline_nodes(builder, children, name);
        }
        Some("%T") if node.reference_quotes_title => append_quoted_title(builder, children, name),
        // mdoc_term.c maps these reference fields through termp_under_pre();
        // quoted %T takes the separate branch above when a journal is present.
        Some("%B" | "%I" | "%J" | "%T") => {
            builder.with_font_scope(Font::Emphasis, |builder| {
                append_inline_nodes(builder, children, name);
            });
        }
        _ => append_inline_nodes(builder, children, name),
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

    fn source_node(&mut self, node: &'node Node, starts_line: bool) {
        // The caller entered the root before scope dispatch.  Direct Fo Fa
        // operands enter in function_argument(); all other nested source
        // wrappers use the same cursor as ordinary text nodes.
        if starts_line && node.id != self.root_id && node.macro_name.as_deref() != Some("Fa") {
            self.builder.begin_executed_node(node);
        }
    }

    fn event(&mut self, event: crate::mandoc::containers::Event<'node>) {
        use crate::mandoc::containers::Event;
        match event {
            Event::BeginNode(node) => self.builder.begin_executed_node(node),
            Event::Anchor(target, source) => {
                self.builder.append(vec![Inline::anchor_at(target, source)]);
            }
            Event::Break => {
                self.builder.hard_break();
            }
            Event::FlushLine => {
                self.builder.append(vec![Inline::LineBreak]);
            }
            Event::Children(nodes) => append_inline_nodes(self.builder, nodes, self.name),
            Event::Glyph(value) => self.builder.append_text(&value),
            Event::Tight => self.builder.tighten_next_boundary(),
            Event::Release => self.builder.release_next_boundary(),
            Event::EmptyWord => self.builder.execute_empty_word(),
            Event::EnterKeep => self.builder.enter_keep_words(),
            Event::ExitKeep => self.builder.exit_keep_words(),
            Event::FunctionArgument(argument, comma_after) => {
                super::generated::function_argument(self.builder, argument, comma_after, self.name);
            }
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
    match node.macro_name.as_deref() {
        Some("In") => append_include(builder, node, name),
        Some("Bx") => append_bsd_reference(builder, node, name),
        Some("Lk") => {
            if let Some(anchor) = navigation_anchor(node) {
                builder.append(vec![anchor]);
            }
            append_link(builder, node, name);
        }
        Some("Mt") => {
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

fn section_reference(authored_target: String, children: Vec<Inline>) -> Vec<Inline> {
    if children.is_empty() {
        return children;
    }
    vec![Inline::Link {
        target: mant_ir::LinkTarget::Section {
            id: authored_target.into(),
        },
        title: None,
        children,
    }]
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
