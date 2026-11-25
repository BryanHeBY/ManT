//! Prove when buffered break markers can be replayed or must wait for a field flush.

use super::{
    EntryCursor, FieldCell, ReplayDomain, RoffInlineEvent, TextExecutionContext, can_place_pending,
    decorated_word, fits_bounded_filled_prefix, has_prior_retreat, is_bounded_filled_prefix_entry,
    is_filled_line_domain, is_fresh_word_entry, local_overwrite, marked_operands, motion_columns,
};

pub(super) enum MarkerReplay {
    PreserveSlots,
    AwaitFieldFlush,
}

/// `ESCAPE_BREAK` (`\p`) buffers `'\n'` without running `term_fill()` until
/// flush (`term.c:281-366, 659-660`); keep bounded marked words editable.
fn appends_marked_advance(events: &[RoffInlineEvent], context: &TextExecutionContext<'_>) -> bool {
    // Appending at lastcol does not need the earlier field bytes. Even with
    // an opaque earlier word, keep the full NBRSP run after the marker so a
    // later HORIZ can overwrite one slot without dropping the other slots.
    // term_word does not flush at ESCAPE_BREAK or mid-word ESCAPE_NOSPACE
    // (term.c:659-667, 678-699).
    let entry = &context.slot_entry;
    if entry.domain != ReplayDomain::Filled
        || entry.cursor != EntryCursor::AtEnd
        || entry.prefix.is_some()
        || context.zero_advance.is_armed()
        || context.zero_advance.has_pending_glyph()
        || context.zero_advance.has_annotation_output_owner()
        || !matches!(events.last(), Some(RoffInlineEvent::HorizontalMotion {
            argument: Some(value), ..
        }) if motion_columns(value).is_some_and(|(absolute, units)| !absolute && units > 0))
    {
        return false;
    }
    let mut saw_ink = false;
    let mut saw_break = false;
    let size = events.iter().try_fold(0usize, |size, event| {
        let added = match event {
            RoffInlineEvent::Text(text)
                if !saw_break && text.bytes().all(|b| b.is_ascii_graphic()) =>
            {
                saw_ink |= !text.is_empty();
                text.len().saturating_mul(5)
            }
            RoffInlineEvent::LineBreak if saw_ink => {
                saw_break = true;
                1
            }
            RoffInlineEvent::NoSpace if saw_break => 0,
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } if saw_break => {
                let (absolute, units) = motion_columns(value)?;
                if absolute || units <= 0 {
                    return None;
                }
                libmandoc_rs::TableColumnMetrics::relative_motion_columns(value)
            }
            _ => return None,
        };
        size.checked_add(added)
    });
    saw_break
        && size.is_some_and(|size| {
            size <= crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS
        })
}

fn retreats_within_marked_append(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    let entry = &context.slot_entry;
    if entry.domain != ReplayDomain::Filled
        || entry.cursor != EntryCursor::AtEnd
        || entry.prefix.is_some()
        || context.zero_advance.is_armed()
        || context.zero_advance.has_pending_glyph()
        || context.zero_advance.has_annotation_output_owner()
    {
        return false;
    }
    // The earlier opaque bytes need not be known if both the markers
    // and the retreat stay in the appended suffix. term_word() does
    // not flush the markers before processing HORIZ (term.c:659–718).
    let [
        body @ ..,
        RoffInlineEvent::LineBreak,
        RoffInlineEvent::NoSpace,
        RoffInlineEvent::HorizontalMotion {
            argument: Some(value),
            ..
        },
    ] = events
    else {
        return false;
    };
    let Some((false, units @ ..=-1)) = motion_columns(value) else {
        return false;
    };
    let mut markers = 1usize;
    let mut glyphs = 0usize;
    for event in body {
        match event {
            RoffInlineEvent::Text(text) if text.bytes().all(|b| b.is_ascii_graphic()) => {
                glyphs = glyphs.saturating_add(text.len());
            }
            RoffInlineEvent::LineBreak if glyphs > 0 => markers = markers.saturating_add(1),
            _ => return false,
        }
    }
    markers >= 2
        && glyphs >= 2
        && units.unsigned_abs() as usize <= markers.saturating_add(glyphs).saturating_mul(24)
        && glyphs.saturating_mul(5).saturating_add(markers)
            <= crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS
}

