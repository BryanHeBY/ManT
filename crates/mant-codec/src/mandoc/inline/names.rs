// Copyright (c) 2010, 2012-2020, 2022, 2025, 2026
//               Ingo Schwarze <schwarze@openbsd.org>
// Copyright (c) 2008, 2009, 2010, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
// Copyright (c) 2013 Franco Fichtner <franco@lastsummer.de>
//
// Permission to use, copy, modify, and distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

//! The native Nm HEAD/BODY phases, shared by structural and inline owners.

use libmandoc_rs::{Node, NodeKind};

use super::{Font, FontScope, InlineBuilder, NamePartGeometry, append_inline_nodes};
use crate::mandoc::blocks::synopsis::{
    SynopsisBoundary, SynopsisToken, lifecycle, synopsis_boundary,
};

#[derive(Clone, Copy)]
pub(in crate::mandoc) struct NameBodyScope {
    geometry: NamePartGeometry,
    font: FontScope,
}

/// BLOCK PREKEEP precedes even its direct punctuation children. Parts use
/// the ordinary node entry/exit and the same live word/field executor.
pub(super) fn append_block(
    builder: &mut InlineBuilder,
    node: &Node,
    previous: Option<&Node>,
    name: Option<&str>,
) {
    if let Some(anchor) = super::navigation_anchor(node) {
        builder.append(vec![anchor]);
    }
    builder.begin_definition_head_consumption();
    builder.enter_keep_words();
    for (index, part) in node.children.iter().enumerate() {
        match part.kind {
            NodeKind::Head => {
                append_head(
                    builder,
                    node,
                    part,
                    previous.map(SynopsisToken::from_node),
                    name,
                );
            }
            NodeKind::Body => {
                let saved = enter_body(builder, part, body_width(node, name));
                append_inline_nodes(builder, &part.children, name);
                finish_body(builder, part, saved);
            }
            _ => super::append_inline_node_with_next(
                builder,
                part,
                node.children.get(index + 1),
                name,
            ),
        }
    }
    builder.exit_keep_words();
}

pub(in crate::mandoc) fn append_head(
    builder: &mut InlineBuilder,
    block: &Node,
    head: &Node,
    previous: Option<SynopsisToken>,
    name: Option<&str>,
) -> bool {
    builder.begin_executed_node(head);
    let geometry = builder.name_part_geometry(head);
    let font = builder.font.checkpoint();
    if !head.children.is_empty() {
        // termp_nm_pre returns zero for an empty HEAD. For a real name,
        // synopsis_pre runs here, after the BLOCK's direct prefix.
        if (block.flags.synopsis_pretty || head.flags.synopsis_pretty)
            && let Some(previous) = previous
        {
            match synopsis_boundary(previous, SynopsisToken::Nm) {
                SynopsisBoundary::Newline => builder.execute_native_newline(),
                SynopsisBoundary::VerticalSpace => builder.native_vertical_space(1),
            }
        }
        builder.enter_name_head(lifecycle::head_effect(block));
        builder.font.push_scope(Font::Strong);
        append_inline_nodes(builder, &head.children, name);
    }
    builder.font.pop_scope(font);
    let occupied = if lifecycle::flushes_head(block) {
        builder.finish_name_head()
    } else {
        builder.definition_head_row_occupied()
    };
    builder.finish_executed_node(head);
    builder.restore_name_part_geometry(geometry);
    occupied
}

/// BODY width uses the original first HEAD string, or the parser's name
/// fallback; it is never measured from newly executed/styled output.
pub(in crate::mandoc) fn body_width(node: &Node, name: Option<&str>) -> usize {
    crate::mandoc::first_part_children(node, NodeKind::Head)
        .first()
        .and_then(|child| child.text.as_deref())
        .or(name)
        .map_or(6, |text| {
            crate::mandoc::roff_escape::width_sample(text).saturating_add(1)
        })
}

pub(in crate::mandoc) fn enter_body(
    builder: &mut InlineBuilder,
    body: &Node,
    width: usize,
) -> NameBodyScope {
    builder.begin_executed_node(body);
    let saved = NameBodyScope {
        geometry: builder.name_part_geometry(body),
        font: builder.font.checkpoint(),
    };
    if !body.children.is_empty() {
        builder.enter_name_body(width);
    }
    saved
}

pub(in crate::mandoc) fn finish_body(
    builder: &mut InlineBuilder,
    body: &Node,
    saved: NameBodyScope,
) {
    builder.font.pop_scope(saved.font);
    if !body.children.is_empty() {
        builder.flush_name_field();
    }
    builder.finish_executed_node(body);
    builder.restore_name_part_geometry(saved.geometry);
}
