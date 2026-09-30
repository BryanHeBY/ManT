// Copyright (c) 2010, 2012-2020, 2022, 2025, 2026
//               Ingo Schwarze <schwarze@openbsd.org>
// Copyright (c) 2008, 2009, 2010, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
// Copyright (c) 2013 Franco Fichtner <franco@lastsummer.de>
// Copyright (c) 2010-2020, 2022-23, 2025, 2026
//               Ingo Schwarze <schwarze@openbsd.org>
// Copyright (c) 2008-2012 Kristaps Dzonsons <kristaps@bsd.lv>
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

//! Display tab pre handlers shared by block and inline output destinations.

use libmandoc_rs::{DisplayKind, Node, NodeKind};

use super::InlineExecutionState;

/// Bl post resets columns only after its `term_newln()` has consumed pending
/// input (mdoc_term.c::termp_bl_post(),1144-1151). Both output destinations
/// call this same configuration phase after executing their native post.
pub(in crate::mandoc) fn exit_post(execution: &mut InlineExecutionState, node: &Node) {
    if node.kind == NodeKind::Block
        && node.macro_name.as_deref() == Some("Bl")
        && node.list_kind == Some(libmandoc_rs::NormalizedListKind::Column)
        && !node.flags.no_print
    {
        execution.reset_default_tabs();
    }
}

/// Apply tab configuration only after the caller has executed the preceding
/// native newline. Resetting before that flush would reinterpret its buffer.
pub(in crate::mandoc) fn enter_pre(execution: &mut InlineExecutionState, node: &Node) {
    if node.flags.no_print || node.scope_end.is_some() {
        return;
    }
    match (node.macro_name.as_deref(), node.kind) {
        // termp_d1_pre(): only BLOCK calls term_newln(), then installs T .5i
        // (mdoc_term.c:1324-1334). Its HEAD/BODY do not repeat the reset.
        (Some("D1" | "Dl"), NodeKind::Block) => execution.reset_default_tabs(),
        // termp_bd_pre(): BLOCK owns print_bvspace(), HEAD skips children,
        // and only a literal BODY installs T 8n (1431-1463). Unfilled BODY
        // preserves existing stops; Bd post never restores a former set.
        (Some("Bd"), NodeKind::Body) if node.display_kind == Some(DisplayKind::Literal) => {
            execution.set_literal_tabs();
        }
        _ => {}
    }
}
