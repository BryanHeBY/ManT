//! Immediate borrowed snapshot to owned Rust AST transfer.
use super::{
    raw::{self, CDocument, CNode, CNodeView, CTableCell, CTableCellView},
    session::DocumentHandle,
};
use crate::{
    AuthorMode, DefinitionListStyle, DisplayKind, Document, MacroSet, Metadata, Node, NodeFlags,
    NodeKind, NormalizedEnclosure, NormalizedFont, NormalizedListKind, RawDocument, TableAlignment,
    TableCell, TableCellKind,
};
use std::{ffi::CStr, os::raw::c_char, ptr::NonNull};
const NODE_GENERATED: u32 = 1 << 0;
const NODE_SENTENCE_END: u32 = 1 << 1;
const NODE_NO_PRINT: u32 = 1 << 2;
const NODE_NO_FILL: u32 = 1 << 3;
const NODE_DEEP_LINK_TARGET: u32 = 1 << 4;
const NODE_PERMALINK: u32 = 1 << 5;
const NODE_LINE_START: u32 = 1 << 6;
const NODE_DELIMITER_OPEN: u32 = 1 << 7;
const NODE_DELIMITER_CLOSE: u32 = 1 << 8;
const NODE_SYNOPSIS_PRETTY: u32 = 1 << 9;
const NODE_TABLE_START: u32 = 1 << 10;
const MAX_OWNED_NODE_DEPTH: usize = 256;

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
    let root = unsafe { copy_node(document, root, 0, &mut node_truncated) }?.0;
    Ok(RawDocument {
        document: Document {
            macro_set: macro_set(unsafe { raw::mant_mandoc_document_macroset(document) })?,
            metadata: Metadata {
                title: unsafe { optional_string(raw::mant_mandoc_document_title(document)) },
                section: unsafe { optional_string(raw::mant_mandoc_document_section(document)) },
                volume: unsafe { optional_string(raw::mant_mandoc_document_volume(document)) },
                os: unsafe { optional_string(raw::mant_mandoc_document_os(document)) },
                arch: unsafe { optional_string(raw::mant_mandoc_document_arch(document)) },
                name: unsafe { optional_string(raw::mant_mandoc_document_name(document)) },
                date: unsafe { optional_string(raw::mant_mandoc_document_date(document)) },
                alias_target: unsafe {
                    optional_string(raw::mant_mandoc_document_alias_target(document))
                },
                has_body: unsafe { raw::mant_mandoc_document_has_body(document) } != 0,
            },
            root,
        },
        diagnostics: unsafe {
            optional_string(raw::mant_mandoc_document_diagnostics(document)).unwrap_or_default()
        },
        node_truncated,
        equation_truncated: unsafe { raw::mant_mandoc_document_equation_truncated(document) } != 0,
    })
}

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

include!(concat!(env!("OUT_DIR"), "/text_sentinels.rs"));

unsafe fn visible_string(pointer: *const c_char) -> Option<String> {
    unsafe { optional_string(pointer) }.map(|text| {
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
            return text;
        }
        text.chars()
            .filter_map(|character| match character {
                ASCII_NBRZW | ASCII_BREAK | ASCII_TABREF => None,
                ASCII_HYPH => Some('-'),
                ASCII_NBRSP => Some(' '),
                other => Some(other),
            })
            .collect()
    })
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

fn list_kind(value: i32) -> Result<Option<NormalizedListKind>, String> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(NormalizedListKind::Bullet)),
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

unsafe fn copy_node(
    document: *mut CDocument,
    pointer: *const CNode,
    depth: usize,
    truncated: &mut bool,
) -> Result<(Node, *const CNode), String> {
    let mut view = std::mem::MaybeUninit::<CNodeView>::uninit();
    if unsafe { raw::mant_mandoc_node_snapshot(document, pointer, view.as_mut_ptr()) } == 0 {
        return Err("libmandoc returned an invalid borrowed syntax node".to_owned());
    }
    let view = unsafe { view.assume_init() };
    let text = unsafe { visible_string(view.text) };
    let line_continuation = text.as_deref().is_some_and(ends_with_no_space_escape);
    let enclosure_open = unsafe { optional_string(view.enclosure_open) };
    let enclosure_close = unsafe { optional_string(view.enclosure_close) };
    let mut node = Node {
        kind: node_kind(view.kind)?,
        macro_name: unsafe { optional_string(view.macro_name) },
        text,
        tag: unsafe { visible_string(view.tag) },
        line: view.line.try_into().unwrap_or_default(),
        column: view.column.try_into().unwrap_or_default(),
        flow_epoch: view.flow_epoch,
        flags: NodeFlags {
            generated: view.flags & NODE_GENERATED != 0,
            sentence_end: view.flags & NODE_SENTENCE_END != 0,
            no_print: view.flags & NODE_NO_PRINT != 0,
            no_fill: view.flags & NODE_NO_FILL != 0,
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
        offset: unsafe { optional_string(view.offset) },
        width: unsafe { optional_string(view.width) },
        table_cells: unsafe { copy_table_cells(document, view.table_cells) }?,
        // The shim reuses this equation buffer on the next node snapshot, so
        // copy it before descending into children or taking another snapshot.
        equation: unsafe { visible_string(view.equation) },
        children: Vec::new(),
    };

    if depth + 1 < MAX_OWNED_NODE_DEPTH {
        let mut child = view.child;
        while !child.is_null() {
            let (owned, next) = unsafe { copy_node(document, child, depth + 1, truncated) }?;
            node.children.push(owned);
            child = next;
        }
    } else if !view.child.is_null() {
        *truncated = true;
    }
    Ok((node, view.next))
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

unsafe fn copy_table_cells(
    document: *const CDocument,
    mut pointer: *const CTableCell,
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
        cells.push(TableCell {
            kind: match view.kind {
                1 => TableCellKind::Empty,
                2 => TableCellKind::HorizontalRule,
                3 => TableCellKind::DoubleHorizontalRule,
                4 => TableCellKind::IsolatedHorizontalRule,
                5 => TableCellKind::IsolatedDoubleHorizontalRule,
                _ => TableCellKind::Text,
            },
            text: unsafe { visible_string(view.text) },
            text_block: view.text_block != 0,
            vertical_continuation: view.vertical_continuation != 0,
            column_span: view.column_span.try_into().unwrap_or(u16::MAX),
            row_span: view.row_span.try_into().unwrap_or(u16::MAX),
            alignment: match view.alignment {
                1 => TableAlignment::Center,
                2 => TableAlignment::Right,
                _ => TableAlignment::Left,
            },
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
                unsafe { visible_string(input.as_ptr()) }.as_deref(),
                Some(expected)
            );
        }
        let input = CString::new("café 日本 😀\t").unwrap();
        assert_eq!(
            unsafe { visible_string(input.as_ptr()) }.as_deref(),
            Some("café 日本 😀\t")
        );
        assert_eq!(unsafe { visible_string(std::ptr::null()) }, None);
    }
}
