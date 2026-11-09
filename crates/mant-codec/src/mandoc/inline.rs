//! Lowers typed roff events and semantic mdoc macros into inline IR nodes.

use libmandoc_rs::{
    MacroToken,
    MacroToken::{Man, Mdoc},
    ManMacro, MdocMacro, Node, NodeKind,
};
use mant_ir::Inline;

pub(crate) use mant_ir::{inline_plain_text as plain_text, terms_fit_inline};

pub(in crate::mandoc) mod display_tabs;
mod equation;
mod flow;
mod font;
mod generated;
mod text_execution;
pub(in crate::mandoc) use generated::function_argument;
mod links;
pub(in crate::mandoc) mod names;
pub(in crate::mandoc) mod ownership;
pub(in crate::mandoc) use flow::split_output_scope_prefix;
pub(super) use links::{append_man_link, man_link_destination};
mod scopes;
mod source_fragment;
pub(in crate::mandoc) mod synopsis;
pub(in crate::mandoc) use flow::{
    AuthorBreakEffect, CompletedRowOrigin, DefinitionGeometryCheckpoint, FieldFlag, FieldFlags,
    HeadOperandCapture, InlineExecutionState, NamePartGeometry, NestedListScope, NoFillInlineState,
    OutputRowEnd, PreservedInlineState, RunInBoundaryReceipt, consume_one_row_ending,
    ends_with_executed_line_break, has_rendered_formatter_glyph,
    lower_no_fill_fragment_with_formatter, native_row_origin, prepare_inline_output,
    retain_inline_identities, split_row_origin, strip_native_projection_markers,
    take_definition_term_breaks, take_inline_layout, trailing_completed_row_origins,
};
pub(super) use flow::{FilledBoundary, FontScope, FontState, InlineBuilder};
mod node;
mod source;
mod word_input;
pub(super) use node::append_isolated_inline_node;
pub(in crate::mandoc) use node::{
    append_inline_node_with_siblings, append_inline_node_with_source,
};
pub(super) use word_input::append_normalized_word;
#[cfg(test)]
use word_input::append_text_node;

#[cfg(test)]
use font::parse_roff_text_with_font;
pub(super) use font::{
    TextExecutionContext, TextExecutionPolicy, ZeroAdvanceState, parse_roff_text_with_zero_advance,
};
pub(super) use font::{lower_inline_nodes_with_font_state, parse_roff_text};
pub(in crate::mandoc) use source_fragment::lower_source_fragment_with_formatter_state;

pub(super) use source::roff_macro_arguments;

use super::{
    first_part_children,
    roff_escape::{RoffFont as Font, RoffInlineEvent, decode, is_formatter_word_blank},
};

pub(super) fn lower_inline_nodes(nodes: &[Node], default_name: Option<&str>) -> Vec<Inline> {
    lower_inline_nodes_with_spacing(nodes, default_name, true)
}

pub(super) fn lower_inline_nodes_with_spacing(
    nodes: &[Node],
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    let mut builder = InlineBuilder::with_spacing(spacing_enabled);
    append_inline_nodes(&mut builder, nodes, default_name);
    builder.finish()
}

/// Derive an authored section phrase without inheriting terminal execution
/// state from the surrounding formatter.
///
/// CVS uses the same source-derived phrase for `Sh`/`Ss` identities and for
/// `Sx` destinations.  The terminal label is executed separately and may be
/// changed by persistent state such as `Sm off` or a preceding bare `\z`.
pub(super) fn authored_section_phrase(nodes: &[Node], default_name: Option<&str>) -> String {
    if nodes.iter().all(|node| node.kind == NodeKind::Text) {
        return nodes
            .iter()
            .filter_map(|node| node.text.as_deref())
            .filter_map(deroff_text_fragment)
            .collect::<Vec<_>>()
            .join(" ");
    }
    plain_text(&lower_inline_nodes(nodes, default_name))
        .trim()
        .to_owned()
}

/// Normalize one direct text child exactly like CVS `roff.c::deroff()`.
///
/// This is deliberately narrower than visible-text decoding.  Section
/// identities retain embedded escapes such as `A\zBC`; only leading spacing
/// escapes, a trailing continuation backslash, and surrounding ASCII
/// whitespace are removed from each authored fragment.
fn deroff_text_fragment(source: &str) -> Option<&str> {
    let bytes = source.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if bytes[start] == b'\\'
            && bytes
                .get(start + 1)
                .is_some_and(|next| b" %&0^|~".contains(next))
        {
            start += 2;
        } else if bytes[start].is_ascii_whitespace() {
            start += 1;
        } else {
            break;
        }
    }

    let mut end = bytes.len();
    if end > start && bytes[end - 1] == b'\\' {
        end -= 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    (start < end).then(|| &source[start..end])
}

#[cfg(test)]
mod authored_phrase_tests;