fn appends_marked_ink_to_scoped_tab_field(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    // A complete scoped field ending with TABREF remains editable when
    // a leading marker is followed by authored spaces and graph ink.
    // A later HORIZ can replace the LF before term_fill (term.c:659–718).
    let entry = &context.slot_entry;
    entry.domain == ReplayDomain::Scoped
        && entry.cursor == EntryCursor::AtEnd
        && !context.zero_advance.is_armed()
        && !context.zero_advance.has_pending_glyph()
        && !context.zero_advance.has_annotation_output_owner()
        && matches!(events, [RoffInlineEvent::LineBreak, RoffInlineEvent::Text(text)]
        if text.starts_with(' ') && text.bytes().all(|b| b == b' ' || b.is_ascii_graphic())
            && text.bytes().any(|b| b.is_ascii_graphic())
            && entry.prefix.as_ref().is_some_and(|prefix| {
                prefix.start == 0 && !prefix.backbefore && !prefix.backafter
                    && matches!(prefix.cells.last(), Some((FieldCell::TabReference, _)))
                    && prefix.raw_slot_count().saturating_add(text.len().saturating_mul(5)).saturating_add(1)
                        <= crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS
            }))
}

fn awaits_bounded_marker_flush(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
    fresh: bool,
) -> bool {
    appends_marked_advance(events, context)
        || appends_marked_ink_to_scoped_tab_field(events, context)
        || retreats_within_marked_append(events, context)
        || marked_operands::bounded_literal_control_append(events, context)
        || inherits_bounded_marked_backafter(events, context)
        || !context.zero_advance.is_armed()
            && if fresh {
                match context.slot_entry.domain {
                    ReplayDomain::Filled => {
                        marked_operands::bounded_append(events)
                            || marked_operands::bounded_filled_marked_append(events, 0)
                    }
                    ReplayDomain::Scoped => marked_operands::bounded_append(events),
                    ReplayDomain::DefinitionLine => {
                        marked_operands::bounded_field_append(events)
                            || marked_operands::bounded_append(events)
                            || marked_operands::bounded_definition_marked_head(events, context)
                            || context.slot_entry.definition_row_occupied
                                && starts_definition_with_graphless_marker(events)
                    }
                    ReplayDomain::LiteralDefinitionLine => false,
                }
            } else {
                is_bounded_filled_prefix_entry(events, context)
                    && context.slot_entry.prefix.as_ref().is_some_and(|prefix| {
                        marked_operands::bounded_filled_marked_append(
                            events,
                            prefix.raw_slot_count(),
                        )
                    })
            }
}

fn starts_definition_with_graphless_marker(events: &[RoffInlineEvent]) -> bool {
    // A graphless LF / NBRZW before an authored space in a fresh BODY
    // shares the live field with the first graph. A later HORIZ can
    // overwrite that byte before term_fill() (term.c:621–628, 659–718).
    matches!(events, [
        RoffInlineEvent::LineBreak, RoffInlineEvent::ZeroWidthGlyph, RoffInlineEvent::Text(text)
    ] if text.starts_with(' ')
        && text.bytes().all(|b| b == b' ' || b.is_ascii_graphic())
        && text.bytes().any(|b| b.is_ascii_graphic())
        && text.len().saturating_mul(5).saturating_add(2)
            <= crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS)
}

fn inherits_bounded_marked_backafter(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    // The first graph consumes inherited BACKAFTER; the marker uses
    // bufferc(), so the next graph still consumes BACKBEFORE (term.c).
    // Keep that pair and the terminal marker in the live buffer through
    // the continuation, instead of projecting the first graph as hidden.
    let [
        RoffInlineEvent::Text(first),
        RoffInlineEvent::LineBreak,
        RoffInlineEvent::Text(second),
        RoffInlineEvent::LineBreak,
        RoffInlineEvent::NoSpace,
    ] = events
    else {
        return false;
    };
    let first = first.strip_prefix(' ').unwrap_or(first);
    let graph = |text: &str| matches!(text.as_bytes(), [byte] if byte.is_ascii_graphic());
    graph(first)
        && graph(second)
        && context.zero_advance.is_armed()
        && !has_prior_retreat(context)
        && fits_bounded_filled_prefix(events, context)
        && context
            .slot_entry
            .prefix
            .as_ref()
            .is_some_and(|prefix| prefix.backafter && !prefix.backbefore)
}

