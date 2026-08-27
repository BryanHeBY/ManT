//! First structural pass for the semantic mdoc(7) macro package.
//!
//! The roff executor owns source order and macro expansion. This pass gives
//! the initial M5 macro families their `Block`/`Head`/`Body` shape, records
//! metadata and normalized list/display/font attributes, and leaves unhandled
//! macros as ordinary elements for later incremental validation.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    AuthorMode, DisplayKind, MacroSet, NodeFlags, NodeId, NodeKind, NormalizedEnclosure,
    NormalizedFont, NormalizedListKind, SourceSpan,
    ast::{DocumentBuilder, MdocListMarker},
};

mod driver;
mod lists;
use lists::{
    active_column_item, active_column_list, append_column_ta_cell,
    append_implicit_column_table_row, broken_item_recoveries, column_item_cell_count,
    discard_item_body, extend_pending_short_column_item, finalize_last_empty_column_item,
    finalize_last_fixed_head_list_item, finalize_short_column_items, fixed_head_list_type,
    is_implicit_column_row_macro, item_header_partial_scope, make_block, make_synthetic_block,
    move_initial_list_content_out, split_column_item_cells, structure_implicit_column_item,
    structure_implicit_column_table_item, take_inline_column_ta_tail,
};
mod generated;
use generated::{
    clear_quoted_bx_trailing_delimiter_sentence_end, column_system_name_starts_next_element,
    expand_standard_exit_status, expand_standard_return_value, generated_system_name,
    insert_generated_ar_default, insert_generated_nm_name, insert_generated_nonbreaking_default,
    insert_generated_system_name, insert_generated_system_names, is_legacy_roff_font,
    push_generated_text, push_generated_text_at, system_macro_name,
};
mod blocks;
use blocks::{
    append_broken_full_block_body, append_broken_implicit_block_body, append_to_parent,
    argument_location, complete_explicit_tail, discard_empty_block, discard_node_from_parent,
    discard_previous_paragraph_control, first_mdoc_content_node, implicit_partial_ancestor_blocks,
    node_kind_name, normalize_inline_paragraph_controls,
    normalize_list_trailing_paragraph_controls, normalize_section_paragraph_boundaries,
    normalize_trailing_no_space_in_implicit_blocks, paragraph_layout_recovery_offset,
    recover_unmatched_ec, take_trailing_line_start_text_children,
};
mod inline;
pub(crate) use inline::is_mdoc_callable_macro;
use inline::{
    append_explicit_partial_tail, clear_initial_implicit_body_delimiter_flags,
    clear_leading_explicit_partial_punctuation, clear_terminal_implicit_body_opening_flags,
    explicit_partial_block_close, explicit_partial_tail_events, implicit_partial_block_name,
    is_implicit_partial_block_macro, is_inline_mdoc_macro, is_mdoc_closing_delimiter,
    is_mdoc_middle_delimiter, is_mdoc_noncallable_macro, mark_implicit_partial_tail_sentence_ends,
    mark_opening_delimiter, matching_explicit_partial_close_index, mdoc_inline_argument_limit,
    move_explicit_leading_open_delimiter, move_leading_open_delimiter,
    move_leading_open_delimiters, move_paragraph_permalink, no_space_macro_requires_warning,
    split_explicit_partial_block_tail, split_inline_macro_events, split_mdoc_inline_children,
    split_mdoc_inline_tokens, split_mdoc_inline_tokens_with_options,
    structure_item_head_explicit_partial, structure_matched_explicit_partial_blocks,
    structure_nested_implicit_explicit_scopes, structure_nested_implicit_partial_blocks,
    structure_unclosed_explicit_partial_blocks, tag_empty_macro_requires_warning,
    take_implicit_partial_tail, transfer_line_start,
};
mod validate;
use validate::{
    empty_tag_macro_name, flush_pending_authors_section, flush_pending_name_section,
    flush_pending_nd_delimiters, is_tag_style_delimiter_restart_macro,
    mark_link_terminal_delimiter, node_arguments, rebase_expanded_argument_locations,
    rebase_option_expansion_locations, standard_description, tag_macro_name, text_offset_location,
    validate_an, validate_at, validate_function_argument_commas, validate_function_name,
    validate_library, validate_no_break_trailing_delimiter, validate_prefix_following,
    validate_tag,
};
mod normalize;
use normalize::{
    coalesce_adjacent_text_children, coalesce_implicit_partial_body_text,
    coalesce_mdoc_display_phrases, coalesce_text_children, coalesce_text_children_after,
    collapse_long_option_prefixes, expand_fl_elements,
    relocate_crossed_closer_to_nested_implicit_body,
};
mod metadata;
use metadata::{
    clear_generated_synopsis_pretty_children, mark_no_print, mark_sentence_end,
    mark_synopsis_pretty, mdoc_operating_system_flavour, record_date, record_name,
    record_operating_system, record_title,
};
mod attributes;
use attributes::{
    apply_attributes, apply_presentation_flags, display_attributes, font_attributes, is_bf_option,
    list_attributes, normalize_filled_blank_lines, suppress_filled_c_blank_lines,
    trim_mdoc_filled_text_trailing_whitespace,
};
mod tags;
use tags::{
    automatic_mdoc_function_tag, default_volume, emphasis_fallback_elements, inline_target_name,
    mark_definition_item_head_targets, mark_definition_item_xo_head_targets, mark_destination,
    mark_emphasis_targets, mark_first_visible_permalink, mark_manual_target, mark_permalink,
    mark_section_targets, mark_target, mark_unique_function_targets, visible_head_text,
};
mod recovery;
pub(crate) use driver::structure;
pub(crate) use recovery::Recovery;

