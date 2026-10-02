//! Immediate borrowed snapshot to owned Rust AST transfer.
use super::{
    raw::{
        self, CDocument, CEquationBox, CEquationBoxView, CNode, CNodeView, CTableCell,
        CTableCellView, CTableRuleCell, CTableRuleCellView,
    },
    session::DocumentHandle,
};
use crate::{
    AuthorMode, DefinitionListStyle, DisplayKind, Document, EquationBox, EquationFont,
    EquationKind, EquationPosition, MacroSet, Metadata, Node, NodeFlags, NodeKind,
    NormalizedEnclosure, NormalizedFont, NormalizedListKind, NormalizedSection, RawDocument,
    ScopeEnd, TableAlignment, TableCell, TableCellKind, TableFont, TableRowKind, TableRuleCellKind,
};
use std::{collections::HashMap, ffi::CStr, os::raw::c_char, ptr::NonNull};
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
const MAX_OWNED_EQUATION_DEPTH: usize = 256;
const MAX_OWNED_EQUATION_NODES: usize = 50_000;
const MAX_OWNED_EQUATION_BYTES: usize = 2 * 1024 * 1024;
const MAX_OWNED_SYNTAX_ITEMS: usize = 250_000;
const MAX_OWNED_SYNTAX_BYTES: usize = 128 * 1024 * 1024;

#[derive(Default)]
struct TransferBudget {
    items: usize,
    bytes: usize,
}

