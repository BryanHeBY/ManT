//! Inline AST node entry, macro dispatch and scope exit.
use libmandoc_rs::{
    MacroToken::{Man, Mdoc, Roff},
    ManMacro, MdocMacro, Node, NodeKind, RoffMacro,
};
use mant_ir::Inline;

use super::{
    InlineBuilder, append_inline_nodes, font::lower_man_font_scope, lower_inline_nodes, names,
    plain_text, scopes, synopsis, word_input::append_text_node,
};
use crate::mandoc::{adjacency, controls, layout, roff_escape, source_span, targets};

/// Execute one node without an original parent sequence. Native pre-handlers
/// use this for their individual operands; callers owning a complete child
/// array must use `adjacency::with_siblings` and `append_inline_node_with_source`.
pub(in crate::mandoc) fn append_isolated_inline_node(
    builder: &mut InlineBuilder,
    node: &Node,
    default_name: Option<&str>,
) {
    append_inline_node_with_source(builder, node, adjacency::Siblings::default(), default_name);
}

pub(in crate::mandoc) fn append_inline_node_with_siblings(
    builder: &mut InlineBuilder,
    node: &Node,
    previous: Option<&Node>,
    next: Option<&Node>,
    default_name: Option<&str>,
) {
    append_inline_node_with_source(
        builder,
        node,
        adjacency::Siblings {
            previous,
            raw_next: next,
            next,
        },
        default_name,
    );
}

pub(in crate::mandoc) fn append_inline_node_with_source(
    builder: &mut InlineBuilder,
    node: &Node,
    siblings: adjacency::Siblings<'_>,
    default_name: Option<&str>,
) {
    let previous = siblings.previous;
    let next = siblings.raw_next;
    if node.flags.no_print || node.kind == NodeKind::Comment {
        // Tg can recover an authored target even when native output is hidden.
        if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Tg)) {
            scopes::append(builder, node, default_name);
        }
        return;
    }
    if node.kind == NodeKind::Text && !node.flags.no_print {
        append_text_node(builder, node);
        builder.finish_executed_node(node);
        return;
    }
    let final_word_join_before = builder.final_word_join_state();
    if node.scope_end.is_some()
        && node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Eo))
        && !node.children.is_empty()
    {
        // mdoc_html.c::mdoc_eo_pre() applies NOSPACE before the marker's
        // child word executes. In an inline Bk scope, PREKEEP otherwise
        // commits an ordinary blank before the container callback can run.
        builder.tighten_next_boundary();
    }
    builder.begin_executed_node(node);
    let geometry = builder.definition_geometry_checkpoint(node);
    synopsis::enter(builder, node, previous);
    if node.kind == NodeKind::Block
        && node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Rs))
        && node.section == libmandoc_rs::NormalizedSection::SeeAlso
        && previous.is_some()
    {
        // termp_rs_pre() tests this BLOCK's own roff_node_prev, after
        // NODE_LINE, not a document-global paragraph predecessor.
        builder.native_vertical_space(1);
    }
    if execute_author_pre(builder, node) {
        builder.finish_executed_node(node);
        builder.restore_definition_geometry(geometry);
        return;
    }
    // Native node entry and macro pre are not term_word(): print_mdoc_node()
    // runs pre before its children (mdoc_term.c:398-407), and only an actual
    // word clears skipvsp (term.c:573-589). In particular Bd's print_bvspace,
    // D1/Dl's newline, font scopes, and transparent Xo wrappers must execute
    // before any later operand changes word registers. TEXT and generated
    // words already enter through their own formatter-word methods below.
    if prepares_semantic_output_owner(node) {
        // Compact semantic wrappers capture their whole operand stream in
        // one Link or Code node. Their entry is still no
        // word event, so the only preparation is an ownership checkpoint:
        // a `\z` glyph delayed from preceding source settles at the real
        // first operand's own word entry, and the checkpoint then returns
        // it to its original owner instead of letting the wrapper's
        // annotation capture it.
        let checkpoint = builder.begin_semantic_owner_checkpoint();
        execute_inline_macro_handler(builder, node, previous, next, default_name);
        builder.finish_semantic_owner_checkpoint(checkpoint);
        finish_inline_node_execution(builder, node, siblings.next, final_word_join_before);
        execute_inline_macro_post(builder, node);
        builder.finish_executed_node(node);
        builder.restore_definition_geometry(geometry);
        return;
    }
    if node.flags.delimiter_close {
        builder.tighten_next_boundary();
    }
    execute_inline_macro_handler(builder, node, previous, next, default_name);
    finish_inline_node_execution(builder, node, siblings.next, final_word_join_before);
    execute_inline_macro_post(builder, node);
    builder.finish_executed_node(node);
    builder.restore_definition_geometry(geometry);
}

