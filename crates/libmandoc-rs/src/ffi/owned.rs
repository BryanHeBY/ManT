//! Immediate borrowed snapshot to owned Rust AST transfer.
use super::{
    raw::{
        self, CDocument, CEquationBox, CEquationBoxView, CNode, CNodeView, CTableCell,
        CTableCellView, CTableRuleCell, CTableRuleCellView,
    },
    session::DocumentHandle,
};
use crate::{
    AuthorMode, DefinitionListStyle, DisplayKind, Document, Equation, EquationBox, EquationBoxKind,
    EquationFont, EquationPosition, MacroSet, Metadata, Node, NodeFlags, NodeKind,
    NormalizedEnclosure, NormalizedFont, NormalizedListKind, RawDocument, TableAlignment,
    TableCell, TableCellDataKind, TableCellKind, TableCellLayoutKind, TableRowKind,
    TableRuleCellKind,
};
use std::{
    ffi::CStr,
    mem::{MaybeUninit, align_of, offset_of, size_of},
    os::raw::c_char,
    ptr::NonNull,
    sync::OnceLock,
};
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
const KNOWN_NODE_FLAGS: u32 = NODE_GENERATED
    | NODE_SENTENCE_END
    | NODE_NO_PRINT
    | NODE_NO_FILL
    | NODE_DEEP_LINK_TARGET
    | NODE_PERMALINK
    | NODE_LINE_START
    | NODE_DELIMITER_OPEN
    | NODE_DELIMITER_CLOSE
    | NODE_SYNOPSIS_PRETTY
    | NODE_TABLE_START;
const MAX_OWNED_NODE_DEPTH: usize = 256;
const MAX_OWNED_EQUATION_BOXES: usize = 1_000_000;
static SNAPSHOT_LAYOUT_VALIDATION: OnceLock<Result<(), String>> = OnceLock::new();

#[derive(Clone, Copy)]
struct NativeLayout {
    size: usize,
    align: usize,
    field_count: u32,
    offset: unsafe extern "C" fn(u32) -> usize,
}

pub(super) fn copy_document(pointer: *mut CDocument) -> Result<RawDocument, String> {
    let handle = DocumentHandle(
        NonNull::new(pointer)
            .ok_or_else(|| "libmandoc could not allocate a document".to_owned())?,
    );
    copy_document_from_handle(&handle, None)
}

pub(super) fn copy_document_from_handle(
    handle: &DocumentHandle,
    mut execution_node_keys: Option<&mut Vec<u32>>,
) -> Result<RawDocument, String> {
    let document = handle.0.as_ptr();
    validate_snapshot_layouts()?;
    if !native_bool(
        unsafe { raw::mant_mandoc_document_ok(document) },
        "document-ok",
    )? {
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
    let mut equation_truncated = false;
    let mut equation_box_count = 0_usize;
    let root = unsafe {
        copy_node(
            document,
            root,
            0,
            &mut node_truncated,
            &mut equation_truncated,
            &mut equation_box_count,
            &mut execution_node_keys,
        )
    }?
    .0;
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
                has_body: native_bool(
                    unsafe { raw::mant_mandoc_document_has_body(document) },
                    "document has-body",
                )?,
            },
            root,
        },
        diagnostics: unsafe {
            optional_string(raw::mant_mandoc_document_diagnostics(document)).unwrap_or_default()
        },
        node_truncated,
        equation_truncated,
    })
}

pub(super) fn validate_snapshot_layouts() -> Result<(), String> {
    SNAPSHOT_LAYOUT_VALIDATION
        .get_or_init(compute_snapshot_layout_validation)
        .clone()
}