#[cfg(test)]
mod tests;

/// Bounded semantic-restructuring result consumed by the parser boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StructureOutcome {
    /// First source location whose `Head`/`Body` pair could not fit.
    pub(crate) node_limit_location: Option<SourceSpan>,
    /// Recoverable mdoc scope findings retained in source order.
    pub(crate) recoveries: Vec<Recovery>,
}

#[derive(Clone, Copy)]
enum ArgumentPlacement {
    Head,
    Body,
    /// Keep scanner-level token boundaries for a body that subsequently
    /// applies mdoc's nested inline-call grammar.
    BodyTokens,
    Drop,
}

#[allow(clippy::struct_excessive_bools)] // Each flag retains a distinct terminal list/display provenance.
#[derive(Clone, Default)]
struct BlockAttributes {
    list_kind: Option<NormalizedListKind>,
    /// Exact selected marker, retained only for native renderer output.
    list_marker: Option<MdocListMarker>,
    /// `Bl -hang` shares the public Definition projection with `-tag`, but
    /// retains a distinct terminal first-line field rule.
    terminal_hanging_list: bool,
    /// `Bl -ohang` shares the public Definition projection with `-tag`, but
    /// renders its Head and Body as separate equally indented terminal lines.
    terminal_overhanging_list: bool,
    /// `Bl -inset` shares the public Definition projection with `-tag`, but
    /// begins terminal Body content directly after its term.
    terminal_inset_list: bool,
    /// `Bl -diag` shares the public Definition projection with `-tag`, but
    /// uses a bold terminal term and a two-cell Body gap.
    terminal_diagnostic_list: bool,
    /// Selected mdoc list selector without its leading dash.
    list_type: &'static str,
    /// Number of declaration phrases following `Bl -column`.
    column_count: Option<usize>,
    /// Declaration phrases retained only for native terminal column layout.
    ///
    /// They are deliberately absent from the normalized public list node:
    /// libmandoc's owned AST exposes the `Column` behavior, not its discarded
    /// input labels.  The terminal device still needs their display widths.
    column_widths: Vec<String>,
    display_kind: Option<DisplayKind>,
    /// `-literal` and `-unfilled` share the public normalized display kind,
    /// but the terminal device assigns their tabs differently.
    literal_display: bool,
    /// `-centered` is publicly a filled display, but its terminal device
    /// field centers each completed visual line.
    centered_display: bool,
    font: Option<NormalizedFont>,
    compact: bool,
    offset: Option<String>,
    width: Option<String>,
}

/// Width retention is more specific than the public normalized list kind:
/// several mdoc list forms lower to `Definition` but have distinct layout
/// validation rules.
#[derive(Clone, Copy)]
enum ListWidthRule {
    /// Discard an authored width and provide no default.
    Drop,
    /// Retain an authored width, but provide no default.
    Retain,
    /// Warn that a `-tag` list uses the formatter's private `6n` default.
    DefaultSix,
    /// Use `2n` when no width was authored.
    DefaultTwo,
    /// Use `3n` when no width was authored.
    DefaultThree,
}