fn append_ordered_declaration_or_display_part(
    builder: &mut InlineBuilder,
    node: &Node,
    previous: Option<&Node>,
    default_name: Option<&str>,
) -> bool {
    match node.macro_token.as_ref() {
        Some(Mdoc(MdocMacro::Nm)) if node.kind == NodeKind::Block => {
            names::append_block(builder, node, previous, default_name);
        }
        Some(Mdoc(MdocMacro::Vt)) if node.kind == NodeKind::Block => {
            // termp_vt_pre(BLOCK) changes no font. Its complete ordered
            // stream includes direct TEXT before/after BODY; only BODY's
            // own under-pre scope ends before those trailing siblings.
            append_inline_nodes(builder, &node.children, default_name);
        }
        Some(Mdoc(MdocMacro::Vt)) if node.kind == NodeKind::Head => {
            // The shared node entry still executes; termp_vt_pre(HEAD)
            // returns zero, including for a present empty HEAD.
        }
        Some(Mdoc(MdocMacro::D1 | MdocMacro::Dl))
            if matches!(node.kind, NodeKind::Head | NodeKind::Body | NodeKind::Tail) =>
        {
            let saved_font = builder.font.checkpoint();
            let posts = builder.scope_posts.clone();
            if node.kind == NodeKind::Body {
                posts.enter_display_body(node.id, &builder.font, None);
            }
            append_inline_nodes(builder, &node.children, default_name);
            if node.kind == NodeKind::Body {
                posts.restore_body_font(node.id, &mut builder.font);
            } else {
                builder.font.pop_scope(saved_font);
            }
        }
        _ => return false,
    }
    true
}

fn execute_inline_macro_handler(
    builder: &mut InlineBuilder,
    node: &Node,
    previous: Option<&Node>,
    next: Option<&Node>,
    default_name: Option<&str>,
) {
    if node.kind == NodeKind::Equation {
        super::append_equation_node(builder, node, next);
        return;
    }
    if append_ordered_declaration_or_display_part(builder, node, previous, default_name) {
        return;
    }
    match node.macro_token.as_ref() {
        Some(Man(
            ManMacro::B
            | ManMacro::I
            | ManMacro::Sb
            | ManMacro::R
            | ManMacro::Bi
            | ManMacro::Br
            | ManMacro::Ib
            | ManMacro::Ir
            | ManMacro::Rb
            | ManMacro::Ri
            | ManMacro::Op,
        )) => {
            lower_man_font_scope(builder, node, default_name);
        }
        Some(Mdoc(MdocMacro::Ns)) => {
            if !node.flags.line_start {
                builder.tighten_next_boundary();
            }
        }
        // `Pf` owns visible prefix text and suppresses only the boundary to
        // the following sibling. Treating it like the empty `Ns` request
        // silently discarded constructs such as `.Pf [\-]ddd Cm \&.`.
        Some(Mdoc(MdocMacro::Pf)) => {
            scopes::append(builder, node, default_name);
            if next.is_some_and(|next| !next.flags.line_start) {
                builder.tighten_next_boundary();
            }
        }
        // A roff break ends the current output line, not the paragraph.
        // `Pp` can also occur inside an extended mdoc definition head, where
        // it separates alternative terms without ending the owning item.
        // Keeping both inline lets the definition lowering retain that
        // distinction instead of concatenating the alternatives.
        Some(Roff(RoffMacro::Br)) => {
            builder.control_line_break();
        }
        Some(Mdoc(MdocMacro::Pp)) => {
            // mdoc_term.c::termp_pp_pre() executes term_vspace(), including
            // the current field's term_newln() and skipvsp, before recording
            // the target. Collecting a HEAD does not change that execution.
            builder.native_vertical_space(1);
            builder.mark_definition_term_break();
            if let Some(target) = targets::raw_target(node) {
                builder.append(vec![Inline::anchor_at(target, source_span(node))]);
            }
        }
        // Formatting requests carry control arguments such as `CW` and `R`.
        // `Es` likewise only changes the delimiters later `En` nodes use;
        // libmandoc resolves those delimiters onto each invocation. These
        // requests change formatter state and are never document text.
        // Verbatim regions already retain their semantics through
        // libmandoc's no-fill flag, so leaking these arguments would only
        // create phantom paragraphs around preformatted blocks.
        Some(Roff(RoffMacro::Ft)) => {
            let name = node
                .children
                .first()
                .and_then(|node| node.text.as_deref())
                .unwrap_or("P");
            if name == "P" {
                builder.font.restore();
            } else {
                builder.font.select(roff_escape::font(name));
            }
        }
        Some(Man(ManMacro::Sm)) => {
            append_inline_nodes(builder, &node.children, default_name);
        }
        Some(Mdoc(MdocMacro::Sm)) => {
            let setting = plain_text(&lower_inline_nodes(&node.children, default_name));
            builder.set_spacing(setting.trim());
        }
        Some(Roff(RoffMacro::Ti)) => {
            builder.temporary_indent(
                node.children
                    .first()
                    .and_then(|child| child.text.as_deref()),
            );
        }
        // The request's own roff_term_pre_br() dispatch (roff_term.c:45-58)
        // also clears the field's NOBREAK state.
        Some(Roff(RoffMacro::Nf | RoffMacro::Fi)) => {
            builder.fill_mode_boundary();
        }
        Some(Roff(RoffMacro::Sp)) => {
            builder.execute_spacing_request(layout::vertical_space_delta(node));
        }
        name if controls::formatter_control(name).is_some_and(|control| !control.specialized) => {
            match controls::formatter_control(name).map(|control| control.boundary) {
                Some(controls::FormatterBoundary::Line) => {
                    builder.control_line_break();
                }
                Some(controls::FormatterBoundary::NoBreak) => builder.no_break_flush(),
                Some(controls::FormatterBoundary::None) | None => {}
            }
        }
        name if controls::operand_control(name).is_some() => {}
        Some(Mdoc(MdocMacro::Ap)) => {
            builder.tighten_next_boundary();
            builder.append_text("'");
            builder.tighten_next_boundary();
        }
        _ => scopes::append(builder, node, default_name),
    }
}