impl TransferBudget {
    fn charge(&mut self, bytes: usize) -> Result<(), String> {
        self.items = self.items.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
        if self.items > MAX_OWNED_SYNTAX_ITEMS || self.bytes > MAX_OWNED_SYNTAX_BYTES {
            return Err(format!(
                "owned syntax transfer exceeded its cumulative node/byte budget ({} items, {} bytes)",
                self.items, self.bytes
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
struct EquationBudget {
    nodes: usize,
    bytes: usize,
    truncated: bool,
}

#[derive(Default)]
struct IdentityState {
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

pub(super) fn copy_document(pointer: *mut CDocument) -> Result<RawDocument, String> {
    let handle = DocumentHandle(
        NonNull::new(pointer)
            .ok_or_else(|| "libmandoc could not allocate a document".to_owned())?,
    );
    let document = handle.0.as_ptr();
    if unsafe { raw::mant_mandoc_document_ok(document) } == 0 {
        return Err(
            unsafe { optional_string(raw::mant_mandoc_document_error(document)) }
                .unwrap_or_else(|| "libmandoc could not parse the source".to_owned()),
        );
    }

    let root = unsafe { raw::mant_mandoc_document_root(document) };
    if root.is_null() {
        return Err("libmandoc produced no syntax tree".to_owned());
    }

    let mut node_truncated = false;
    let mut equation_budget = EquationBudget::default();
    let mut transfer_budget = TransferBudget::default();
    let mut identities = IdentityState::default();
    let root = unsafe {
        copy_node(
            document,
            root,
            0,
            &mut node_truncated,
            &mut equation_budget,
            &mut transfer_budget,
            &mut identities,
        )
    }?
    .0;
    Ok(RawDocument {
        document: Document {
            macro_set: macro_set(unsafe { raw::mant_mandoc_document_macroset(document) })?,
            metadata: Metadata {
                title: unsafe { checked_string(raw::mant_mandoc_document_title(document)) }?,
                section: unsafe { checked_string(raw::mant_mandoc_document_section(document)) }?,
                volume: unsafe { checked_string(raw::mant_mandoc_document_volume(document)) }?,
                os: unsafe { checked_string(raw::mant_mandoc_document_os(document)) }?,
                arch: unsafe { checked_string(raw::mant_mandoc_document_arch(document)) }?,
                name: unsafe { checked_string(raw::mant_mandoc_document_name(document)) }?,
                date: unsafe { checked_string(raw::mant_mandoc_document_date(document)) }?,
                alias_target: unsafe {
                    checked_string(raw::mant_mandoc_document_alias_target(document))
                }?,
                has_body: unsafe { raw::mant_mandoc_document_has_body(document) } != 0,
            },
            root,
        },
        diagnostics: unsafe { checked_string(raw::mant_mandoc_document_diagnostics(document)) }?
            .unwrap_or_default(),
        node_truncated,
        equation_truncated: equation_budget.truncated,
        escape_truncated: unsafe { raw::mant_mandoc_document_escape_depth_truncated(document) }
            != 0,
    })
}

// Native failure messages and locale probes are outside the successful owned
// AST contract. Preserve a readable status even when their raw bytes are bad.
pub(super) unsafe fn optional_string(pointer: *const c_char) -> Option<String> {
    if pointer.is_null() {
        None
    } else {
        Some(
            unsafe { CStr::from_ptr(pointer) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

unsafe fn checked_string(pointer: *const c_char) -> Result<Option<String>, String> {
    if pointer.is_null() {
        return Ok(None);
    }
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    let text =
        std::str::from_utf8(bytes).map_err(|_| "libmandoc returned a non-UTF-8 internal string")?;
    Ok(Some(text.to_owned()))
}

#[cfg(test)]
mod string_boundary_tests {
    use super::checked_string;

    #[test]
    fn successful_internal_strings_reject_invalid_utf8() {
        let invalid = [0xff_u8, 0];
        let result = unsafe { checked_string(invalid.as_ptr().cast()) };
        assert_eq!(
            result.unwrap_err(),
            "libmandoc returned a non-UTF-8 internal string"
        );
    }
}

include!(concat!(env!("OUT_DIR"), "/text_sentinels.rs"));

unsafe fn visible_string(pointer: *const c_char) -> Result<Option<String>, String> {
    Ok(unsafe { checked_string(pointer) }?.map(|text| normalize_visible_text(&text)))
}

fn has_native_text_sentinel(text: &str) -> bool {
    text.chars().any(|character| {
        [
            ASCII_NBRSP,
            ASCII_NBRZW,
            ASCII_BREAK,
            ASCII_HYPH,
            ASCII_TABREF,
        ]
        .contains(&character)
    })
}

fn normalize_visible_text(text: &str) -> String {
    if !text.chars().any(|character| {
        [
            ASCII_NBRSP,
            ASCII_NBRZW,
            ASCII_BREAK,
            ASCII_HYPH,
            ASCII_TABREF,
        ]
        .contains(&character)
    }) {
        return text.to_owned();
    }
    text.chars()
        .filter_map(|character| match character {
            ASCII_NBRZW | ASCII_BREAK | ASCII_TABREF => None,
            ASCII_HYPH => Some('-'),
            ASCII_NBRSP => Some(' '),
            other => Some(other),
        })
        .collect()
}

fn macro_set(value: i32) -> Result<MacroSet, String> {
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

fn table_row_kind(
    value: i32,
    layout_rules: Vec<TableRuleCellKind>,
) -> Result<Option<TableRowKind>, String> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(TableRowKind::Data)),
        2 => Ok(Some(TableRowKind::HorizontalRule)),
        3 => Ok(Some(TableRowKind::DoubleHorizontalRule)),
        4 if !layout_rules.is_empty() => Ok(Some(TableRowKind::LayoutRule {
            cells: layout_rules,
        })),
        4 => Err("libmandoc returned an empty layout-only rule row".to_owned()),
        _ => Err("libmandoc returned an unknown table row kind".to_owned()),
    }
}

unsafe fn copy_equation(
    document: *const CDocument,
    pointer: *const CEquationBox,
    depth: usize,
    budget: &mut EquationBudget,
) -> Result<Option<EquationBox>, String> {
    // CVS eqn.h and eqn.c::eqn_box_alloc represent a child/sibling forest.
    // Preserve its depth-first prefix and transfer limits on the heap: the
    // supported 256-box path must also fit a Windows main thread's stack.
    struct Frame {
        equation: EquationBox,
        child: *const CEquationBox,
        depth: usize,
    }

    let Some((equation, child, _)) =
        (unsafe { copy_equation_shallow(document, pointer, depth, budget) })?
    else {
        return Ok(None);
    };
    let mut stack = vec![Frame {
        equation,
        child,
        depth,
    }];
    while let Some(mut frame) = stack.pop() {
        if !frame.child.is_null()
            && !budget.truncated
            && let Some((equation, child, next)) =
                unsafe { copy_equation_shallow(document, frame.child, frame.depth + 1, budget) }?
        {
            frame.child = next;
            let child_depth = frame.depth + 1;
            stack.push(frame);
            stack.push(Frame {
                equation,
                child,
                depth: child_depth,
            });
            continue;
        }
        if let Some(parent) = stack.last_mut() {
            parent.equation.children.push(frame.equation);
        } else {
            return Ok(Some(frame.equation));
        }
    }
    Err("libmandoc returned an empty equation traversal".to_owned())
}

unsafe fn copy_equation_shallow(
    document: *const CDocument,
    pointer: *const CEquationBox,
    depth: usize,
    budget: &mut EquationBudget,
) -> Result<Option<(EquationBox, *const CEquationBox, *const CEquationBox)>, String> {
    if pointer.is_null() {
        return Ok(None);
    }
    if depth >= MAX_OWNED_EQUATION_DEPTH || budget.nodes >= MAX_OWNED_EQUATION_NODES {
        budget.truncated = true;
        return Ok(None);
    }
    let mut view = std::mem::MaybeUninit::<CEquationBoxView>::uninit();
    if unsafe { raw::mant_mandoc_eqn_box_snapshot(document, pointer, view.as_mut_ptr()) } == 0 {
        return Err("libmandoc returned an invalid borrowed equation box".to_owned());
    }
    let view = unsafe { view.assume_init() };
    let kind = match view.kind {
        0 => EquationKind::Text,
        1 => EquationKind::Subexpression,
        2 => EquationKind::List,
        3 => EquationKind::Pile,
        4 => EquationKind::Matrix,
        _ => return Err("libmandoc returned an unknown equation box kind".to_owned()),
    };
    let font = match view.font {
        0 => EquationFont::None,
        1 => EquationFont::Roman,
        2 => EquationFont::Bold,
        3 => EquationFont::Fat,
        4 => EquationFont::Italic,
        _ => return Err("libmandoc returned an unknown equation font".to_owned()),
    };
    let position = match view.position {
        0 => EquationPosition::None,
        1 => EquationPosition::Superscript,
        2 => EquationPosition::SubscriptSuperscript,
        3 => EquationPosition::Subscript,
        4 => EquationPosition::To,
        5 => EquationPosition::From,
        6 => EquationPosition::FromTo,
        7 => EquationPosition::Over,
        8 => EquationPosition::Sqrt,
        _ => return Err("libmandoc returned an unknown equation position".to_owned()),
    };
    let text = unsafe { checked_string(view.text) }?;
    let left = unsafe { checked_string(view.left) }?;
    let right = unsafe { checked_string(view.right) }?;
    let top = unsafe { checked_string(view.top) }?;
    let bottom = unsafe { checked_string(view.bottom) }?;
    let required_bytes = std::mem::size_of::<EquationBox>()
        + [&text, &left, &right, &top, &bottom]
            .into_iter()
            .filter_map(|part| part.as_ref())
            .map(String::len)
            .sum::<usize>();
    budget.nodes += 1;
    budget.bytes = budget.bytes.saturating_add(required_bytes);
    if budget.bytes > MAX_OWNED_EQUATION_BYTES {
        budget.truncated = true;
        return Ok(None);
    }
    Ok(Some((
        EquationBox {
            kind,
            font,
            position,
            size: view.size,
            expected_args: view.expected_args,
            actual_args: view.actual_args,
            text,
            gnu_ldots: match view.gnu_ldots {
                0 => false,
                1 => true,
                _ => return Err("libmandoc returned an invalid GNU ldots flag".to_owned()),
            },
            left,
            right,
            top,
            bottom,
            children: Vec::new(),
        },
        view.first,
        view.next,
    )))
}

#[cfg(test)]
mod equation_transfer_tests {
    use super::*;

    #[test]
    fn equation_transfer_limits_keep_the_same_depth_first_prefix() {
        // Exact pristine -Ttree source: eqn.c::eqn_box_alloc appends the
        // siblings a, {b c}, d. Exhaustion inside the group must not resume
        // at d, and a rejected box's budget charge remains observable.
        let source = b".TH EQN 1\n.SH BODY\n.EQ\na { b c } d\n.EN\n";
        let pointer = unsafe {
            raw::mant_mandoc_parse_buffer(
                c"budget.1".as_ptr(),
                source.as_ptr(),
                source.len(),
                std::ptr::null(),
                0,
                0,
                std::ptr::null(),
                None,
                std::ptr::null_mut(),
            )
        };
        let document = DocumentHandle(NonNull::new(pointer).expect("native document"));
        let mut pending = vec![unsafe { raw::mant_mandoc_document_root(pointer) }];
        let equation = loop {
            let node = pending.pop().expect("native equation node");
            let mut view = std::mem::MaybeUninit::<CNodeView>::uninit();
            assert_ne!(
                unsafe { raw::mant_mandoc_node_snapshot(pointer, node, view.as_mut_ptr()) },
                0
            );
            let view = unsafe { view.assume_init() };
            if !view.equation.is_null() {
                break view.equation;
            }
            if !view.next.is_null() {
                pending.push(view.next);
            }
            if !view.child.is_null() {
                pending.push(view.child);
            }
        };
        for (depth, mut budget, expected_children) in [
            (
                0,
                EquationBudget {
                    nodes: MAX_OWNED_EQUATION_NODES - 3,
                    ..EquationBudget::default()
                },
                2,
            ),
            (MAX_OWNED_EQUATION_DEPTH - 2, EquationBudget::default(), 2),
            (
                0,
                EquationBudget {
                    bytes: MAX_OWNED_EQUATION_BYTES - 2 * std::mem::size_of::<EquationBox>() - 1,
                    ..EquationBudget::default()
                },
                1,
            ),
        ] {
            let initial_nodes = budget.nodes;
            let initial_bytes = budget.bytes;
            let owned = unsafe { copy_equation(pointer, equation, depth, &mut budget) }
                .unwrap()
                .unwrap();
            assert!(budget.truncated);
            assert_eq!(budget.nodes - initial_nodes, 3);
            assert_eq!(
                budget.bytes - initial_bytes,
                3 * std::mem::size_of::<EquationBox>() + 1
            );
            assert_eq!(owned.children.len(), expected_children);
            assert_eq!(owned.children[0].text.as_deref(), Some("a"));
            assert!(owned.children.last().unwrap().children.is_empty());
            assert!(!owned.readable_text().contains('d'));
        }
        let owned = unsafe { copy_equation(pointer, equation, 0, &mut EquationBudget::default()) }
            .unwrap()
            .unwrap();
        assert_eq!(
            owned
                .children
                .iter()
                .map(|child| child.text.as_deref())
                .collect::<Vec<_>>(),
            [Some("a"), None, Some("d")]
        );
        assert_eq!(
            owned.children[1]
                .children
                .iter()
                .map(|child| child.text.as_deref())
                .collect::<Vec<_>>(),
            [Some("b"), Some("c")]
        );
        // The returned EquationBoxes are independent of the borrowed forest.
        drop(document);
        assert!(owned.readable_text().contains('d'));
    }
}

unsafe fn copy_node(
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
    let raw_text = unsafe { checked_string(view.text) }?;
    let native_text = raw_text
        .as_ref()
        .filter(|text| has_native_text_sentinel(text))
        .cloned();
    let text = raw_text.as_deref().map(normalize_visible_text);
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

/// Copy the declared `Bl -column` width strings, if this node owns any.
unsafe fn copy_column_strings(
    pointer: *const *const c_char,
    count: usize,
    transfer_budget: &mut TransferBudget,
) -> Result<Vec<String>, String> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if pointer.is_null() {
        return Err("libmandoc returned null columns with a nonzero count".to_owned());
    }
    if !pointer.is_aligned() {
        return Err("libmandoc returned misaligned column pointers".to_owned());
    }
    let pointer_bytes = count
        .checked_mul(std::mem::size_of::<*const c_char>())
        .filter(|bytes| *bytes <= usize::try_from(isize::MAX).unwrap_or(usize::MAX))
        .ok_or_else(|| "libmandoc column pointer range overflowed".to_owned())?;
    let owned_bytes = count
        .checked_mul(std::mem::size_of::<String>())
        .ok_or_else(|| "libmandoc column allocation overflowed".to_owned())?;
    if count > MAX_OWNED_SYNTAX_ITEMS.saturating_sub(transfer_budget.items)
        || owned_bytes > MAX_OWNED_SYNTAX_BYTES.saturating_sub(transfer_budget.bytes)
        || pointer_bytes > MAX_OWNED_SYNTAX_BYTES
    {
        return Err("owned column transfer exceeded its cumulative node/byte budget".to_owned());
    }
    // Reserve the complete pointer/count transfer before allocating or walking
    // borrowed storage. Parser-owned entries remain valid for this call only.
    transfer_budget.items += count;
    transfer_budget.bytes += owned_bytes;
    let mut columns = Vec::with_capacity(count);
    for index in 0..count {
        let value = unsafe { *pointer.add(index) };
        if value.is_null() {
            return Err("libmandoc returned a null column string".to_owned());
        }
        let bytes = unsafe { CStr::from_ptr(value) }.to_bytes();
        if bytes.len() > MAX_OWNED_SYNTAX_BYTES.saturating_sub(transfer_budget.bytes) {
            return Err("owned column strings exceeded the cumulative byte budget".to_owned());
        }
        transfer_budget.bytes += bytes.len();
        columns.push(
            std::str::from_utf8(bytes)
                .map_err(|_| "libmandoc returned a non-UTF-8 column string")?
                .to_owned(),
        );
    }
    Ok(columns)
}

unsafe fn copy_table_cells(
    document: *const CDocument,
    mut pointer: *const CTableCell,
    transfer_budget: &mut TransferBudget,
) -> Result<Vec<TableCell>, String> {
    let mut cells = Vec::new();
    while !pointer.is_null() {
        let mut view = std::mem::MaybeUninit::<CTableCellView>::uninit();
        if unsafe { raw::mant_mandoc_table_cell_snapshot(document, pointer, view.as_mut_ptr()) }
            == 0
        {
            return Err("libmandoc returned an invalid borrowed table cell".to_owned());
        }
        let view = unsafe { view.assume_init() };
        let raw_text = unsafe { checked_string(view.text) }?;
        let cell = TableCell {
            kind: match view.kind {
                1 => TableCellKind::Empty,
                2 => TableCellKind::HorizontalRule,
                3 => TableCellKind::DoubleHorizontalRule,
                4 => TableCellKind::IsolatedHorizontalRule,
                5 => TableCellKind::IsolatedDoubleHorizontalRule,
                _ => TableCellKind::Text,
            },
            font: match view.font {
                0 => None,
                1 => Some(TableFont::Roman),
                2 => Some(TableFont::Bold),
                3 => Some(TableFont::Italic),
                4 => Some(TableFont::BoldItalic),
                5 => Some(TableFont::Code),
                6 => Some(TableFont::CodeBold),
                7 => Some(TableFont::CodeItalic),
                _ => return Err("libmandoc returned an unknown table layout font".to_owned()),
            },
            text: raw_text.as_deref().map(normalize_visible_text),
            native_text: raw_text.filter(|text| has_native_text_sentinel(text)),
            text_block: view.text_block != 0,
            source_recovery_safe: view.source_recovery_safe != 0,
            vertical_continuation: view.vertical_continuation != 0,
            column_span: view.column_span.try_into().unwrap_or(u16::MAX),
            row_span: view.row_span.try_into().unwrap_or(u16::MAX),
            alignment: match view.alignment {
                1 => TableAlignment::Center,
                2 => TableAlignment::Right,
                _ => TableAlignment::Left,
            },
        };
        transfer_budget.charge(
            std::mem::size_of::<TableCell>()
                .saturating_add(cell.text.as_ref().map_or(0, String::len))
                .saturating_add(cell.native_text.as_ref().map_or(0, String::len)),
        )?;
        cells.push(cell);
        pointer = view.next;
    }
    Ok(cells)
}

unsafe fn copy_table_rule_cells(
    document: *const CDocument,
    mut pointer: *const CTableRuleCell,
    transfer_budget: &mut TransferBudget,
) -> Result<Vec<TableRuleCellKind>, String> {
    let mut cells = Vec::new();
    while !pointer.is_null() {
        let mut view = std::mem::MaybeUninit::<CTableRuleCellView>::uninit();
        if unsafe {
            raw::mant_mandoc_table_rule_cell_snapshot(document, pointer, view.as_mut_ptr())
        } == 0
        {
            return Err("libmandoc returned an invalid borrowed table rule cell".to_owned());
        }
        let view = unsafe { view.assume_init() };
        transfer_budget.charge(std::mem::size_of::<TableRuleCellKind>())?;
        cells.push(match view.kind {
            1 => TableRuleCellKind::Horizontal,
            2 => TableRuleCellKind::DoubleHorizontal,
            _ => return Err("libmandoc returned an unknown table rule cell kind".to_owned()),
        });
        pointer = view.next;
    }
    Ok(cells)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn column_pointer_count_is_validated_before_allocation_or_dereference() {
        let mut budget = TransferBudget::default();
        assert!(
            unsafe { copy_column_strings(std::ptr::null(), 0, &mut budget) }
                .unwrap()
                .is_empty()
        );
        assert!(unsafe { copy_column_strings(std::ptr::null(), 1, &mut budget) }.is_err());
        let invalid = std::ptr::NonNull::<*const c_char>::dangling().as_ptr();
        let misaligned = invalid.with_addr(invalid.addr().wrapping_add(1));
        assert!(unsafe { copy_column_strings(misaligned, 1, &mut budget) }.is_err());
        for count in [usize::MAX, MAX_OWNED_SYNTAX_ITEMS + 1] {
            assert!(unsafe { copy_column_strings(invalid, count, &mut budget) }.is_err());
        }
        let strings = [
            CString::new("first").unwrap(),
            CString::new("\\(em").unwrap(),
        ];
        let pointers = strings.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();
        assert_eq!(
            unsafe { copy_column_strings(pointers.as_ptr(), 2, &mut budget) }.unwrap(),
            ["first", "\\(em"]
        );
        drop(strings);
        let null_entry = [std::ptr::null()];
        assert!(unsafe { copy_column_strings(null_entry.as_ptr(), 1, &mut budget) }.is_err());
        budget.bytes = MAX_OWNED_SYNTAX_BYTES;
        assert!(unsafe { copy_column_strings(invalid, 1, &mut budget) }.is_err());
    }

    #[test]
    fn native_marker_values_and_visible_translation_follow_the_pinned_header() {
        assert_eq!(ASCII_TABREF, '\u{1a}');
        assert_eq!(ASCII_HYPH, '\u{1c}');
        assert_eq!(ASCII_BREAK, '\u{1d}');
        assert_eq!(ASCII_NBRZW, '\u{1e}');
        assert_eq!(ASCII_NBRSP, '\u{1f}');
        for (marker, expected) in [
            (ASCII_TABREF, "AB"),
            (ASCII_HYPH, "A-B"),
            (ASCII_BREAK, "AB"),
            (ASCII_NBRZW, "AB"),
            (ASCII_NBRSP, "A B"),
        ] {
            let input = CString::new(format!("A{marker}B")).unwrap();
            // CString owns the NUL-terminated bytes for this complete call.
            assert_eq!(
                unsafe { visible_string(input.as_ptr()) }
                    .unwrap()
                    .as_deref(),
                Some(expected)
            );
        }
        let input = CString::new("café 日本 😀\t").unwrap();
        assert_eq!(
            unsafe { visible_string(input.as_ptr()) }
                .unwrap()
                .as_deref(),
            Some("café 日本 😀\t")
        );
        assert_eq!(unsafe { visible_string(std::ptr::null()) }.unwrap(), None);
    }
}