#[derive(Clone, Copy)]
struct ScopeFrame {
    close: &'static str,
    open: NodeId,
    body: NodeId,
    /// Whether this scope materializes a third `Tail` child only when its
    /// matching closer arrives.  Unclosed Eo blocks intentionally retain only
    /// Head and Body, as in the legacy owned AST.
    tail_on_close: bool,
    /// A function block accepts only its first transparent `.Tg` as an
    /// automatic destination.
    transparent_target_taken: bool,
    /// A cross-line explicit opener created in the tail of an already-crossed
    /// block is structurally nested, but its own closer is not another break
    /// of that historical implicit ancestor.
    suppress_implicit_ancestor_break: bool,
    resume_active: NodeId,
    resume_flow: NodeId,
}

fn close_name(value: &str) -> &'static str {
    match value {
        "Ac" => "Ac",
        "Bc" => "Bc",
        "Brc" => "Brc",
        "Dc" => "Dc",
        "Ec" => "Ec",
        "Ek" => "Ek",
        "El" => "El",
        "Ed" => "Ed",
        "Ef" => "Ef",
        "Fc" => "Fc",
        "Oc" => "Oc",
        "Pc" => "Pc",
        "Qc" => "Qc",
        "Re" => "Re",
        "Sc" => "Sc",
        "Xc" => "Xc",
        _ => unreachable!("only known mdoc closers reach this helper"),
    }
}

fn is_explicit_partial_close(value: &str) -> bool {
    matches!(
        value,
        "Ac" | "Bc" | "Brc" | "Dc" | "Oc" | "Pc" | "Qc" | "Sc"
    )
}

/// Whether a retained scope is an explicit partial block.  Eo is exceptional:
/// its `.Ec` materializes a Tail and therefore is not listed with the simple
/// Ac/Bc/… close macros, but it follows the same broken-nesting recovery.
fn is_explicit_partial_scope(frame: &ScopeFrame) -> bool {
    frame.tail_on_close || is_explicit_partial_close(frame.close)
}

/// Return the conventional manual-section restriction for a named `.Sh`.
/// This is the deliberately finite `post_sh_head()` subset whose condition is
/// independent of section ordering and body validation. Like libmandoc, the
/// first byte of composite manual sections (for example `3p`) controls it.
fn unexpected_section_manuals(section: &str, manual_section: Option<&str>) -> Option<&'static str> {
    let manual = manual_section?.as_bytes().first().copied()?;
    if section == "ERRORS" {
        return (!matches!(manual, b'2' | b'3' | b'4' | b'9')).then_some("2, 3, 4, 9");
    }
    if matches!(section, "RETURN VALUES" | "LIBRARY") {
        return (!matches!(manual, b'2' | b'3' | b'9')).then_some("2, 3, 9");
    }
    (section == "CONTEXT" && manual != b'9').then_some("9")
}

/// Return conventional mdoc section rank and canonical spelling. This order
/// is deliberately the upstream `enum roff_sec` order used by `post_sh_head`.
fn named_mdoc_section(section: &str) -> Option<(u8, &'static str)> {
    match section {
        "NAME" => Some((1, "NAME")),
        "LIBRARY" => Some((2, "LIBRARY")),
        "SYNOPSIS" => Some((3, "SYNOPSIS")),
        "DESCRIPTION" => Some((4, "DESCRIPTION")),
        "CONTEXT" => Some((5, "CONTEXT")),
        "IMPLEMENTATION NOTES" => Some((6, "IMPLEMENTATION NOTES")),
        "RETURN VALUES" => Some((7, "RETURN VALUES")),
        "ENVIRONMENT" => Some((8, "ENVIRONMENT")),
        "FILES" => Some((9, "FILES")),
        "EXIT STATUS" => Some((10, "EXIT STATUS")),
        "EXAMPLES" => Some((11, "EXAMPLES")),
        "DIAGNOSTICS" => Some((12, "DIAGNOSTICS")),
        "COMPATIBILITY" => Some((13, "COMPATIBILITY")),
        "ERRORS" => Some((14, "ERRORS")),
        "SEE ALSO" => Some((15, "SEE ALSO")),
        "STANDARDS" => Some((16, "STANDARDS")),
        "HISTORY" => Some((17, "HISTORY")),
        "AUTHORS" => Some((18, "AUTHORS")),
        "CAVEATS" => Some((19, "CAVEATS")),
        "BUGS" => Some((20, "BUGS")),
        "SECURITY CONSIDERATIONS" => Some((21, "SECURITY CONSIDERATIONS")),
        _ => None,
    }
}

