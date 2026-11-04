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

//! Source-node SYNOPSIS entry, independent of its paragraph/literal owner.

use libmandoc_rs::{MacroToken::Mdoc, MdocMacro, Node, NodeKind};

use super::InlineBuilder;
use crate::mandoc::blocks::synopsis::{SynopsisBoundary, SynopsisToken, synopsis_boundary};

pub(in crate::mandoc) fn has_pre(node: &Node) -> bool {
    if node.flags.no_print || !node.flags.synopsis_pretty || node.scope_end.is_some() {
        return false;
    }
    match node.macro_token.as_ref() {
        Some(Mdoc(MdocMacro::Vt)) => matches!(node.kind, NodeKind::Block | NodeKind::Element),
        Some(Mdoc(MdocMacro::Fo)) => node.kind == NodeKind::Block,
        Some(Mdoc(
            MdocMacro::Ft | MdocMacro::Fd | MdocMacro::Cd | MdocMacro::In | MdocMacro::Fn,
        )) => node.kind == NodeKind::Element,
        // Nm calls synopsis_pre only at a nonempty HEAD, after direct
        // BLOCK children. names::append_head owns that distinct lifecycle.
        _ => false,
    }
}

/// CVS `mdoc_term.c::synopsis_pre` (1185-1228). The source dispatcher passes
/// the actual `roff_node_prev`, never a previous IR block. A block-routed node
/// executes here before choosing its destination, then enters the inline
/// dispatcher without a predecessor; nested inline nodes pass their own
/// siblings. Thus no destination replays the pre or records an event history.
pub(in crate::mandoc) fn enter(builder: &mut InlineBuilder, node: &Node, previous: Option<&Node>) {
    if !has_pre(node) {
        return;
    }
    let Some(previous) = previous else {
        return;
    };
    match synopsis_boundary(
        SynopsisToken::from_node(previous),
        SynopsisToken::from_node(node),
    ) {
        SynopsisBoundary::Newline => builder.execute_native_newline(),
        SynopsisBoundary::VerticalSpace => builder.native_vertical_space(1),
    }
}
