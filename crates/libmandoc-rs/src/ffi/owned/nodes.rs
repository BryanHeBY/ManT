//! Syntax snapshots, checked discriminants and BODY identities.

use super::super::raw::{self, CDocument, CNode, CNodeView};
use super::{
    budget::{EquationBudget, TransferBudget},
    equations::copy_equation,
    strings::{checked_string, copy_column_strings, split_visible_text, visible_string},
    tables::{copy_table_cells, copy_table_rule_cells, table_row_kind},
};
use crate::{
    AuthorMode, DefinitionListStyle, DisplayKind, MacroSet, Node, NodeFlags, NodeKind,
    NormalizedEnclosure, NormalizedFont, NormalizedListKind, NormalizedSection, ScopeEnd,
};
use std::collections::HashMap;

const NODE_GENERATED: u32 = 1 << 0;
const NODE_SENTENCE_END: u32 = 1 << 1;
const NODE_NO_PRINT: u32 = 1 << 2;
const NODE_NO_FILL: u32 = 1 << 3;
const NODE_BROKEN: u32 = 1 << 11;
const NODE_DEEP_LINK_TARGET: u32 = 1 << 4;
const NODE_PERMALINK: u32 = 1 << 5;
const NODE_LINE_START: u32 = 1 << 6;
const NODE_DELIMITER_OPEN: u32 = 1 << 7;
const NODE_DELIMITER_CLOSE: u32 = 1 << 8;
const NODE_SYNOPSIS_PRETTY: u32 = 1 << 9;
const NODE_TABLE_START: u32 = 1 << 10;
const MAX_OWNED_NODE_DEPTH: usize = 256;

#[derive(Default)]
pub(super) struct IdentityState {
    next: u32,
    // Only BODY nodes can be targets of mdoc_endbody_alloc's `body` link.
    // Keep their borrowed addresses for this synchronous transfer alone.
    bodies: HashMap<*const CNode, u32>,
}

impl IdentityState {
    fn assign(&mut self, pointer: *const CNode, body: bool) -> Result<u32, String> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| "owned syntax identity limit exceeded".to_owned())?;
        if body {
            self.bodies.insert(pointer, self.next);
        }
        Ok(self.next)
    }

    fn closed_body(&self, pointer: *const CNode) -> Result<u32, String> {
        self.bodies.get(&pointer).copied().ok_or_else(|| {
            "libmandoc returned a body close marker without an owned body".to_owned()
        })
    }
}

pub(super) fn macro_set(value: i32) -> Result<MacroSet, String> {
    match value {
        0 => Ok(MacroSet::None),
        1 => Ok(MacroSet::Mdoc),
        2 => Ok(MacroSet::Man),
        _ => Err("libmandoc returned an unknown macro set".to_owned()),
    }
}

fn node_kind(value: i32) -> Result<NodeKind, String> {
    match value {
        0 => Ok(NodeKind::Root),
        1 => Ok(NodeKind::Block),
        2 => Ok(NodeKind::Head),
        3 => Ok(NodeKind::Body),
        4 => Ok(NodeKind::Tail),
        5 => Ok(NodeKind::Element),
        6 => Ok(NodeKind::Text),
        7 => Ok(NodeKind::Comment),
        8 => Ok(NodeKind::Table),
        9 => Ok(NodeKind::Equation),
        _ => Err("libmandoc returned an unknown node kind".to_owned()),
    }
}

fn normalized_section(value: i32) -> Result<NormalizedSection, String> {
    use NormalizedSection as S;
    Ok(match value {
        0 => S::None,
        1 => S::Name,
        2 => S::Library,
        3 => S::Synopsis,
        4 => S::Description,
        5 => S::Context,
        6 => S::Implementation,
        7 => S::ReturnValues,
        8 => S::Environment,
        9 => S::Files,
        10 => S::ExitStatus,
        11 => S::Examples,
        12 => S::Diagnostics,
        13 => S::Compatibility,
        14 => S::Errors,
        15 => S::SeeAlso,
        16 => S::Standards,
        17 => S::History,
        18 => S::Authors,
        19 => S::Caveats,
        20 => S::Bugs,
        21 => S::Security,
        22 => S::Custom,
        _ => return Err("libmandoc returned an unknown normalized section".to_owned()),
    })
}

