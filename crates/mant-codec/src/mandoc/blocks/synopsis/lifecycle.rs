//! Native HEAD field and BODY origin facts, separate from output ownership.

use libmandoc_rs::{MacroToken::Man, ManMacro, Node, NodeKind};

use crate::mandoc::inline::{AuthorBreakEffect, FieldFlag, FieldFlags};
use crate::mandoc::layout::{Distance, SourceIndent};
use crate::mandoc::{LoweringContext, first_part_children};

pub(super) fn flushes_head(node: &Node) -> bool {
    node.macro_token.as_ref() == Some(&Man(ManMacro::Sy))
        || !first_part_children(node, NodeKind::Body).is_empty()
}

/// `pre_SY` and `termp_nm_pre` measure the name without executing its words.
/// `width_sample` is the shared `term_strlen` escape/character-width reader.
pub(in crate::mandoc::blocks) fn head_width(node: &Node) -> usize {
    first_part_children(node, NodeKind::Head)
        .first()
        .map_or(1, |name| {
            name.text.as_deref().map_or(6, |text| {
                crate::mandoc::roff_escape::width_sample(text).saturating_add(1)
            })
        })
}

pub(super) fn head_effect(node: &Node) -> AuthorBreakEffect {
    if !flushes_head(node) {
        return AuthorBreakEffect::Line;
    }
    let man = node.macro_token.as_ref() == Some(&Man(ManMacro::Sy));
    let head = first_part_children(node, NodeKind::Head);
    let body = first_part_children(node, NodeKind::Body);
    let flags = if man {
        // man_term.c::pre_SY(HEAD): no trailspace, and NOBREAK is absent
        // when the BODY's first node is native no-fill.
        if body.first().is_some_and(|node| node.flags.no_fill) {
            FieldFlags::inset()
        } else {
            FieldFlags::new(&[FieldFlag::NoBreak], 0)
        }
    } else if head.len() > 1 || head.first().is_some_and(|node| node.kind != NodeKind::Text) {
        FieldFlags::hang()
    } else {
        // mdoc_term.c::termp_nm_pre(HEAD): NOBREAK|BRIND, trailspace 1.
        FieldFlags::diag()
    };
    let width = u16::try_from(head_width(node)).unwrap_or(u16::MAX);
    AuthorBreakEffect::Field {
        gap_cells: u8::from(!man),
        body_width_columns: width,
        field_width_columns: width,
        flags,
    }
}

pub(in crate::mandoc::blocks) fn body_origin(
    node: &Node,
    context: &LoweringContext<'_>,
    origin: SourceIndent,
) -> SourceIndent {
    context.offset_indent(
        node,
        origin,
        Distance::cells(i32::try_from(head_width(node)).unwrap_or(i32::MAX)),
    )
}