fn execute_inline_macro_post(builder: &mut InlineBuilder, node: &Node) {
    if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Fd)) {
        // mdoc_term.c::print_mdoc_node() restores the font before running
        // termp_fd_post(). That post requests term_newln() for every sink,
        // even after \c. It does not run roff_pre_br(): NOBREAK field flags
        // survive; without buffered cells or an open device row, it emits
        // no physical line (term.c::term_newln,475-480).
        builder.execute_native_newline();
    }
}

fn execute_author_pre(builder: &mut InlineBuilder, node: &Node) -> bool {
    if node.macro_token.as_ref() != Some(&Mdoc(MdocMacro::An)) {
        return false;
    }
    builder.execute_author(node.author_mode);
    // mdoc_term.c::termp_an_pre() returns 0 for -split/-nosplit, suppressing
    // children even when malformed input supplied excess operands.
    node.author_mode.is_some()
}

fn finish_inline_node_execution(
    builder: &mut InlineBuilder,
    node: &Node,
    next: Option<&Node>,
    final_word_join_before: Option<bool>,
) {
    // A bare Fl followed by a callable macro on the same source line owns an
    // external join. An explicit empty operand is different: it consumes the
    // prefix's operand position and must leave that sibling separate.
    if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Fl))
        && node.children.is_empty()
        && next.is_some_and(|next| next.kind != NodeKind::Text && !next.flags.line_start)
    {
        builder.tighten_next_boundary();
    }
    if node.flags.delimiter_open {
        builder.tighten_next_boundary();
    }
    // The AST flag is only a fallback for scopes that lower through a private
    // builder.  A scope that executed its own final word (for example `.In`
    // closing with `>`) reports the result explicitly, and must not be
    // retightened from the source spelling.
    if node.flags.line_continuation && builder.final_word_join_state() == final_word_join_before {
        builder.tighten_next_boundary();
        builder.inherit_final_word_join(Some(true));
    }
}

/// Semantic transforms capture an output suffix before their first operand.
/// Plain font scopes and containers merely execute their child stream and
/// have no such replacement owner to prepare at macro entry.
fn prepares_semantic_output_owner(node: &Node) -> bool {
    matches!(
        node.macro_token.as_ref(),
        Some(
            Mdoc(
                MdocMacro::Lk
                    | MdocMacro::Mt
                    | MdocMacro::Sx
                    | MdocMacro::In
                    | MdocMacro::Bx
                    | MdocMacro::Xr
            ) | Man(ManMacro::Mr)
        )
    )
}