fn list_kind(value: i32) -> Result<Option<NormalizedListKind>, String> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(NormalizedListKind::Bullet)),
        6 => Ok(Some(NormalizedListKind::Dash)),
        2 => Ok(Some(NormalizedListKind::Ordered)),
        3 => Ok(Some(NormalizedListKind::Definition)),
        4 => Ok(Some(NormalizedListKind::Column)),
        5 => Ok(Some(NormalizedListKind::Plain)),
        _ => Err("libmandoc returned an unknown list kind".to_owned()),
    }
}

fn definition_list_style(value: i32) -> Result<Option<DefinitionListStyle>, String> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(DefinitionListStyle::Tag)),
        2 => Ok(Some(DefinitionListStyle::Diagnostic)),
        3 => Ok(Some(DefinitionListStyle::Hang)),
        4 => Ok(Some(DefinitionListStyle::Inset)),
        5 => Ok(Some(DefinitionListStyle::Overhang)),
        _ => Err("libmandoc returned an unknown definition list style".to_owned()),
    }
}

fn display_kind(value: i32) -> Result<Option<DisplayKind>, String> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(DisplayKind::Literal)),
        2 => Ok(Some(DisplayKind::Filled)),
        3 => Ok(Some(DisplayKind::Unfilled)),
        _ => Err("libmandoc returned an unknown display kind".to_owned()),
    }
}

fn font_kind(value: i32) -> Result<Option<NormalizedFont>, String> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(NormalizedFont::Emphasis)),
        2 => Ok(Some(NormalizedFont::Literal)),
        3 => Ok(Some(NormalizedFont::Symbolic)),
        _ => Err("libmandoc returned an unknown normalized font".to_owned()),
    }
}

fn author_mode(value: i32) -> Result<Option<AuthorMode>, String> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(AuthorMode::Split)),
        2 => Ok(Some(AuthorMode::NoSplit)),
        _ => Err("libmandoc returned an unknown author mode".to_owned()),
    }
}

pub(super) unsafe fn copy_node(
    document: *mut CDocument,
    pointer: *const CNode,
    depth: usize,
    truncated: &mut bool,
    equation_budget: &mut EquationBudget,
    transfer_budget: &mut TransferBudget,
    identities: &mut IdentityState,
) -> Result<(Node, *const CNode), String> {
    // Keep native traversal state on the heap. A deep roff tree reaches the
    // 256-level copy limit without consuming a thread's small C/Rust stack.
    struct Frame {
        node: Node,
        child: *const CNode,
        depth: usize,
    }
    let (node, child, root_next) = unsafe {
        copy_node_shallow(
            document,
            pointer,
            equation_budget,
            transfer_budget,
            identities,
        )
    }?;
    let mut stack = vec![Frame { node, child, depth }];
    while let Some(mut frame) = stack.pop() {
        if !frame.child.is_null() && frame.depth + 1 < MAX_OWNED_NODE_DEPTH {
            let (node, child, next) = unsafe {
                copy_node_shallow(
                    document,
                    frame.child,
                    equation_budget,
                    transfer_budget,
                    identities,
                )
            }?;
            frame.child = next;
            let child_depth = frame.depth + 1;
            stack.push(frame);
            stack.push(Frame {
                node,
                child,
                depth: child_depth,
            });
            continue;
        }
        if !frame.child.is_null() {
            *truncated = true;
        }
        if let Some(parent) = stack.last_mut() {
            parent.node.children.push(frame.node);
        } else {
            return Ok((frame.node, root_next));
        }
    }
    Err("libmandoc returned an empty syntax traversal".to_owned())
}