/// Recover literal tabs in filled `.Sh` arguments after earlier section
/// validation. Scanner diagnostics normally precede structural lowering, but
/// libmandoc reports the preceding duplicate/order finding before tabs in a
/// later section heading; retaining this source-local recovery preserves that
/// public ordering.
fn mdoc_heading_tab_recoveries(builder: &DocumentBuilder, node: NodeId) -> Vec<Recovery> {
    builder
        .children(node)
        .into_iter()
        .flatten()
        .flat_map(|argument| {
            let Some(location) = builder.node_location(*argument) else {
                return Vec::new();
            };
            builder
                .node_text(*argument)
                .into_iter()
                .flat_map(|text| {
                    text.bytes()
                        .enumerate()
                        .filter(|(_, byte)| *byte == b'\t')
                        .filter_map(|(offset, _)| {
                            let offset = u32::try_from(offset).ok()?;
                            let start = location.start.checked_add(offset)?;
                            let location =
                                SourceSpan::new(location.source, start, start.saturating_add(1))
                                    .ok();
                            Some(Recovery::FilledTextTab { location })
                        })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn close_explicit_partial_scope(
    scopes: &mut Vec<ScopeFrame>,
    implicitly_closed: &mut Vec<&'static str>,
    active_body: &mut NodeId,
    flow_parent: &mut NodeId,
    close: &str,
) {
    if let Some(index) = scopes.iter().rposition(|frame| frame.close == close) {
        let frame = scopes[index];
        implicitly_closed.extend(scopes[index + 1..].iter().map(|frame| frame.close));
        scopes.truncate(index);
        *active_body = frame.resume_active;
        *flow_parent = frame.resume_flow;
    } else if let Some(index) = implicitly_closed
        .iter()
        .rposition(|implicit| *implicit == close)
    {
        implicitly_closed.remove(index);
    } else {
        // Unlike the full-block closer family, an explicit partial closer
        // can be consumed by the surrounding parsed macro without opening a
        // public scope (for example `Bc` following a column `It`).  The
        // legacy parser leaves that inert syntax diagnostic-free.
    }
}

/// A tail on an authored closer request starts a fresh source-line event.
/// Same-line tails of an opener and inline `No`/`Fl` close arguments retain
/// their existing source position instead.
fn mark_explicit_partial_close_tail_line_start(builder: &mut DocumentBuilder, events: &[NodeId]) {
    let Some(first) = events.first().copied() else {
        return;
    };
    if builder.node_macro_name(first).is_some() {
        return;
    }
    if let Some(mut flags) = builder.node_flags(first) {
        flags.line_start = true;
        let _ = builder.set_node_flags(first, flags);
    }
}

/// Remove a surrounding explicit-partial closer that a callable macro holds
/// in its direct argument stream.  The caller attaches the retained prefix
/// first, then restores the enclosing flow through `close_explicit_partial_scope`.
fn take_explicit_partial_close_argument(
    builder: &mut DocumentBuilder,
    node: NodeId,
    scopes: &[ScopeFrame],
) -> Option<(&'static str, Vec<NodeId>)> {
    let close = scopes
        .last()
        .map(|frame| frame.close)
        .filter(|close| is_explicit_partial_close(close))?;
    let arguments = builder.children(node)?.to_vec();
    if let Some(index) = arguments
        .iter()
        .position(|argument| builder.node_text(*argument) == Some(close))
    {
        let _ = builder.replace_children(node, &arguments[..index]);
        return Some((close, arguments[index + 1..].to_vec()));
    }

    // `No` coalesces adjacent source words before the parent scope machine
    // sees them.  Preserve the retained phrase but split an exact final close
    // word back out of that one semantic text node.
    let last = *arguments.last()?;
    let text = builder.node_text(last)?.to_owned();
    let prefix = text.strip_suffix(close)?.trim_end_matches([' ', '\t']);
    if prefix.len().saturating_add(close.len()) == text.len() {
        return None;
    }
    if prefix.is_empty() {
        let _ = builder.replace_children(node, &arguments[..arguments.len().saturating_sub(1)]);
    } else {
        let _ = builder.text(last, prefix.to_owned());
    }
    Some((close, Vec::new()))
}

/// Macros currently implementing `post_tg()`'s explicit-destination cases.
/// Keep this gate narrow: a pending `.Tg` must never silently cross an
/// unrelated source event while additional structural families are staged.
fn accepts_pending_manual_tag(macro_name: Option<&str>) -> bool {
    matches!(
        macro_name,
        Some(
            "Pp" | "Lp"
                | "Tg"
                | "Bl"
                | "Bd"
                | "D1"
                | "Dl"
                | "Fn"
                | "Fo"
                | "Fc"
                | "Rs"
                | "Sh"
                | "Ss"
                | "Fl"
                | "Cm"
                | "Dv"
                | "Em"
                | "Er"
                | "Ev"
                | "Ic"
                | "Li"
                | "Ms"
                | "No"
                | "Sy"
        )
    )
}

/// Return the immediately preceding paragraph boundary eligible for a
/// following standalone `.Tg` destination.  Do not let a tag cross visible
/// text or an unrelated semantic element: `post_tg()` only borrows the
/// paragraph it directly follows.
fn preceding_manual_tag_paragraph(
    builder: &DocumentBuilder,
    parent: NodeId,
    tag_node: NodeId,
) -> Option<NodeId> {
    let children = builder.children(parent)?;
    let position = children.iter().position(|node| *node == tag_node)?;
    for node in children[..position].iter().rev() {
        match builder.node_macro_name(*node) {
            Some("Pp" | "Lp") => return Some(*node),
            Some("Tg") => {}
            _ => return None,
        }
    }
    None
}

/// Bibliographic fields accepted inside an mdoc `Rs` reference block.
///
/// The scanner retains each control-line word independently for roff
/// execution, but the mdoc end-of-line validator exposes every direct text
/// run as one phrase in the public AST.
fn is_reference_field_macro(name: &str) -> bool {
    reference_field_order(name).is_some()
}

/// Return the validator's stable ordering for a direct `Rs` child.
///
/// Invalid children have no order and therefore sort before bibliography
/// fields; equal entries retain authored order.  This mirrors libmandoc's
/// insertion sort in `post_rs()`.
fn reference_field_order(name: &str) -> Option<u8> {
    Some(match name {
        "%A" => 1,
        "%T" => 2,
        "%B" => 3,
        "%I" => 4,
        "%J" => 5,
        "%R" => 6,
        "%N" => 7,
        "%V" => 8,
        "%U" => 9,
        "%P" => 10,
        "%Q" => 11,
        "%C" => 12,
        "%D" => 13,
        "%O" => 14,
        _ => return None,
    })
}

/// Whether the `in_line_eoln` grammar for this bibliography field has the
/// upstream `MDOC_JOIN` flag.
fn reference_field_joins_arguments(name: &str) -> bool {
    matches!(
        name,
        "%A" | "%B" | "%C" | "%D" | "%I" | "%J" | "%O" | "%Q" | "%R" | "%T"
    )
}

/// Apply mdoc's `post_rs()` direct-child order after a reference scope ends.
fn normalize_reference_field_order(builder: &mut DocumentBuilder, body: NodeId) {
    let Some(children) = builder.children(body) else {
        return;
    };
    let mut ordered = children.to_vec();
    ordered.sort_by_key(|node| {
        builder
            .node_macro_name(*node)
            .and_then(reference_field_order)
            .unwrap_or_default()
    });
    if ordered != children {
        let _ = builder.replace_children(body, &ordered);
    }
}

fn open_name(value: &str) -> &'static str {
    match value {
        "Ac" => "Ao",
        "Bc" => "Bo",
        "Brc" => "Bro",
        "Dc" => "Do",
        "Ec" => "Eo",
        "Ek" => "Bk",
        "El" => "Bl",
        "Ed" => "Bd",
        "Ef" => "Bf",
        "Fc" => "Fo",
        "Oc" => "Oo",
        "Pc" => "Po",
        "Qc" => "Qo",
        "Re" => "Rs",
        "Sc" => "So",
        "Xc" => "Xo",
        _ => unreachable!("only known mdoc scope closers are retained"),
    }
}