/// Apply one validated mdoc `Sm` state transition.
///
/// The same state machine is used for top-level filled flow and for nested
/// definition terms. Keeping it here prevents an `Sm` inside `Xo` from being
/// discarded merely because that subtree is lowered by an inline builder.
pub(super) fn updated_spacing(current: bool, setting: &str) -> bool {
    match setting {
        "on" => true,
        "off" => false,
        "" => !current,
        _ => current,
    }
}

/// Append sibling events without throwing away pending formatter effects.
pub(super) fn append_inline_nodes(
    builder: &mut InlineBuilder,
    nodes: &[Node],
    default_name: Option<&str>,
) {
    append_inline_sequence(builder, nodes, default_name, false);
}

pub(super) fn append_function_children(
    builder: &mut InlineBuilder,
    nodes: &[Node],
    default_name: Option<&str>,
) {
    append_inline_sequence(builder, nodes, default_name, true);
}

fn append_inline_sequence(
    builder: &mut InlineBuilder,
    nodes: &[Node],
    default_name: Option<&str>,
    function_body: bool,
) {
    for (node, siblings) in super::adjacency::with_siblings(nodes) {
        // mandoc joins the final pair in a contiguous mdoc bibliography
        // author run with "and". The conjunction is formatter-generated, so
        // it is not a child of either `%A` node and must be restored while the
        // sibling context is still available.
        if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::PercentA))
            && siblings.previous.is_some_and(|previous| {
                previous.macro_token.as_ref() == Some(&Mdoc(MdocMacro::PercentA))
            })
            && siblings
                .next
                .is_none_or(|next| next.macro_token.as_ref() != Some(&Mdoc(MdocMacro::PercentA)))
        {
            builder.append_text("and");
        }
        if function_body && node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Fa)) {
            let comma_after = !node.children.is_empty()
                && siblings
                    .next
                    .is_some_and(|next| next.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Fa)));
            generated::function_argument(builder, node, comma_after, default_name);
        } else {
            append_inline_node_with_source(builder, node, siblings, default_name);
        }
    }
}

pub(in crate::mandoc) fn is_configuration_equation(node: &Node) -> bool {
    node.kind == NodeKind::Equation
        && node.equation.as_ref().is_none_or(|equation| {
            equation.kind == libmandoc_rs::EquationKind::Text
                && equation.text.is_none()
                && equation.children.is_empty()
                && equation.position == libmandoc_rs::EquationPosition::None
                && equation.top.is_none()
                && equation.bottom.is_none()
        })
}

fn append_equation_node(builder: &mut InlineBuilder, node: &Node, next: Option<&Node>) {
    // Native node entry/post surround the actual eqn_box word stream once.
    if !node.flags.line_start {
        builder.tighten_next_boundary();
    }
    if !is_configuration_equation(node) {
        equation::append_owned(builder, node);
    }
    builder.release_next_boundary();
    if next.is_some_and(|next| !next.flags.line_start) {
        builder.tighten_next_boundary();
    }
}

/// Execute mdoc `.In` delimiters and operands in one formatter stream before
/// compacting the result into the renderer-neutral code span.  In particular,
/// the closing `>` is a real generated glyph: it can overstrike a preceding
/// zero-advance glyph just like CVS `term_word()` does.
pub(super) fn append_include(builder: &mut InlineBuilder, node: &Node, default_name: Option<&str>) {
    let children = inline_children(node);
    if let Some(anchor) = navigation_anchor(node) {
        builder.append(vec![anchor]);
    }
    let saved = builder
        .font
        .push_scope(if node.flags.synopsis_pretty && node.flags.line_start {
            Font::Strong
        } else {
            Font::Emphasis
        });
    // The code-span compaction flattens nested nodes, so delayed glyphs
    // from preceding source must leave this wrapper's output before it
    // runs: their own owner, style, and link identity cannot survive the
    // compaction, and the outer semantic checkpoint could only recover
    // plain text from the flattened value.
    let scope = builder.begin_output_checkpoint();
    builder.zero_advance.begin_output_owner();
    if node.flags.synopsis_pretty && node.flags.line_start {
        // termp_in_pre()1552-1558 writes two separate words, "#include"
        // and "<". The latter's automatic separator is its own native cell.
        builder.append_text("#include");
    }
    builder.append_text("<");
    builder.tighten_next_boundary();
    append_inline_nodes(builder, children, default_name);
    if node.flags.synopsis_pretty {
        builder.font.select(Font::Strong);
    }
    builder.font.pop_scope(saved);
    builder.tighten_next_boundary();
    builder.append_text(">");
    let membership = builder.zero_advance.end_output_owner();
    builder
        .zero_advance
        .bind_deferred(membership, &ownership::Annotation::Code, false);
    builder.wrap_output_since(scope, |nodes| {
        membership.map_owned(nodes, |rest| {
            let value = plain_text(&rest);
            let mut output = Vec::new();
            if !value.is_empty() {
                output.push(Inline::Code { value });
            }
            output
        })
    });
}