fn compute_snapshot_layout_validation() -> Result<(), String> {
    let node_offsets = node_view_offsets();
    let table_cell_offsets = table_cell_view_offsets();
    let table_rule_cell_offsets = table_rule_cell_view_offsets();
    let equation_box_offsets = equation_box_view_offsets();

    validate_layout(
        "node view",
        size_of::<CNodeView>(),
        align_of::<CNodeView>(),
        &node_offsets,
        NativeLayout {
            size: unsafe { raw::mant_mandoc_node_view_size() },
            align: unsafe { raw::mant_mandoc_node_view_align() },
            field_count: unsafe { raw::mant_mandoc_node_view_field_count() },
            offset: raw::mant_mandoc_node_view_offset,
        },
    )?;
    validate_layout(
        "table cell view",
        size_of::<CTableCellView>(),
        align_of::<CTableCellView>(),
        &table_cell_offsets,
        NativeLayout {
            size: unsafe { raw::mant_mandoc_table_cell_view_size() },
            align: unsafe { raw::mant_mandoc_table_cell_view_align() },
            field_count: unsafe { raw::mant_mandoc_table_cell_view_field_count() },
            offset: raw::mant_mandoc_table_cell_view_offset,
        },
    )?;
    validate_layout(
        "table rule cell view",
        size_of::<CTableRuleCellView>(),
        align_of::<CTableRuleCellView>(),
        &table_rule_cell_offsets,
        NativeLayout {
            size: unsafe { raw::mant_mandoc_table_rule_cell_view_size() },
            align: unsafe { raw::mant_mandoc_table_rule_cell_view_align() },
            field_count: unsafe { raw::mant_mandoc_table_rule_cell_view_field_count() },
            offset: raw::mant_mandoc_table_rule_cell_view_offset,
        },
    )?;
    validate_layout(
        "equation box view",
        size_of::<CEquationBoxView>(),
        align_of::<CEquationBoxView>(),
        &equation_box_offsets,
        NativeLayout {
            size: unsafe { raw::mant_mandoc_equation_box_view_size() },
            align: unsafe { raw::mant_mandoc_equation_box_view_align() },
            field_count: unsafe { raw::mant_mandoc_equation_box_view_field_count() },
            offset: raw::mant_mandoc_equation_box_view_offset,
        },
    )
}

fn node_view_offsets() -> [usize; 27] {
    [
        offset_of!(CNodeView, kind),
        offset_of!(CNodeView, execution_node_key),
        offset_of!(CNodeView, macro_name),
        offset_of!(CNodeView, text),
        offset_of!(CNodeView, tag),
        offset_of!(CNodeView, line),
        offset_of!(CNodeView, column),
        offset_of!(CNodeView, flow_epoch),
        offset_of!(CNodeView, table_escape),
        offset_of!(CNodeView, table_source_recovery_safe),
        offset_of!(CNodeView, table_row_kind),
        offset_of!(CNodeView, flags),
        offset_of!(CNodeView, list_kind),
        offset_of!(CNodeView, definition_list_style),
        offset_of!(CNodeView, display_kind),
        offset_of!(CNodeView, font_kind),
        offset_of!(CNodeView, author_mode),
        offset_of!(CNodeView, compact),
        offset_of!(CNodeView, offset),
        offset_of!(CNodeView, width),
        offset_of!(CNodeView, enclosure_open),
        offset_of!(CNodeView, enclosure_close),
        offset_of!(CNodeView, equation),
        offset_of!(CNodeView, table_cells),
        offset_of!(CNodeView, table_rule_cells),
        offset_of!(CNodeView, child),
        offset_of!(CNodeView, next),
    ]
}

fn table_cell_view_offsets() -> [usize; 17] {
    [
        offset_of!(CTableCellView, text),
        offset_of!(CTableCellView, source),
        offset_of!(CTableCellView, source_line),
        offset_of!(CTableCellView, source_column),
        offset_of!(CTableCellView, source_end_line),
        offset_of!(CTableCellView, source_end_column),
        offset_of!(CTableCellView, source_escape),
        offset_of!(CTableCellView, kind),
        offset_of!(CTableCellView, layout_kind),
        offset_of!(CTableCellView, data_kind),
        offset_of!(CTableCellView, text_block),
        offset_of!(CTableCellView, source_recovery_safe),
        offset_of!(CTableCellView, vertical_continuation),
        offset_of!(CTableCellView, column_span),
        offset_of!(CTableCellView, row_span),
        offset_of!(CTableCellView, alignment),
        offset_of!(CTableCellView, next),
    ]
}