fn allows_marker_motion(
    value: &str,
    index: usize,
    filled: bool,
    fresh: bool,
    domain: ReplayDomain,
) -> bool {
    filled && motion_columns(value).is_some()
        || fresh
            && domain == ReplayDomain::DefinitionLine
            && index == 0
            && motion_columns(value).is_some_and(|(absolute, units)| !absolute && units < 0)
}

fn is_trailing_no_space_marker(tail: &[RoffInlineEvent]) -> bool {
    tail.iter().all(|next| {
        matches!(
            next,
            RoffInlineEvent::LineBreak
                | RoffInlineEvent::NoSpace
                | RoffInlineEvent::Font(_)
                | RoffInlineEvent::PreviousFont
        )
    })
}

fn allows_structured_marker_text(
    text: &str,
    fresh_structured: bool,
    fresh: bool,
    domain: ReplayDomain,
    (owns_link, opens_link, filled, has_marker, has_backstep): (bool, bool, bool, bool, bool),
) -> bool {
    fresh_structured
        && text.chars().all(|ch| {
            ch == '\u{a0}'
                || ch == ' ' && (owns_link && filled || opens_link || has_marker && has_backstep)
                || ch.is_ascii_graphic()
        })
        || fresh
            && domain == ReplayDomain::DefinitionLine
            && !has_marker
            && text
                .bytes()
                .all(|byte| byte == b' ' || byte.is_ascii_graphic())
}

/// A marker sets breakline during the eventual `term_fill()`, while
/// `term_word()` can still overwrite raw slots on either side of it.
fn rewrites_marked_blank_tail(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    if !fits_bounded_filled_prefix(events, context)
        || context.zero_advance.has_buffered_glyph() && !can_place_pending(context)
        || context.zero_advance.has_annotation_output_owner()
        || !context.slot_entry.prefix.as_ref().is_some_and(|prefix| {
            prefix.cursor_retreat != 0 || prefix.backbefore || prefix.backafter
        })
    {
        return false;
    }
    // A retreated col is still live even when a new zero-advance
    // overprint writes a marker and then NBRSP. The marker is only a
    // byte until term_fill; the next operand can replace that byte.
    // See term.c:659-661, 678-698, 903-929.
    let ink = events.iter().position(|event| {
        !matches!(
            event,
            RoffInlineEvent::ZeroAdvance
                | RoffInlineEvent::Font(_)
                | RoffInlineEvent::PreviousFont
                | RoffInlineEvent::Breakpoint
        ) && !matches!(event, RoffInlineEvent::HorizontalMotion { argument: Some(value), .. } if is_forward_relative_motion(value))
    });
    let Some(
        [
            RoffInlineEvent::Text(text),
            RoffInlineEvent::HorizontalMotion {
                argument: Some(argument),
                ..
            },
            RoffInlineEvent::LineBreak,
            tail @ ..,
        ],
    ) = ink.map(|index| &events[index..])
    else {
        return false;
    };
    text.len() >= 2
        && text.bytes().all(|byte| byte.is_ascii_graphic())
        && is_forward_relative_motion(argument)
        && !tail.is_empty()
        && tail.iter().all(|event| {
            matches!(event, RoffInlineEvent::Glyph(text, _) if text.chars().all(|ch| ch == '\u{a0}'))
                || matches!(event, RoffInlineEvent::HorizontalMotion { argument: Some(value), .. } if is_forward_relative_motion(value))
        })
}

fn is_forward_relative_motion(value: &str) -> bool {
    motion_columns(value).is_some_and(|(absolute, units)| !absolute && units > 0)
}

