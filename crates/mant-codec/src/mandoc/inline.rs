//! Lowers typed roff events and semantic mdoc macros into inline IR nodes.

use libmandoc_rs::{
    MacroToken,
    MacroToken::{Man, Mdoc, Roff},
    ManMacro, MdocMacro, Node, NodeKind, RoffMacro,
};
use mant_ir::Inline;

pub(crate) use mant_ir::{inline_plain_text as plain_text, terms_fit_inline};

pub(in crate::mandoc) mod display_tabs;
mod flow;
mod font;
mod generated;
pub(in crate::mandoc) use generated::function_argument;
mod links;
pub(super) use links::{append_man_link, man_link_identity_text};
mod scopes;
mod source_fragment;
pub(in crate::mandoc) use flow::{
    AuthorBreakEffect, CompletedRowOrigin, DefinitionGeometryCheckpoint, FieldFlag, FieldFlags,
    HeadOperandCapture, InlineExecutionState, NoFillInlineState, PreservedInlineState,
    consume_one_row_ending, ends_with_executed_line_break, has_rendered_formatter_glyph,
    lower_no_fill_fragment_with_formatter, native_row_origin, prepare_inline_output,
    retain_inline_identities, strip_native_projection_markers, trailing_completed_row_origins,
};
pub(super) use flow::{FilledBoundary, FontScope, FontState, InlineBuilder};
mod source;

use font::lower_man_font_scope;
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
    roff_escape::{
        RoffFont as Font, RoffInlineEvent, decode, is_formatter_word_blank, visible_text,
    },
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

pub(super) fn append_inline_node(
    builder: &mut InlineBuilder,
    node: &Node,
    default_name: Option<&str>,
) {
    append_inline_node_with_next(builder, node, None, default_name);
}

// Node entry, handler, and post have one shared execution sequence.
pub(super) fn append_inline_node_with_next(
    builder: &mut InlineBuilder,
    node: &Node,
    next: Option<&Node>,
    default_name: Option<&str>,
) {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        // Tg can recover an authored target even when native output is hidden.
        if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Tg)) {
            scopes::append(builder, node, default_name);
        }
        return;
    }
    if node.kind == NodeKind::Text && !node.flags.no_print {
        append_text_node(builder, node);
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
    if execute_author_pre(builder, node) {
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
        execute_inline_macro_handler(builder, node, next, default_name);
        builder.finish_semantic_owner_checkpoint(checkpoint);
        finish_inline_node_execution(builder, node, next, final_word_join_before);
        execute_inline_macro_post(builder, node);
        builder.restore_definition_geometry(geometry);
        return;
    }
    if node.flags.delimiter_close {
        builder.tighten_next_boundary();
    }
    execute_inline_macro_handler(builder, node, next, default_name);
    finish_inline_node_execution(builder, node, next, final_word_join_before);
    execute_inline_macro_post(builder, node);
    builder.restore_definition_geometry(geometry);
}