fn table_rule_cell_view_offsets() -> [usize; 2] {
    [
        offset_of!(CTableRuleCellView, kind),
        offset_of!(CTableRuleCellView, next),
    ]
}

fn equation_box_view_offsets() -> [usize; 13] {
    [
        offset_of!(CEquationBoxView, kind),
        offset_of!(CEquationBoxView, font),
        offset_of!(CEquationBoxView, position),
        offset_of!(CEquationBoxView, size),
        offset_of!(CEquationBoxView, expected_args),
        offset_of!(CEquationBoxView, actual_args),
        offset_of!(CEquationBoxView, text),
        offset_of!(CEquationBoxView, left),
        offset_of!(CEquationBoxView, right),
        offset_of!(CEquationBoxView, top),
        offset_of!(CEquationBoxView, bottom),
        offset_of!(CEquationBoxView, first),
        offset_of!(CEquationBoxView, next),
    ]
}

fn validate_layout(
    name: &str,
    rust_size: usize,
    rust_align: usize,
    rust_offsets: &[usize],
    native: NativeLayout,
) -> Result<(), String> {
    if rust_size != native.size || rust_align != native.align {
        return Err(format!(
            "libmandoc {name} ABI mismatch: Rust size/alignment {rust_size}/{rust_align}, native {}/{}",
            native.size, native.align
        ));
    }
    if usize::try_from(native.field_count).ok() != Some(rust_offsets.len()) {
        return Err(format!(
            "libmandoc {name} ABI mismatch: Rust has {} fields, native has {}",
            rust_offsets.len(),
            native.field_count
        ));
    }
    for (field, &rust_offset) in rust_offsets.iter().enumerate() {
        let field =
            u32::try_from(field).map_err(|_| format!("libmandoc {name} field count overflow"))?;
        let native_offset = unsafe { (native.offset)(field) };
        if rust_offset != native_offset {
            return Err(format!(
                "libmandoc {name} ABI mismatch at field {field}: Rust offset {rust_offset}, native {native_offset}"
            ));
        }
    }
    Ok(())
}

fn native_bool(value: i32, field: &str) -> Result<bool, String> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(format!("libmandoc returned an invalid {field} flag")),
    }
}

fn optional_source_coordinate(value: i32, field: &str) -> Result<Option<u32>, String> {
    match value {
        -1 => Ok(None),
        value if value >= 0 => value
            .try_into()
            .map(Some)
            .map_err(|_| format!("libmandoc returned an oversized {field}")),
        _ => Err(format!("libmandoc returned an invalid {field}")),
    }
}

struct TableSourceEvidence {
    source: Option<String>,
    line: Option<u32>,
    column: Option<u32>,
    end_line: Option<u32>,
    end_column: Option<u32>,
    escape: Option<u8>,
}