pub(super) fn marker_replay(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> Option<MarkerReplay> {
    if rewrites_marked_blank_tail(events, context)
        || marked_operands::bounded_definition_marker_edit(events, context)
    {
        return Some(MarkerReplay::AwaitFieldFlush);
    }
    if context.zero_advance.has_buffered_glyph() {
        return None;
    }
    let fresh = is_fresh_word_entry(context);
    if awaits_bounded_marker_flush(events, context, fresh) {
        return Some(MarkerReplay::AwaitFieldFlush);
    }
    scan_marker_replay(events, context, fresh)
}

fn scan_marker_replay(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
    fresh: bool,
) -> Option<MarkerReplay> {
    let domain = context.slot_entry.domain;
    let bounded_filled =
        fresh && domain == ReplayDomain::Filled || is_bounded_filled_prefix_entry(events, context);
    let ends_with_no_space = matches!(events.last(), Some(RoffInlineEvent::NoSpace));
    let fresh_structured = bounded_filled
        || fresh
            && matches!(domain, ReplayDomain::DefinitionLine | ReplayDomain::Scoped)
            && decorated_word(events, context.font.current);
    // Plain source blanks already have an incremental pass receipt.
    // When they separate an annotation from surrounding text, keep the
    // blank and label in the editable slots until acceptance can decide
    // both their row and their visible ownership.
    let owns_link = events
        .iter()
        .any(|event| matches!(event, RoffInlineEvent::Link(_)));
    let opens_link = events
        .iter()
        .any(|event| matches!(event, RoffInlineEvent::Link(Some(_))));
    let mut has_ink = false;
    let mut has_marker = false;
    let mut ink_after_marker = false;
    let mut has_other_padding_controls = false;
    let mut has_backstep = false;
    let mut has_motion = false;
    for (index, event) in events.iter().enumerate() {
        match event {
            RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text, _)
                if text.bytes().all(|byte| byte.is_ascii_graphic()) =>
            {
                has_ink |= !text.is_empty();
                ink_after_marker |= has_marker && !text.is_empty();
            }
            RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text, _)
                if allows_structured_marker_text(
                    text,
                    fresh_structured,
                    fresh,
                    domain,
                    (
                        owns_link,
                        opens_link,
                        bounded_filled,
                        has_marker,
                        has_backstep,
                    ),
                ) =>
            {
                let ink = text.bytes().any(|byte| byte.is_ascii_graphic());
                has_ink |= ink;
                ink_after_marker |= has_marker && ink;
                has_other_padding_controls |= owns_link || text.contains('\u{a0}');
            }
            RoffInlineEvent::LineBreak => {
                has_marker = true;
                // A final marker in a moving operand or a `\c`-terminated
                // operand leaves its earlier marker and graph editable.
                // term_word() keeps the raw suffix available to the next
                // word until term_fill() (term.c:659-667).
                let trailing_no_space_marker = ends_with_no_space
                    && (bounded_filled || has_other_padding_controls)
                    && is_trailing_no_space_marker(&events[index + 1..]);
                ink_after_marker &= has_motion || trailing_no_space_marker;
            }
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } if allows_marker_motion(value, index, bounded_filled, fresh, domain) => {
                has_other_padding_controls = true;
                has_motion = true;
            }
            RoffInlineEvent::Breakpoint
            | RoffInlineEvent::BreakableHyphen
            | RoffInlineEvent::NonBreakingWordBlank
                if bounded_filled =>
            {
                has_other_padding_controls = true;
            }
            RoffInlineEvent::ZeroAdvance => has_backstep = true,
            RoffInlineEvent::NoSpace if is_filled_line_domain(domain) => {}
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::ZeroWidthGlyph => {}
            _ => return None,
        }
    }
    // A placed local backstep proves the raw word, including any source
    // SP the terminal preserves beside a marker (term.c:659-661,946-967).
    // Otherwise the existing glyph owner must carry the pending BACK
    // register through the next term_word().
    if has_backstep
        && !(bounded_filled && ends_with_no_space && !has_prior_retreat(context))
        && (has_other_padding_controls || local_overwrite(events, context).is_none())
    {
        return None;
    }
    let preserve_filled_marker =
        bounded_filled && (!has_backstep || ends_with_no_space) && (has_motion || has_backstep);
    if !has_marker || !has_ink || !ink_after_marker && !has_backstep && !preserve_filled_marker {
        return None;
    }
    Some(if ink_after_marker {
        MarkerReplay::AwaitFieldFlush
    } else {
        MarkerReplay::PreserveSlots
    })
}