unsafe fn copy_node_shallow(
    document: *mut CDocument,
    pointer: *const CNode,
    equation_budget: &mut EquationBudget,
    transfer_budget: &mut TransferBudget,
    identities: &mut IdentityState,
) -> Result<(Node, *const CNode, *const CNode), String> {
    let mut view = std::mem::MaybeUninit::<CNodeView>::uninit();
    if unsafe { raw::mant_mandoc_node_snapshot(document, pointer, view.as_mut_ptr()) } == 0 {
        return Err("libmandoc returned an invalid borrowed syntax node".to_owned());
    }
    let view = unsafe { view.assume_init() };
    let (text, native_text) = split_visible_text(unsafe { checked_string(view.text) }?);
    let line_continuation = text.as_deref().is_some_and(ends_with_no_space_escape);
    let enclosure_open = unsafe { checked_string(view.enclosure_open) }?;
    let enclosure_close = unsafe { checked_string(view.enclosure_close) }?;
    let kind = node_kind(view.kind)?;
    let scope_end = match view.end_kind {
        0 if view.end_body.is_null() => None,
        1 if !view.end_body.is_null() => Some(ScopeEnd {
            body_id: identities.closed_body(view.end_body)?,
        }),
        _ => return Err("libmandoc returned an invalid body close relation".to_owned()),
    };
    let node = Node {
        id: identities.assign(pointer, kind == NodeKind::Body && scope_end.is_none())?,
        kind,
        section: normalized_section(view.section)?,
        scope_end,
        reference_quotes_title: match view.reference_quotes_title {
            0 => false,
            1 => true,
            _ => return Err("libmandoc returned an invalid reference quote flag".to_owned()),
        },
        macro_name: unsafe { checked_string(view.macro_name) }?,
        text,
        native_text,
        tag: unsafe { visible_string(view.tag) }?,
        line: view.line.try_into().unwrap_or_default(),
        column: view.column.try_into().unwrap_or_default(),
        flow_epoch: view.flow_epoch,
        table_escape: u8::try_from(view.table_escape).ok(),
        table_source_recovery_safe: view.table_source_recovery_safe != 0,
        table_row_kind: table_row_kind(view.table_row_kind, unsafe {
            copy_table_rule_cells(document, view.table_rule_cells, transfer_budget)
        }?)?,
        flags: NodeFlags {
            generated: view.flags & NODE_GENERATED != 0,
            sentence_end: view.flags & NODE_SENTENCE_END != 0,
            no_print: view.flags & NODE_NO_PRINT != 0,
            no_fill: view.flags & NODE_NO_FILL != 0,
            broken: view.flags & NODE_BROKEN != 0,
            deep_link_target: view.flags & NODE_DEEP_LINK_TARGET != 0,
            permalink: view.flags & NODE_PERMALINK != 0,
            line_start: view.flags & NODE_LINE_START != 0,
            delimiter_open: view.flags & NODE_DELIMITER_OPEN != 0,
            delimiter_close: view.flags & NODE_DELIMITER_CLOSE != 0,
            synopsis_pretty: view.flags & NODE_SYNOPSIS_PRETTY != 0,
            table_start: view.flags & NODE_TABLE_START != 0,
            line_continuation,
        },
        list_kind: list_kind(view.list_kind)?,
        definition_list_style: definition_list_style(view.definition_list_style)?,
        display_kind: display_kind(view.display_kind)?,
        font: font_kind(view.font_kind)?,
        author_mode: author_mode(view.author_mode)?,
        enclosure: enclosure_open.map(|opening| NormalizedEnclosure {
            opening,
            closing: enclosure_close,
        }),
        compact: view.compact != 0,
        offset: unsafe { checked_string(view.offset) }?,
        width: unsafe { checked_string(view.width) }?,
        columns: unsafe { copy_column_strings(view.cols, view.ncols, transfer_budget) }?,
        table_cells: unsafe { copy_table_cells(document, view.table_cells, transfer_budget) }?,
        equation: unsafe { copy_equation(document, view.equation, 0, equation_budget) }?,
        children: Vec::new(),
    };

    let string_bytes = [
        node.macro_name.as_ref(),
        node.text.as_ref(),
        node.native_text.as_ref(),
        node.tag.as_ref(),
        node.offset.as_ref(),
        node.width.as_ref(),
    ]
    .into_iter()
    .flatten()
    .fold(0usize, |total, value| total.saturating_add(value.len()));
    transfer_budget.charge(std::mem::size_of::<Node>().saturating_add(string_bytes))?;

    Ok((node, view.child, view.next))
}

/// Match libmandoc's `man_hasc`: only an unescaped final `\c` continues the
/// input line. An odd number of immediately preceding backslashes escapes the
/// candidate backslash instead.
fn ends_with_no_space_escape(text: &str) -> bool {
    let bytes = text.as_bytes();
    let Some(prefix) = bytes.strip_suffix(br"\c") else {
        return false;
    };
    prefix
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 0
}