unsafe fn copy_table_source(view: &CTableCellView) -> Result<TableSourceEvidence, String> {
    let source = unsafe { optional_string(view.source) };
    let line = optional_source_coordinate(view.source_line, "table source line")?;
    let column = optional_source_coordinate(view.source_column, "table source column")?;
    let end_line = optional_source_coordinate(view.source_end_line, "table source end line")?;
    let end_column = optional_source_coordinate(view.source_end_column, "table source end column")?;
    let escape = match view.source_escape {
        -1 => None,
        value => Some(
            value
                .try_into()
                .map_err(|_| "libmandoc returned an invalid table source escape".to_owned())?,
        ),
    };
    let coordinate_facts = [
        line.is_some(),
        column.is_some(),
        end_line.is_some(),
        end_column.is_some(),
        escape.is_some(),
    ];
    if coordinate_facts
        .iter()
        .any(|present| *present != source.is_some())
        || line.zip(column) > end_line.zip(end_column)
    {
        return Err(format!(
            "libmandoc returned inconsistent table source evidence: source={}, start={line:?}:{column:?}, end={end_line:?}:{end_column:?}, escape={escape:?}",
            source.is_some()
        ));
    }
    Ok(TableSourceEvidence {
        source,
        line,
        column,
        end_line,
        end_column,
        escape,
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
        3 => Ok(Some(DisplayKind::Unfilled)),
        4 => Ok(Some(DisplayKind::Ragged)),
        5 => Ok(Some(DisplayKind::Centered)),
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

unsafe fn node_from_view(
    document: *mut CDocument,
    view: &CNodeView,
    equation_truncated: &mut bool,
    equation_box_count: &mut usize,
) -> Result<Node, String> {
    let text = unsafe { visible_string(view.text) };
    let line_continuation = text.as_deref().is_some_and(ends_with_no_space_escape);
    let enclosure_open = unsafe { optional_string(view.enclosure_open) };
    let enclosure_close = unsafe { optional_string(view.enclosure_close) };
    Ok(Node {
        execution_node_key: (view.execution_node_key != u32::MAX)
            .then_some(view.execution_node_key),
        kind: node_kind(view.kind)?,
        macro_name: unsafe { optional_string(view.macro_name) },
        text,
        tag: unsafe { visible_string(view.tag) },
        line: view
            .line
            .try_into()
            .map_err(|_| "libmandoc returned a negative source line".to_owned())?,
        column: view
            .column
            .try_into()
            .map_err(|_| "libmandoc returned a negative source column".to_owned())?,
        flow_epoch: view
            .flow_epoch
            .try_into()
            .map_err(|_| "libmandoc returned an oversized flow epoch".to_owned())?,
        table_escape: match view.table_escape {
            -1 => None,
            value => Some(
                value
                    .try_into()
                    .map_err(|_| "libmandoc returned an invalid table escape".to_owned())?,
            ),
        },
        table_source_recovery_safe: native_bool(
            view.table_source_recovery_safe,
            "table source-recovery-safe",
        )?,
        table_row_kind: table_row_kind(view.table_row_kind, unsafe {
            copy_table_rule_cells(document, view.table_rule_cells)
        }?)?,
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
        compact: native_bool(view.compact, "compact")?,
        offset: unsafe { optional_string(view.offset) },
        width: unsafe { optional_string(view.width) },
        table_cells: unsafe { copy_table_cells(document, view.table_cells) }?,
        equation: if view.equation.is_null() {
            None
        } else {
            Some(Box::new(Equation {
                root: unsafe {
                    copy_equation_box(
                        document,
                        view.equation,
                        0,
                        equation_truncated,
                        equation_box_count,
                    )
                }?
                .0,
            }))
        },
        children: Vec::new(),
    })
}

unsafe fn copy_node(
    document: *mut CDocument,
    pointer: *const CNode,
    depth: usize,
    truncated: &mut bool,
    equation_truncated: &mut bool,
    equation_box_count: &mut usize,
    execution_node_keys: &mut Option<&mut Vec<u32>>,
) -> Result<(Node, *const CNode), String> {
    let mut view = std::mem::MaybeUninit::<CNodeView>::uninit();
    if unsafe {
        raw::mant_mandoc_node_snapshot(document, pointer, view.as_mut_ptr(), size_of::<CNodeView>())
    } != 1
    {
        return Err("libmandoc returned an invalid borrowed syntax node".to_owned());
    }
    let view = unsafe { view.assume_init() };
    if let Some(keys) = execution_node_keys.as_deref_mut() {
        keys.try_reserve(1)
            .map_err(|_| "could not allocate execution AST node-key transfer".to_owned())?;
        keys.push(view.execution_node_key);
    }
    if view.flags & !KNOWN_NODE_FLAGS != 0 {
        return Err("libmandoc returned unknown syntax node flags".to_owned());
    }
    let mut node =
        unsafe { node_from_view(document, &view, equation_truncated, equation_box_count) }?;

    if depth + 1 < MAX_OWNED_NODE_DEPTH {
        let mut child = view.child;
        while !child.is_null() {
            let (owned, next) = unsafe {
                copy_node(
                    document,
                    child,
                    depth + 1,
                    truncated,
                    equation_truncated,
                    equation_box_count,
                    execution_node_keys,
                )
            }?;
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

unsafe fn copy_equation_box(
    document: *const CDocument,
    pointer: *const CEquationBox,
    depth: usize,
    truncated: &mut bool,
    box_count: &mut usize,
) -> Result<(EquationBox, *const CEquationBox), String> {
    if *box_count >= MAX_OWNED_EQUATION_BOXES {
        *truncated = true;
        return Err("libmandoc equation tree exceeds the owned transfer limit".to_owned());
    }
    *box_count += 1;
    let mut view = MaybeUninit::<CEquationBoxView>::uninit();
    if unsafe {
        raw::mant_mandoc_equation_box_snapshot(
            document,
            pointer,
            view.as_mut_ptr(),
            size_of::<CEquationBoxView>(),
        )
    } != 1
    {
        return Err("libmandoc returned an invalid borrowed equation box".to_owned());
    }
    let view = unsafe { view.assume_init() };
    let mut children = Vec::new();
    if depth + 1 < MAX_OWNED_NODE_DEPTH {
        let mut child = view.first;
        while !child.is_null() {
            let (owned, next) =
                unsafe { copy_equation_box(document, child, depth + 1, truncated, box_count) }?;
            children.push(owned);
            child = next;
        }
        if u64::try_from(children.len()).ok() != Some(view.actual_args) {
            return Err("libmandoc returned inconsistent equation child counts".to_owned());
        }
    } else if !view.first.is_null() {
        *truncated = true;
    }
    let box_value = EquationBox {
        kind: match view.kind {
            0 => EquationBoxKind::Text,
            1 => EquationBoxKind::Subexpression,
            2 => EquationBoxKind::List,
            3 => EquationBoxKind::Pile,
            4 => EquationBoxKind::Matrix,
            _ => return Err("libmandoc returned an unknown equation box kind".to_owned()),
        },
        font: match view.font {
            0 => EquationFont::None,
            1 => EquationFont::Roman,
            2 => EquationFont::Bold,
            3 => EquationFont::Fat,
            4 => EquationFont::Italic,
            _ => return Err("libmandoc returned an unknown equation font".to_owned()),
        },
        position: match view.position {
            0 => EquationPosition::None,
            1 => EquationPosition::Superscript,
            2 => EquationPosition::SubscriptSuperscript,
            3 => EquationPosition::Subscript,
            4 => EquationPosition::To,
            5 => EquationPosition::From,
            6 => EquationPosition::FromTo,
            7 => EquationPosition::Over,
            8 => EquationPosition::SquareRoot,
            _ => return Err("libmandoc returned an unknown equation position".to_owned()),
        },
        size: view.size,
        expected_args: view.expected_args,
        actual_args: view.actual_args,
        text: unsafe { optional_string(view.text) },
        left: unsafe { optional_string(view.left) },
        right: unsafe { optional_string(view.right) },
        top: unsafe { optional_string(view.top) },
        bottom: unsafe { optional_string(view.bottom) },
        children,
    };
    Ok((box_value, view.next))
}

unsafe fn copy_table_cells(
    document: *const CDocument,
    mut pointer: *const CTableCell,
) -> Result<Vec<TableCell>, String> {
    let mut cells = Vec::new();
    while !pointer.is_null() {
        let mut view = std::mem::MaybeUninit::<CTableCellView>::uninit();
        if unsafe {
            raw::mant_mandoc_table_cell_snapshot(
                document,
                pointer,
                view.as_mut_ptr(),
                size_of::<CTableCellView>(),
            )
        } != 1
        {
            return Err("libmandoc returned an invalid borrowed table cell".to_owned());
        }
        let view = unsafe { view.assume_init() };
        let source = unsafe { copy_table_source(&view) }?;
        cells.push(TableCell {
            kind: match view.kind {
                0 => TableCellKind::Text,
                1 => TableCellKind::Empty,
                2 => TableCellKind::HorizontalRule,
                3 => TableCellKind::DoubleHorizontalRule,
                4 => TableCellKind::IsolatedHorizontalRule,
                5 => TableCellKind::IsolatedDoubleHorizontalRule,
                _ => return Err("libmandoc returned an unknown table cell kind".to_owned()),
            },
            layout_kind: match view.layout_kind {
                1 => TableCellLayoutKind::Center,
                2 => TableCellLayoutKind::Right,
                3 => TableCellLayoutKind::Left,
                4 => TableCellLayoutKind::Numeric,
                5 => TableCellLayoutKind::Span,
                6 => TableCellLayoutKind::Long,
                7 => TableCellLayoutKind::Down,
                8 => TableCellLayoutKind::HorizontalRule,
                9 => TableCellLayoutKind::DoubleHorizontalRule,
                _ => return Err("libmandoc returned an unknown table layout kind".to_owned()),
            },
            data_kind: match view.data_kind {
                1 => TableCellDataKind::Empty,
                2 => TableCellDataKind::Text,
                3 => TableCellDataKind::HorizontalRule,
                4 => TableCellDataKind::DoubleHorizontalRule,
                5 => TableCellDataKind::IsolatedHorizontalRule,
                6 => TableCellDataKind::IsolatedDoubleHorizontalRule,
                _ => return Err("libmandoc returned an unknown table data kind".to_owned()),
            },
            text: unsafe { visible_string(view.text) },
            source: source.source,
            source_line: source.line,
            source_column: source.column,
            source_end_line: source.end_line,
            source_end_column: source.end_column,
            source_escape: source.escape,
            text_block: native_bool(view.text_block, "table text-block")?,
            source_recovery_safe: native_bool(
                view.source_recovery_safe,
                "table source-recovery-safe",
            )?,
            vertical_continuation: native_bool(
                view.vertical_continuation,
                "table vertical-continuation",
            )?,
            column_span: view
                .column_span
                .try_into()
                .map_err(|_| "libmandoc returned an oversized table column span".to_owned())?,
            row_span: view
                .row_span
                .try_into()
                .map_err(|_| "libmandoc returned an oversized table row span".to_owned())?,
            alignment: match view.alignment {
                0 => TableAlignment::Left,
                1 => TableAlignment::Center,
                2 => TableAlignment::Right,
                _ => return Err("libmandoc returned an unknown table alignment".to_owned()),
            },
        });
        pointer = view.next;
    }
    Ok(cells)
}

unsafe fn copy_table_rule_cells(
    document: *const CDocument,
    mut pointer: *const CTableRuleCell,
) -> Result<Vec<TableRuleCellKind>, String> {
    let mut cells = Vec::new();
    while !pointer.is_null() {
        let mut view = std::mem::MaybeUninit::<CTableRuleCellView>::uninit();
        if unsafe {
            raw::mant_mandoc_table_rule_cell_snapshot(
                document,
                pointer,
                view.as_mut_ptr(),
                size_of::<CTableRuleCellView>(),
            )
        } != 1
        {
            return Err("libmandoc returned an invalid borrowed table rule cell".to_owned());
        }
        let view = unsafe { view.assume_init() };
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

    #[test]
    fn native_flags_reject_non_boolean_values() {
        assert_eq!(native_bool(0, "probe"), Ok(false));
        assert_eq!(native_bool(1, "probe"), Ok(true));
        assert!(native_bool(-1, "probe").is_err());
        assert!(native_bool(2, "probe").is_err());
    }
}