fn execute_inline_macro_handler(
    builder: &mut InlineBuilder,
    node: &Node,
    next: Option<&Node>,
    default_name: Option<&str>,
) {
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
            if let Some(target) = super::targets::raw_target(node) {
                builder.append(vec![Inline::anchor_at(target, super::source_span(node))]);
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
                builder.font.select(super::roff_escape::font(name));
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
            builder.temporary_indent();
        }
        // The request's own roff_term_pre_br() dispatch (roff_term.c:45-58)
        // also clears the field's NOBREAK state.
        Some(Roff(RoffMacro::Nf | RoffMacro::Fi)) => {
            builder.fill_mode_boundary();
        }
        Some(Roff(RoffMacro::Sp)) => {
            builder.execute_spacing_request(super::layout::vertical_space_delta(node));
        }
        name if super::controls::formatter_control(name)
            .is_some_and(|control| !control.specialized) =>
        {
            match super::controls::formatter_control(name).map(|control| control.boundary) {
                Some(super::controls::FormatterBoundary::Line) => {
                    builder.control_line_break();
                }
                Some(super::controls::FormatterBoundary::NoBreak) => builder.no_break_flush(),
                Some(super::controls::FormatterBoundary::None) | None => {}
            }
        }
        name if super::controls::operand_control(name).is_some() => {}
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

fn append_text_node(builder: &mut InlineBuilder, node: &Node) {
    builder.begin_executed_node(node);
    if node.flags.delimiter_close {
        builder.tighten_next_boundary();
    }
    let source = node.decoder_text().unwrap_or_default();
    let events = decode(source);
    if source.is_empty() && builder.visits_empty_text_as_space(node) {
        // man_term.c visits every empty TEXT through term_vspace(); mdoc_term.c
        // does so only for NODE_LINE. Neither path calls term_word(), so a
        // negative .sp debt remains available to cancel the requested row.
        builder.execute_visited_empty_text(node.flags.no_fill);
        return;
    }
    // `term_word()` consumes its inter-word boundary even for an explicit
    // empty operand. That word event can resolve a preceding `\\z` glyph
    // before generated enclosure punctuation is emitted. Control *nodes* are
    // routed separately and therefore do not gain this behavior.
    let has_glyph = events.iter().any(|event| match event {
        RoffInlineEvent::Text(value)
        | RoffInlineEvent::Glyph(value)
        | RoffInlineEvent::FallbackGlyph(value) => value
            .chars()
            .any(|character| !is_formatter_word_blank(character) && character != '\n'),
        RoffInlineEvent::BreakableHyphen
        | RoffInlineEvent::DeviceName
        | RoffInlineEvent::Overstrike { .. } => true,
        _ => false,
    });
    builder.begin_word_projection_with_break(
        true,
        !builder.in_definition_field() || has_glyph,
        starts_with_break_marker_blank(&events),
    );
    let pending_word_end_break = builder.take_word_end_break();
    // term_word() stores a previous operand's \p in the native buffer.
    // Its actual automatic separator is consumed by term_fill(), before
    // this operand's internal blanks. Do not replay that older marker in
    // the new TEXT projection: the field's ordered cell consumer decides
    // its row boundary (term.c:573-580,287-306).
    // The plain flush unit owns the same pass arithmetic as a definition
    // field (term.c runs one term_fill() over tcol->buf regardless of
    // authorship), so its marker blank defers to the shared cell consumer
    // instead of a local wipe decision. No-fill borrows the live output sink
    // until its native row retires; isolated table words retain their
    // independent cell scope.
    let field_authoritative = true;
    builder.begin_native_word_owner();
    // pre_alternate selects an operand role before this actual term_word;
    // internal font escapes remain within this same word identity.
    builder.record_current_native_operand(
        events
            .iter()
            .any(|event| matches!(event, RoffInlineEvent::Font(_))),
    );
    let execution = font::parse_roff_text_with_zero_advance(
        source,
        font::TextExecutionContext {
            font: &mut builder.execution.font,
            zero_advance: &mut builder.execution.zero_advance,
            pending_word_end_break: pending_word_end_break && !field_authoritative,
            policy: font::TextExecutionPolicy {
                recognize_generated_references: !node.flags.no_fill,
                record_native_cells: true,
                field_authoritative,
            },
        },
    );
    builder.ensure_definition_field_session();
    builder.escape_coverage.record(execution.escape_scan);
    builder.native_word_writes = Some(execution.native_writes);
    // mdoc_term gives an empty text node a vertical row only when the text
    // itself begins an input line. An empty No/Em argument does not, whereas
    // a buffered zero-width glyph (for example \&) still occupies that row.
    let occupies_literal_row = !execution.output.is_empty()
        || builder.zero_advance.has_buffered_glyph()
        || (node.flags.line_start && node.text.as_deref().is_some_and(str::is_empty))
        || events
            .iter()
            .any(|event| matches!(event, RoffInlineEvent::ZeroWidthGlyph));
    if execution.joins_preceding_node {
        builder.tighten_next_boundary();
        builder.note_zero_advance_join();
    }
    let provisional_definition_break = execution.pending_word_end_break
        && matches!(execution.output.last(), Some(Inline::LineBreak { .. }));
    builder.append_word_with_literal_row(
        execution.output,
        occupies_literal_row,
        execution.trailing_output,
    );
    if execution.definitive_reject {
        builder.note_definitive_word_rejection();
    }
    if execution.word_zero_graph {
        builder.note_row_zero_graph();
    }
    if provisional_definition_break {
        builder.note_provisional_definition_break();
    }
    if execution.pending_word_end_break {
        builder.request_word_end_break(execution.pending_word_end_break_separated);
    } else {
        builder.retain_buffered_field_word_end_break();
    }
    let continues_line = execution
        .source_continuation
        .unwrap_or(node.flags.line_continuation);
    if node.flags.delimiter_open || continues_line {
        builder.tighten_next_boundary();
    }
    builder.continue_source_line(continues_line);
}

/// Whether a decoded word begins with a `\p` marker whose following blank
/// precedes the word's first graph. Upstream, that blank is the retreat
/// target of an armed BACKBEFORE glyph (term.c:901-908), not the marker's
/// break cell; the word-boundary projection must not consume it early.
fn starts_with_break_marker_blank(events: &[RoffInlineEvent]) -> bool {
    let mut seen_marker = false;
    for event in events {
        match event {
            RoffInlineEvent::LineBreak => seen_marker = true,
            RoffInlineEvent::Text(value) => {
                for character in value.chars() {
                    if is_formatter_word_blank(character) {
                        return seen_marker;
                    }
                    if character != '\n' {
                        return false;
                    }
                }
            }
            RoffInlineEvent::Glyph(_)
            | RoffInlineEvent::BreakableHyphen
            | RoffInlineEvent::DeviceName
            | RoffInlineEvent::Overstrike { .. } => return false,
            // These use bufferc(ASCII_NBRZW), not encode1(), so they do
            // not consume BACKBEFORE before the later source blank
            // (term.c:610-638). Their graph role in term_fill is separate.
            _ => {}
        }
    }
    false
}

/// Append sibling events without throwing away pending formatter effects.
pub(super) fn append_inline_nodes(
    builder: &mut InlineBuilder,
    nodes: &[Node],
    default_name: Option<&str>,
) {
    for (index, node) in nodes.iter().enumerate() {
        // mandoc joins the final pair in a contiguous mdoc bibliography
        // author run with "and". The conjunction is formatter-generated, so
        // it is not a child of either `%A` node and must be restored while the
        // sibling context is still available.
        if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::PercentA))
            && index > 0
            && nodes[index - 1].macro_token.as_ref() == Some(&Mdoc(MdocMacro::PercentA))
            && nodes
                .get(index + 1)
                .is_none_or(|next| next.macro_token.as_ref() != Some(&Mdoc(MdocMacro::PercentA)))
        {
            builder.append_text("and");
        }
        append_inline_node_with_next(builder, node, nodes.get(index + 1), default_name);
    }
}

/// Generated equations have no inline child stream to execute.
fn lower_equation_node(node: &Node) -> Vec<Inline> {
    node.equation
        .as_ref()
        .map(crate::mandoc::equations::expression_from_ast)
        .map(|expression| (expression.readable_text(), expression))
        .filter(|(value, _)| !value.trim().is_empty())
        .map(|(value, expression)| vec![Inline::Equation { value, expression }])
        .unwrap_or_default()
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
    let entry_pending = builder.zero_advance.pending_visible_characters();
    let original = builder.zero_advance.pending_projection();
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
    let emitted = builder.zero_advance.end_output_owner();
    builder.wrap_output_since(scope, |nodes| {
        let (owned, rest) = InlineBuilder::split_delayed_glyph_prefix(
            nodes,
            entry_pending,
            emitted,
            original.as_ref(),
        );
        let value = plain_text(&rest);
        let mut output = owned;
        if !value.is_empty() {
            output.push(Inline::Code { value });
        }
        output
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