/// Whether a semantic macro owns an inline enclosure body.
///
/// Both implicit forms such as `Aq` and explicit block forms such as
/// `Ao`/`Ac` arrive as one opener-owned subtree after libmandoc validation.
/// `Eo` carries its delimiters in structural head and tail nodes, while the
/// obsolete `En` carries the state resolved from the preceding `Es` request.
pub(super) fn is_enclosure_macro(macro_name: Option<&MacroToken>) -> bool {
    macro_name.is_some_and(|name| enclosure_marks(name).is_some())
        || matches!(macro_name, Some(Mdoc(MdocMacro::Eo | MdocMacro::En)))
}

pub(super) fn enclosure_marks(name: &MacroToken) -> Option<(String, String)> {
    let glyph = |name: &str| catalog_glyph(name).to_string();
    match name {
        Mdoc(MdocMacro::Op | MdocMacro::Oo | MdocMacro::Bq | MdocMacro::Bo) => {
            Some(("[".to_owned(), "]".to_owned()))
        }
        // mdoc_term.c::termp_quote_pre/post emit these through the
        // device-independent character catalog, never literal Unicode.
        Mdoc(MdocMacro::Dq | MdocMacro::Do) => Some((glyph("lq"), glyph("rq"))),
        Mdoc(MdocMacro::Qq | MdocMacro::Qo) => Some(("\"".to_owned(), "\"".to_owned())),
        Mdoc(MdocMacro::Sq | MdocMacro::So | MdocMacro::Ql) => Some((glyph("oq"), glyph("cq"))),
        Mdoc(MdocMacro::Pq | MdocMacro::Po) => Some(("(".to_owned(), ")".to_owned())),
        Mdoc(MdocMacro::Brq | MdocMacro::Bro) => Some(("{".to_owned(), "}".to_owned())),
        Mdoc(MdocMacro::Aq | MdocMacro::Ao) => Some((glyph("la"), glyph("ra"))),
        _ => None,
    }
}

/// Resolve a formatter-generated glyph through the pinned character catalog
/// (vendor `chars.c`), mirroring upstream `term_word(p, "\\(name")`.
///
/// Only names that are stable in the pinned catalog may be requested; an
/// unknown name is a programming error, not an authored-input condition.
pub(super) fn catalog_glyph(name: &str) -> char {
    match libmandoc_rs::special_character(name) {
        Some(libmandoc_rs::SpecialCharacter::Visible(character)) => character,
        _ => panic!("pinned character catalog lost the generated glyph \\({name})"),
    }
}

/// Convert libmandoc's validated deep-link marker into a zero-width IR node.
/// Explicit `.Tg` requests retain their authored argument; automatically
/// discovered tags fall back to libmandoc's first printable source token.
fn navigation_anchor(node: &Node) -> Option<Inline> {
    super::targets::raw_target(node).map(|id| Inline::anchor_at(id, super::source_span(node)))
}

pub(super) fn inline_children(node: &Node) -> &[Node] {
    // A present empty body still owns the content. Falling back to Head and
    // Body wrappers would execute their enclosing macro a second time.
    node.children
        .iter()
        .find(|child| child.kind == NodeKind::Body)
        .map_or(&node.children, |body| &body.children)
}

pub(super) fn alternating_font_pair(macro_name: Option<&MacroToken>) -> Option<(Font, Font)> {
    match macro_name {
        Some(Man(ManMacro::Bi)) => Some((Font::Strong, Font::Emphasis)),
        Some(Man(ManMacro::Br)) => Some((Font::Strong, Font::Regular)),
        Some(Man(ManMacro::Ib)) => Some((Font::Emphasis, Font::Strong)),
        Some(Man(ManMacro::Ir)) => Some((Font::Emphasis, Font::Regular)),
        Some(Man(ManMacro::Rb)) => Some((Font::Regular, Font::Strong)),
        Some(Man(ManMacro::Ri)) => Some((Font::Regular, Font::Emphasis)),
        _ => None,
    }
}

/// Wrappers whose own formatter action only changes state while their
/// children remain the complete visible execution stream.
///
/// Consumers that inspect an executed prefix (for example a pending
/// definition head) may recurse through these nodes.  Generated-glyph
/// enclosures and atomic semantic macros are deliberately excluded.
#[cfg(test)]
fn text_node(value: &str) -> Vec<Inline> {
    vec![Inline::Text {
        value: value.to_owned(),
    }]
}

fn needs_boundary_space(left: Option<char>, right: Option<char>) -> bool {
    // A non-breaking space is word content on the incoming side (chars.c
    // NBRSP): term_word() still writes its automatic separator before it
    // (term.c:573-576). A trailing NBRSP already separated the previous
    // word, so no second boundary blank follows it.
    matches!(
        (left, right),
        (Some(left), Some(right))
            if !left.is_whitespace()
                && (!right.is_whitespace() || right == '\u{a0}')
    )
}

fn push_text(nodes: &mut Vec<Inline>, value: String) {
    if let Some(Inline::Text { value: previous }) = nodes.last_mut() {
        previous.push_str(&value);
    } else {
        nodes.push(Inline::Text { value });
    }
}

#[cfg(test)]
mod tests;
