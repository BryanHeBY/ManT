use super::{
    DocumentBuilder, DynamicPreprocessRecovery, LimitFinding, Limits, NodeId, NodeKind,
    PreprocessRecovery, SourceSpan, TableAlignment, TableCell, TableTerminalBorder,
    TableTerminalCell, TableTerminalFont, TableTerminalRow,
};

#[derive(Clone)]
pub(super) struct SourceLine {
    pub(super) source: NodeId,
    pub(super) text: String,
    macro_name: Option<Box<str>>,
    /// A bare roff control line is stored at the byte after its introducer.
    /// tbl nevertheless treats it as a literal layout terminator.
    layout_control_prefix: bool,
    has_invalid_input_bytes: bool,
    has_valid_utf8_non_ascii: bool,
    table_input_text: Option<Box<str>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CellFormat {
    Left,
    Center,
    Right,
    Number,
    Span,
    Down,
    HorizontalRule,
    DoubleHorizontalRule,
}

impl CellFormat {
    fn alignment(self) -> TableAlignment {
        match self {
            Self::Center => TableAlignment::Center,
            // The legacy FFI projection lowers tbl's numeric `n` cells to
            // the same public right-alignment value as explicit `r` cells.
            Self::Right | Self::Number => TableAlignment::Right,
            Self::Left
            | Self::Span
            | Self::Down
            | Self::HorizontalRule
            | Self::DoubleHorizontalRule => TableAlignment::Left,
        }
    }
}

#[derive(Clone, Default)]
struct TableLayout {
    rows: Vec<Vec<CellFormat>>,
    terminal_rows: Vec<Vec<TableTerminalCell>>,
    delimiter: char,
    outer_border: TableTerminalBorder,
    all_box: bool,
    centered: bool,
}

struct ParsedLayout {
    layout: TableLayout,
    data_start: usize,
    dynamic_recoveries: Vec<DynamicPreprocessRecovery>,
}

#[derive(Default)]
pub(super) struct ParsedLayoutRow {
    pub(super) cells: Vec<CellFormat>,
    pub(super) terminal_cells: Vec<TableTerminalCell>,
    invalid_fonts: Vec<TableFontIssue>,
    excessive_spacings: Vec<TableSpacingIssue>,
    /// Byte offsets of vertical bars beyond tbl's two-per-cell limit.
    pub(super) vertical_bar_offsets: Vec<usize>,
    /// Byte offsets of actual `^` cell descriptors, rather than `^` bytes
    /// that occurred while scanning another part of the layout language.
    pub(super) leading_down_offsets: Vec<usize>,
    first_cell_offset: Option<usize>,
    leading_vertical_bars: usize,
}

pub(super) struct ParsedLayoutLine {
    pub(super) rows: Vec<ParsedLayoutRow>,
    /// A layout such as `|.` has no actual cell descriptor, but tbl still
    /// carries its initial rule into the one-column recovery row.  This is
    /// presentation-only metadata for the terminal renderer; the public AST
    /// keeps the normal recovered single cell.
    pub(super) leading_vertical_bars: usize,
    pub(super) complete: bool,
    complete_offset: Option<usize>,
}

struct TableFontIssue {
    /// Byte offset of the invalid font name, relative to the format line.
    font_start: usize,
    /// The legacy message prints from the modifier's `f` through line end.
    request: Box<str>,
}

struct TableSpacingIssue {
    /// Byte offset of the first spacing digit, relative to the format line.
    spacing_start: usize,
    value: Box<str>,
}

pub(super) struct ParsedRow {
    pub(super) source: NodeId,
    pub(super) cells: Vec<TableCell>,
    pub(super) terminal: TableTerminalRow,
}

pub(super) struct ParsedTable {
    pub(super) rows: Vec<ParsedRow>,
    pub(super) limit: Option<LimitFinding>,
    pub(super) recoveries: Vec<PreprocessRecovery>,
    pub(super) dynamic_recoveries: Vec<DynamicPreprocessRecovery>,
}

struct RowWithColumns {
    source: NodeId,
    cells: Vec<TableCell>,
    columns: Vec<usize>,
    terminal: TableTerminalRow,
}

#[allow(clippy::struct_excessive_bools)] // Each flag maps to an independent tbl cell recovery invariant.
#[derive(Default)]
struct RawCell {
    text: String,
    text_block: bool,
    /// A request inside `T{…T}` may contribute no flattened text (for
    /// example `.Nm` with its default name), but its source remains semantic
    /// content that the engine reconstructs from the retained source span.
    semantic_content: bool,
    has_invalid_input_bytes: bool,
    has_valid_utf8_non_ascii: bool,
}

struct PendingTextBlock {
    source: NodeId,
    cells: Vec<RawCell>,
}

struct ParsedDataRows {
    rows: Vec<RowWithColumns>,
    unclosed_text_blocks: Vec<NodeId>,
    recoveries: Vec<TableDataRecovery>,
}

enum TableDataRecovery {
    ExtraCells {
        source: NodeId,
        offset: usize,
        text: Box<str>,
    },
    Macro {
        source: NodeId,
        name: Box<str>,
        arguments: Box<str>,
    },
    SpannedData {
        source: NodeId,
        offset: usize,
        text: Box<str>,
    },
}

#[allow(clippy::too_many_lines)] // tbl layout/data synchronization is intentionally one bounded state pass.
pub(super) fn parse_table_rows(
    builder: &DocumentBuilder,
    table_opener: NodeId,
    nodes: &[NodeId],
    limits: &Limits,
) -> ParsedTable {
    let lines = nodes
        .iter()
        .filter_map(|node| source_line(builder, *node))
        .collect::<Vec<_>>();
    let lines = join_table_continuations(lines);
    let ParsedLayout {
        mut layout,
        mut data_start,
        mut dynamic_recoveries,
    } = parse_layout(builder, &lines);
    if layout.rows.is_empty() {
        return ParsedTable {
            rows: Vec::new(),
            limit: None,
            recoveries: Vec::new(),
            dynamic_recoveries,
        };
    }
    let mut rows = Vec::new();
    let mut unclosed_text_blocks = Vec::new();
    let mut data_recoveries = Vec::new();
    loop {
        let reset = lines[data_start..]
            .iter()
            .position(|line| line.macro_name.as_deref() == Some("T&"));
        let data_end = reset.map_or(lines.len(), |offset| data_start + offset);
        let parsed = parse_data_rows(&lines[data_start..data_end], &layout);
        rows.extend(parsed.rows);
        let boundary = reset.map_or(table_opener, |offset| lines[data_start + offset].source);
        let boundary_name = if reset.is_some() { "T&" } else { "TE" };
        unclosed_text_blocks.extend(
            parsed
                .unclosed_text_blocks
                .into_iter()
                .map(|source| (source, boundary, boundary_name)),
        );
        data_recoveries.extend(parsed.recoveries);
        let Some(reset) = reset else {
            break;
        };
        let after_reset = data_start + reset + 1;
        let ParsedLayout {
            layout: next_layout,
            data_start: next_data_offset,
            dynamic_recoveries: next_dynamic_recoveries,
        } = parse_layout_with_delimiter(builder, &lines[after_reset..], layout.delimiter);
        dynamic_recoveries.extend(next_dynamic_recoveries);
        if next_layout.rows.is_empty() {
            break;
        }
        layout = next_layout;
        data_start = after_reset + next_data_offset;
        if data_start >= lines.len() {
            break;
        }
    }
    apply_vertical_spans(&mut rows);
    // Preserve delimiter-wrapped inline equations in the table payload.  The
    // engine lowers a table cell with the complete source-order delimiter
    // state, which is where it can emit a typed `Inline::Code` node and share
    // its bounded normalization cache across ordinary and text-block cells.
    // Flattening an equation here loses that semantic boundary and turns the
    // result into indistinguishable prose downstream.
    let limit = enforce_table_limits(&mut rows, &layout, limits);
    if let Some(first) = rows.first_mut() {
        first.terminal.starts_table = true;
    }
    let rows = rows
        .into_iter()
        .map(|row| ParsedRow {
            source: row.source,
            cells: row.cells,
            terminal: row.terminal,
        })
        .collect::<Vec<_>>();
    dynamic_recoveries.extend(unclosed_text_blocks.into_iter().map(
        |(_source, boundary, boundary_name)| DynamicPreprocessRecovery {
            code: crate::DiagnosticCode::TBL_UNCLOSED_TEXT_BLOCK,
            severity: crate::Severity::Error,
            message: format!("data block open at end of tbl: {boundary_name}").into(),
            location: builder.node_location(boundary),
        },
    ));
    let recoveries = Vec::new();
    dynamic_recoveries.extend(data_recoveries.into_iter().map(|recovery| {
        match recovery {
            TableDataRecovery::ExtraCells {
                source,
                offset,
                text,
            } => DynamicPreprocessRecovery {
                code: crate::DiagnosticCode::TBL_EXTRA_DATA_CELLS,
                severity: crate::Severity::Error,
                message: format!("ignoring extra tbl data cells: {text}").into(),
                location: table_source_location(builder, source, offset),
            },
            TableDataRecovery::Macro {
                source,
                name,
                arguments,
            } => DynamicPreprocessRecovery {
                code: crate::DiagnosticCode::TBL_MACRO,
                severity: crate::Severity::Unsupported,
                message: format!(
                    "ignoring macro in table: {}{}",
                    name,
                    if arguments.is_empty() {
                        String::new()
                    } else {
                        format!(" {arguments}")
                    }
                )
                .into(),
                // Scanner control elements begin at their macro name (column 2
                // after the leading roff control character), matching mandoc's
                // `tbl_cdata()` diagnostic position. A nested `.TS` is read
                // as a tbl control rather than an ordinary macro, so mandoc
                // points after its two-letter request name.
                location: table_source_location(
                    builder,
                    source,
                    if name.as_ref() == "TS" { 2 } else { 0 },
                ),
            },
            TableDataRecovery::SpannedData {
                source,
                offset,
                text,
            } => DynamicPreprocessRecovery {
                code: crate::DiagnosticCode::TBL_SPANNED_DATA,
                severity: crate::Severity::Error,
                message: format!("ignoring data in spanned tbl cell: {text}").into(),
                location: table_source_location(builder, source, offset),
            },
        }
    }));
    if rows.is_empty() {
        dynamic_recoveries.push(DynamicPreprocessRecovery {
            code: crate::DiagnosticCode::TBL_NO_DATA,
            severity: crate::Severity::Error,
            message: "tbl without any data cells".into(),
            location: builder.node_location(table_opener),
        });
    }
    ParsedTable {
        rows,
        limit,
        recoveries,
        dynamic_recoveries,
    }
}

/// tbl consumes a trailing odd escape together with the following text line.
///
/// This happens after scanner-stage source retention: the merged source keeps
/// the final physical line's position, matching the legacy table-row
/// projection, while its text carries the complete logical field.
fn join_table_continuations(lines: Vec<SourceLine>) -> Vec<SourceLine> {
    let mut lines = lines.into_iter().peekable();
    let mut output = Vec::new();
    while let Some(mut line) = lines.next() {
        while line.macro_name.is_none()
            && has_trailing_odd_escape(&line.text)
            && lines.peek().is_some_and(|next| next.macro_name.is_none())
        {
            let mut next = lines.next().expect("a peeked table line exists");
            let has_table_input_text =
                line.table_input_text.is_some() || next.table_input_text.is_some();
            let mut table_input_text = line
                .table_input_text
                .take()
                .map_or_else(|| line.text.clone(), Into::into);
            let next_table_input_text = next
                .table_input_text
                .take()
                .map_or_else(|| next.text.clone(), Into::into);
            let _ = line.text.pop();
            line.text.push_str(&next.text);
            if has_table_input_text {
                if table_input_text.ends_with('\\') {
                    let _ = table_input_text.pop();
                }
                table_input_text.push_str(&next_table_input_text);
                line.table_input_text = Some(table_input_text.into_boxed_str());
            }
            line.source = next.source;
            line.has_invalid_input_bytes |= next.has_invalid_input_bytes;
            line.has_valid_utf8_non_ascii |= next.has_valid_utf8_non_ascii;
        }
        output.push(line);
    }
    output
}

fn has_trailing_odd_escape(text: &str) -> bool {
    let trailing = text.bytes().rev().take_while(|byte| *byte == b'\\').count();
    trailing % 2 == 1
}

pub(super) fn source_line(builder: &DocumentBuilder, node: NodeId) -> Option<SourceLine> {
    match builder.node_kind(node) {
        Some(NodeKind::Text) => Some(SourceLine {
            source: node,
            text: builder.node_text(node)?.to_owned(),
            macro_name: None,
            layout_control_prefix: false,
            has_invalid_input_bytes: builder.node_has_invalid_input_bytes(node),
            has_valid_utf8_non_ascii: builder.node_has_valid_utf8_non_ascii(node),
            table_input_text: builder.node_table_input_text(node).map(Into::into),
        }),
        // tbl text blocks can contain ordinary roff macro requests.  They are
        // scanner elements by this point; preserving their expanded argument
        // text keeps the table cell visible without re-reading host source.
        Some(NodeKind::Element) => {
            let macro_name = builder.node_macro_name(node);
            let text = builder
                .children(node)
                .into_iter()
                .flatten()
                .filter_map(|child| builder.node_text(*child))
                .collect::<Vec<_>>()
                .join(" ");
            // A bare `.` has an empty roff request name.  The scanner places
            // its node at the byte after the control character, but tbl's
            // layout grammar consumes the spelling as its terminator.
            let layout_control_prefix = macro_name == Some("") && text.is_empty();
            Some(SourceLine {
                source: node,
                text: if layout_control_prefix {
                    ".".to_owned()
                } else {
                    text
                },
                macro_name: macro_name.map(Into::into),
                layout_control_prefix,
                has_invalid_input_bytes: builder
                    .children(node)
                    .into_iter()
                    .flatten()
                    .any(|child| builder.node_has_invalid_input_bytes(*child)),
                has_valid_utf8_non_ascii: builder
                    .children(node)
                    .into_iter()
                    .flatten()
                    .any(|child| builder.node_has_valid_utf8_non_ascii(*child)),
                table_input_text: None,
            })
        }
        _ => None,
    }
}

fn parse_layout(builder: &DocumentBuilder, lines: &[SourceLine]) -> ParsedLayout {
    parse_layout_with_delimiter(builder, lines, '\t')
}

#[allow(clippy::too_many_lines)] // tbl's layout grammar is stateful across rows and modifiers.
fn parse_layout_with_delimiter(
    builder: &DocumentBuilder,
    lines: &[SourceLine],
    delimiter: char,
) -> ParsedLayout {
    let mut layout = TableLayout {
        delimiter,
        ..TableLayout::default()
    };
    let mut dynamic_recoveries = Vec::new();
    // tbl permits a line containing only one or two leading `|` tokens before
    // the next real layout row.  It is device geometry rather than a public
    // table cell, so carry it only until that next row is materialized.
    let mut pending_leading_vertical_bars = 0_usize;
    for (index, line) in lines.iter().enumerate() {
        let raw = line.text.as_str();
        let mut format_offset = raw.len() - raw.trim_start().len();
        let mut format = raw.trim();
        if let Some((options, remainder)) = format.rsplit_once(';') {
            parse_table_options(
                builder,
                line,
                options,
                format_offset,
                &mut layout,
                &mut dynamic_recoveries,
            );
            format_offset += options.len() + 1 + (remainder.len() - remainder.trim_start().len());
            format = remainder.trim();
        } else if format.contains("tab(") {
            parse_table_options(
                builder,
                line,
                format,
                format_offset,
                &mut layout,
                &mut dynamic_recoveries,
            );
            continue;
        }
        let ParsedLayoutLine {
            rows,
            leading_vertical_bars,
            complete,
            complete_offset,
        } = parse_layout_line(format);
        let has_rows = !rows.is_empty();
        for mut parsed in rows {
            if pending_leading_vertical_bars != 0 {
                if let Some(first) = parsed.terminal_cells.first_mut() {
                    first.before_vertical_rules = u8::try_from(
                        usize::from(first.before_vertical_rules)
                            .saturating_add(pending_leading_vertical_bars),
                    )
                    .unwrap_or(u8::MAX);
                    first.leading_vertical_from_standalone = true;
                }
                pending_leading_vertical_bars = 0;
            }
            for offset in parsed.vertical_bar_offsets {
                dynamic_recoveries.push(DynamicPreprocessRecovery {
                    code: crate::DiagnosticCode::TBL_VERTICAL_BAR,
                    severity: crate::Severity::Warning,
                    message: "skipping vertical bar in tbl layout".into(),
                    location: table_layout_location(builder, line, format_offset + offset),
                });
            }
            for issue in parsed.invalid_fonts {
                let location =
                    table_layout_location(builder, line, format_offset + issue.font_start);
                dynamic_recoveries.push(DynamicPreprocessRecovery {
                    code: crate::DiagnosticCode::TBL_UNKNOWN_FONT,
                    severity: crate::Severity::Warning,
                    message: format!("unknown font, skipping request: TS {}", issue.request).into(),
                    location,
                });
            }
            for issue in parsed.excessive_spacings {
                dynamic_recoveries.push(DynamicPreprocessRecovery {
                    code: crate::DiagnosticCode::TBL_EXCESSIVE_SPACING,
                    severity: crate::Severity::Error,
                    message: format!("ignoring excessive spacing in tbl layout: {}", issue.value)
                        .into(),
                    location: table_layout_location(
                        builder,
                        line,
                        format_offset + issue.spacing_start,
                    ),
                });
            }
            if parsed.cells.first() == Some(&CellFormat::Span) {
                dynamic_recoveries.push(DynamicPreprocessRecovery {
                    code: crate::DiagnosticCode::TBL_LEADING_SPAN,
                    severity: crate::Severity::Warning,
                    message: "tbl line starts with span".into(),
                    location: table_layout_location(
                        builder,
                        line,
                        format_offset + parsed.first_cell_offset.unwrap_or_default(),
                    ),
                });
            }
            if layout.rows.is_empty() {
                for offset in parsed.leading_down_offsets {
                    dynamic_recoveries.push(DynamicPreprocessRecovery {
                        code: crate::DiagnosticCode::TBL_LEADING_DOWN,
                        severity: crate::Severity::Warning,
                        message: "tbl column starts with span".into(),
                        location: table_layout_location(builder, line, format_offset + offset),
                    });
                }
            }
            if !parsed.cells.is_empty() {
                layout.rows.push(parsed.cells);
                layout.terminal_rows.push(parsed.terminal_cells);
            }
        }
        if !has_rows && leading_vertical_bars != 0 {
            pending_leading_vertical_bars = pending_leading_vertical_bars
                .saturating_add(leading_vertical_bars)
                .min(2);
        }
        if complete {
            if layout.rows.is_empty() {
                dynamic_recoveries.push(DynamicPreprocessRecovery {
                    code: crate::DiagnosticCode::TBL_EMPTY_LAYOUT,
                    severity: crate::Severity::Error,
                    message: "empty tbl layout".into(),
                    location: complete_offset.and_then(|offset| {
                        // tbl reports this recovery just after the layout
                        // terminator.  A bare roff `.` source node already
                        // starts at that byte because scanner control spans
                        // exclude the introducer.
                        table_layout_location(
                            builder,
                            line,
                            format_offset + offset + usize::from(!line.layout_control_prefix),
                        )
                    }),
                });
                // tbl recovers an empty completed layout as one left-aligned
                // column, so its following physical rows remain visible.
                layout.rows.push(vec![CellFormat::Left]);
                layout.terminal_rows.push(vec![TableTerminalCell {
                    before_vertical_rules: u8::try_from(pending_leading_vertical_bars)
                        .unwrap_or(u8::MAX),
                    leading_vertical_from_standalone: pending_leading_vertical_bars != 0,
                    after_vertical_rules: 0,
                    horizontal_rule: TableTerminalBorder::None,
                    span: false,
                    vertical_continuation: false,
                    numeric: false,
                    width_ignored: false,
                    width_expanding: false,
                    spacing: None,
                    minimum_width: None,
                    font: TableTerminalFont::Roman,
                }]);
            }
            return ParsedLayout {
                layout,
                data_start: index + 1,
                dynamic_recoveries,
            };
        }
    }
    ParsedLayout {
        layout: TableLayout::default(),
        data_start: lines.len(),
        dynamic_recoveries,
    }
}

fn table_layout_location(
    builder: &DocumentBuilder,
    line: &SourceLine,
    offset: usize,
) -> Option<crate::SourceSpan> {
    table_source_location(builder, line.source, offset)
}

fn table_source_location(
    builder: &DocumentBuilder,
    source: NodeId,
    offset: usize,
) -> Option<crate::SourceSpan> {
    let span = builder.node_location(source)?;
    let start = span.start.checked_add(u32::try_from(offset).ok()?)?;
    SourceSpan::new(span.source, start, start.saturating_add(1)).ok()
}

#[allow(clippy::too_many_lines)] // Option diagnostics preserve exact cursor recovery order.
fn parse_table_options(
    builder: &DocumentBuilder,
    line: &SourceLine,
    value: &str,
    value_offset: usize,
    layout: &mut TableLayout,
    recoveries: &mut Vec<DynamicPreprocessRecovery>,
) {
    let bytes = value.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        while bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b',')
        {
            cursor += 1;
        }
        let Some(byte) = bytes.get(cursor).copied() else {
            break;
        };
        if !byte.is_ascii_alphabetic() {
            push_table_option_recovery(
                builder,
                line,
                value_offset + cursor,
                recoveries,
                crate::DiagnosticCode::TBL_OPTION_CHARACTER,
                crate::Severity::Error,
                format!(
                    "non-alphabetic character in tbl options: {}",
                    char::from(byte)
                ),
            );
            cursor += 1;
            continue;
        }
        let name_start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic) {
            cursor += 1;
        }
        let name = &value[name_start..cursor];
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let argument = if bytes.get(cursor) == Some(&b'(') {
            let argument_start = cursor + 1;
            if let Some(relative_end) = bytes[argument_start..]
                .iter()
                .position(|byte| *byte == b')')
            {
                let argument_end = argument_start + relative_end;
                cursor = argument_end + 1;
                Some((argument_start, &value[argument_start..argument_end]))
            } else {
                cursor = bytes.len();
                None
            }
        } else {
            None
        };
        match (name, argument) {
            ("tab", Some((_, argument))) if argument.chars().count() == 1 => {
                layout.delimiter = argument.chars().next().expect("one character was checked");
            }
            ("tab", Some((argument_start, argument))) => {
                push_table_option_recovery(
                    builder,
                    line,
                    value_offset + argument_start,
                    recoveries,
                    crate::DiagnosticCode::TBL_OPTION_ARGUMENT_SIZE,
                    crate::Severity::Error,
                    format!(
                        "wrong tbl option argument size: tab want 1 have {}",
                        argument.chars().count()
                    ),
                );
            }
            ("tab", None) => {
                push_table_option_recovery(
                    builder,
                    line,
                    value_offset + cursor,
                    recoveries,
                    crate::DiagnosticCode::TBL_OPTION_ARGUMENT,
                    crate::Severity::Error,
                    "missing tbl option argument: tab".to_owned(),
                );
            }
            ("decimalpoint", Some((_, argument))) if argument.chars().count() == 1 => {}
            ("decimalpoint", Some((argument_start, argument))) => {
                push_table_option_recovery(
                    builder,
                    line,
                    value_offset + argument_start,
                    recoveries,
                    crate::DiagnosticCode::TBL_OPTION_ARGUMENT_SIZE,
                    crate::Severity::Error,
                    format!(
                        "wrong tbl option argument size: decimalpoint want 1 have {}",
                        argument.chars().count()
                    ),
                );
            }
            ("decimalpoint", None) => {
                push_table_option_recovery(
                    builder,
                    line,
                    value_offset + cursor,
                    recoveries,
                    crate::DiagnosticCode::TBL_OPTION_ARGUMENT,
                    crate::Severity::Error,
                    "missing tbl option argument: decimalpoint".to_owned(),
                );
            }
            ("delim", Some((argument_start, argument))) => {
                push_table_option_recovery(
                    builder,
                    line,
                    value_offset + argument_start,
                    recoveries,
                    crate::DiagnosticCode::TBL_EQN_DELIMITER_OPTION,
                    crate::Severity::Unsupported,
                    format!("eqn delim option in tbl: {argument}"),
                );
            }
            ("box", None) => layout.outer_border = TableTerminalBorder::Single,
            ("doublebox", None) => layout.outer_border = TableTerminalBorder::Double,
            ("allbox", None) => {
                layout.all_box = true;
                if layout.outer_border == TableTerminalBorder::None {
                    layout.outer_border = TableTerminalBorder::Single;
                }
            }
            ("center", None) => layout.centered = true,
            ("expand" | "nokeep" | "nowarn", None) => {}
            (name, _) => {
                push_table_option_recovery(
                    builder,
                    line,
                    value_offset + name_start,
                    recoveries,
                    crate::DiagnosticCode::TBL_UNKNOWN_OPTION,
                    crate::Severity::Error,
                    format!("skipping unknown tbl option: {name}"),
                );
            }
        }
    }
}

fn push_table_option_recovery(
    builder: &DocumentBuilder,
    line: &SourceLine,
    offset: usize,
    recoveries: &mut Vec<DynamicPreprocessRecovery>,
    code: &'static str,
    severity: crate::Severity,
    message: String,
) {
    recoveries.push(DynamicPreprocessRecovery {
        code,
        severity,
        message: message.into_boxed_str(),
        location: table_layout_location(builder, line, offset),
    });
}

#[cfg(test)]
pub(super) fn parse_format_row(value: &str) -> Vec<CellFormat> {
    parse_layout_line(value)
        .rows
        .into_iter()
        .next()
        .map_or_else(Vec::new, |row| row.cells)
}

pub(super) fn parse_layout_line(value: &str) -> ParsedLayoutLine {
    let mut rows = Vec::new();
    let mut row = ParsedLayoutRow::default();
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut complete = false;
    let mut complete_offset = None;
    while index < bytes.len() {
        while index < bytes.len() && matches!(bytes[index], b' ' | b'\t' | b'|') {
            if bytes[index] == b'|' {
                // Leading bars belong to the layout row.  tbl accepts two and
                // diagnoses every following one at its own source byte.
                if row.leading_vertical_bars >= 2 {
                    row.vertical_bar_offsets.push(index);
                } else {
                    row.leading_vertical_bars += 1;
                }
            }
            index += 1;
        }
        if index == bytes.len() {
            break;
        }
        if bytes[index] == b',' {
            if !row.cells.is_empty() {
                rows.push(row);
            }
            row = ParsedLayoutRow::default();
            index += 1;
            continue;
        }
        if bytes[index] == b'.' {
            complete = true;
            complete_offset = Some(index);
            break;
        }
        let Some(cell) = table_format_cell(bytes[index]) else {
            index += 1;
            continue;
        };
        row.first_cell_offset.get_or_insert(index);
        if cell == CellFormat::Down {
            row.leading_down_offsets.push(index);
        }
        let mut terminal = TableTerminalCell {
            // Leading layout bars describe the table's outer frame before
            // the first physical field only.  Repeating them on every cell
            // turns a leading `|` into spurious inter-column borders in the
            // terminal grid.
            before_vertical_rules: u8::try_from(if row.cells.is_empty() {
                row.leading_vertical_bars
            } else {
                0
            })
            .unwrap_or(u8::MAX),
            leading_vertical_from_standalone: false,
            after_vertical_rules: 0,
            horizontal_rule: match cell {
                CellFormat::HorizontalRule => TableTerminalBorder::Single,
                CellFormat::DoubleHorizontalRule => TableTerminalBorder::Double,
                _ => TableTerminalBorder::None,
            },
            span: cell == CellFormat::Span,
            vertical_continuation: cell == CellFormat::Down,
            numeric: cell == CellFormat::Number,
            width_ignored: false,
            width_expanding: false,
            spacing: None,
            minimum_width: None,
            font: TableTerminalFont::Roman,
        };
        row.cells.push(cell);
        index += 1;
        let mut vertical_bars = 0;
        consume_table_format_modifiers(
            bytes,
            &mut index,
            &mut terminal,
            &mut row.invalid_fonts,
            &mut row.excessive_spacings,
            &mut row.vertical_bar_offsets,
            &mut vertical_bars,
        );
        row.terminal_cells.push(terminal);
    }
    let leading_vertical_bars = if row.cells.is_empty() {
        row.leading_vertical_bars
    } else {
        0
    };
    if !row.cells.is_empty() {
        rows.push(row);
    }
    ParsedLayoutLine {
        rows,
        leading_vertical_bars,
        complete,
        complete_offset,
    }
}

fn table_format_cell(byte: u8) -> Option<CellFormat> {
    match byte.to_ascii_lowercase() {
        b'l' | b'a' => Some(CellFormat::Left),
        b'c' => Some(CellFormat::Center),
        b'r' => Some(CellFormat::Right),
        b'n' => Some(CellFormat::Number),
        b's' => Some(CellFormat::Span),
        b'^' => Some(CellFormat::Down),
        b'-' | b'_' => Some(CellFormat::HorizontalRule),
        b'=' => Some(CellFormat::DoubleHorizontalRule),
        _ => None,
    }
}

/// Consume the tbl(7) layout modifiers following one cell descriptor.
///
/// The modifiers are presentation-only for our owned row projection.  They
/// still have to be skipped faithfully: font names and parenthesized width
/// expressions can contain `l`, `c`, `r`, and `a`, which would otherwise be
/// misread as additional columns.
#[allow(clippy::too_many_lines)] // Modifier recovery shares one byte cursor to avoid reclassification.
fn consume_table_format_modifiers(
    bytes: &[u8],
    index: &mut usize,
    terminal: &mut TableTerminalCell,
    invalid_fonts: &mut Vec<TableFontIssue>,
    excessive_spacings: &mut Vec<TableSpacingIssue>,
    vertical_bar_offsets: &mut Vec<usize>,
    vertical_bars: &mut usize,
) {
    while *index < bytes.len() {
        while *index < bytes.len() && matches!(bytes[*index], b' ' | b'\t') {
            *index += 1;
        }
        let Some(&byte) = bytes.get(*index) else {
            return;
        };
        if matches!(
            byte,
            b'.' | b','
                | b'-'
                | b'='
                | b'^'
                | b'_'
                | b'A'
                | b'C'
                | b'L'
                | b'N'
                | b'R'
                | b'S'
                | b'a'
                | b'c'
                | b'l'
                | b'n'
                | b'r'
                | b's'
        ) {
            return;
        }
        if byte == b'(' {
            *index += 1;
            while *index < bytes.len() && bytes[*index] != b')' {
                *index += 1;
            }
            *index += usize::from(*index < bytes.len());
            continue;
        }
        if byte.is_ascii_digit() {
            let spacing_start = *index;
            while *index < bytes.len() && bytes[*index].is_ascii_digit() {
                *index += 1;
            }
            let value = &bytes[spacing_start..*index];
            if std::str::from_utf8(value)
                .ok()
                .and_then(|value| value.parse::<u16>().ok())
                .is_some_and(|value| value > 9)
            {
                excessive_spacings.push(TableSpacingIssue {
                    spacing_start,
                    value: String::from_utf8_lossy(value).into(),
                });
            } else if let Ok(value) = std::str::from_utf8(value)
                && let Ok(value) = value.parse::<u8>()
            {
                terminal.spacing = Some(value);
            }
            continue;
        }
        let modifier_start = *index;
        *index += 1;
        if byte == b'|' {
            if *vertical_bars >= 2 {
                vertical_bar_offsets.push(modifier_start);
            } else {
                *vertical_bars += 1;
                terminal.after_vertical_rules = u8::try_from(*vertical_bars).unwrap_or(u8::MAX);
            }
            continue;
        }
        match byte.to_ascii_lowercase() {
            b'b' => terminal.font = TableTerminalFont::Bold,
            b'i' => terminal.font = TableTerminalFont::Italic,
            b'z' => terminal.width_ignored = true,
            b'x' => terminal.width_expanding = true,
            b'f' => {
                while *index < bytes.len() && matches!(bytes[*index], b' ' | b'\t') {
                    *index += 1;
                }
                if bytes.get(*index) == Some(&b'(') {
                    continue;
                }
                let font_start = *index;
                let mut font_end = font_start;
                if font_end < bytes.len() {
                    font_end += 1;
                    if bytes
                        .get(font_end)
                        .is_some_and(|byte| !matches!(byte, b' ' | b'\t' | b'.'))
                    {
                        font_end += 1;
                    }
                }
                let font = &bytes[font_start..font_end];
                if !is_legacy_table_font(font) {
                    invalid_fonts.push(TableFontIssue {
                        font_start,
                        request: String::from_utf8_lossy(&bytes[modifier_start..]).into(),
                    });
                }
                terminal.font = table_terminal_font(font);
                *index = font_end;
            }
            b'p' | b'v' => {
                if bytes
                    .get(*index)
                    .is_some_and(|byte| matches!(byte, b'+' | b'-'))
                {
                    *index += 1;
                }
                while *index < bytes.len() && bytes[*index].is_ascii_digit() {
                    *index += 1;
                }
            }
            b'w' => {
                if bytes.get(*index) == Some(&b'(') {
                    *index += 1;
                    let width_start = *index;
                    while *index < bytes.len() && bytes[*index] != b')' {
                        *index += 1;
                    }
                    terminal.minimum_width = table_terminal_width(&bytes[width_start..*index]);
                    *index += usize::from(*index < bytes.len());
                } else {
                    let width_start = *index;
                    while *index < bytes.len() && bytes[*index].is_ascii_digit() {
                        *index += 1;
                    }
                    terminal.minimum_width = table_terminal_width(&bytes[width_start..*index]);
                }
            }
            _ => {}
        }
    }
}

/// Convert tbl's explicit `w` layout modifier to terminal character cells.
///
/// The terminal shares roff's 24-basic-unit grid: an unsuffixed width and
/// `n` use one cell per unit, while physical scales round up to the next
/// terminal cell.  Invalid or non-positive values don't constrain a column.
fn table_terminal_width(value: &[u8]) -> Option<u16> {
    let value = std::str::from_utf8(value).ok()?.trim();
    if value.is_empty() {
        return None;
    }
    let mut numeric = None;
    for end in value
        .char_indices()
        .map(|(index, _)| index)
        .skip(1)
        .chain(std::iter::once(value.len()))
    {
        if let Ok(scale) = value[..end].parse::<f64>()
            && scale.is_finite()
        {
            numeric = Some((end, scale));
        }
    }
    let (end, scale) = numeric?;
    let multiplier = match value[end..].chars().next() {
        Some('c') => 240.0 / 2.54,
        Some('i') => 240.0,
        Some('f') => 65_536.0,
        Some('M') => 0.24,
        Some('P' | 'v') => 40.0,
        Some('p') => 10.0 / 3.0,
        Some('u') => 1.0,
        _ => 24.0,
    };
    let basic = (scale * multiplier).trunc();
    if !basic.is_finite() || basic <= 0.0 {
        return None;
    }
    // The terminal field is ultimately a `u16`; reject overlarge finite
    // values before the deliberate whole-cell conversion rather than relying
    // on an unchecked float-to-integer truncation.
    if basic > f64::from(u16::MAX) * 24.0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let cells = ((basic as u128).saturating_add(11) / 24).max(1);
    u16::try_from(cells).ok()
}

fn table_terminal_font(font: &[u8]) -> TableTerminalFont {
    match font {
        b"B" | b"3" | b"BI" | b"CB" | b"VB" => TableTerminalFont::Bold,
        b"I" | b"2" | b"CI" | b"VI" => TableTerminalFont::Italic,
        _ => TableTerminalFont::Roman,
    }
}

fn is_legacy_table_font(font: &[u8]) -> bool {
    matches!(
        font,
        b"C" | b"V"
            | b"B"
            | b"3"
            | b"I"
            | b"2"
            | b"R"
            | b"1"
            | b"4"
            | b"BI"
            | b"CB"
            | b"CI"
            | b"CR"
            | b"CW"
            | b"VB"
            | b"VI"
    )
}

#[allow(clippy::too_many_lines)] // tbl's pending text-block state is intentionally linear.
fn parse_data_rows(lines: &[SourceLine], layout: &TableLayout) -> ParsedDataRows {
    let mut output = Vec::new();
    // Horizontal rules are public zero-cell rows, but upstream tbl does not
    // advance the format row for them.  Track real data rows independently
    // from the published row count so an intervening rule cannot shift the
    // alignment of the next data row.
    let mut data_row_count = 0;
    let mut pending = None::<PendingTextBlock>;
    let mut recoveries = Vec::new();
    for line in lines {
        // A nested `.TS` does not start a second table: tbl rejects it as a
        // macro inside the active range.  Do this before empty-text handling,
        // because scanner-stage `.TS` is an Element with no text children;
        // otherwise it would leak as an empty generated table row.
        if line.macro_name.as_deref() == Some("TS") {
            recoveries.push(TableDataRecovery::Macro {
                source: line.source,
                name: "TS".into(),
                arguments: String::new().into(),
            });
            continue;
        }
        let text = line
            .table_input_text
            .as_deref()
            .unwrap_or(&line.text)
            .trim_end();
        // An argument-less control inside `T{…T}` (notably `.Nm`) still has
        // source semantics, even though its scanner text payload is empty.
        // Let the pending text-block path retain that fact instead of
        // collapsing it into an unrelated empty tbl row.
        if text.is_empty() && line.macro_name.is_none() {
            if pending.is_none() {
                data_row_count = advance_past_horizontal_layout_rows(
                    &mut output,
                    line.source,
                    layout,
                    data_row_count,
                );
                push_table_row(&mut output, line.source, Vec::new(), layout, data_row_count);
                data_row_count = data_row_count.saturating_add(1);
            }
            continue;
        }
        if is_table_rule(text) {
            // The legacy AST projection cannot represent whether this was a
            // single or double rule, but it does retain the span as an empty
            // `Table` node at the physical rule line.
            push_table_row(&mut output, line.source, Vec::new(), layout, data_row_count);
            if let Some(row) = output.last_mut() {
                row.terminal.horizontal_rule = table_data_rule(text);
            }
            continue;
        }
        if let Some(row) = pending.as_mut() {
            if let Some(name) = line.macro_name.as_deref() {
                recoveries.push(TableDataRecovery::Macro {
                    source: line.source,
                    name: name.into(),
                    arguments: text.into(),
                });
                if let Some(cell) = row.cells.last_mut() {
                    cell.semantic_content = true;
                }
            }
            let trimmed = text.trim_start();
            if let Some(remainder) = trimmed.strip_prefix("T}") {
                if let Some(cell) = row.cells.last_mut() {
                    cell.text_block = true;
                }
                if let Some(remainder) = remainder.strip_prefix(layout.delimiter) {
                    let remainder = remainder.trim_start();
                    // A closing text block may share its physical row with
                    // ordinary cells and the opening marker of the next text
                    // block: `T}:middle:T{`.  Preserve those ordinary cells,
                    // then keep the row pending for the newly opened block.
                    if let Some((before, next)) = remainder.split_once("T{")
                        && (before.is_empty() || before.ends_with(layout.delimiter))
                    {
                        let before = before.trim_end_matches(layout.delimiter);
                        if !before.trim().is_empty() {
                            append_plain_cells(
                                &mut row.cells,
                                before,
                                layout.delimiter,
                                line.has_invalid_input_bytes,
                                line.has_valid_utf8_non_ascii,
                            );
                        }
                        row.cells.push(RawCell {
                            text: String::new(),
                            text_block: true,
                            semantic_content: false,
                            has_invalid_input_bytes: line.has_invalid_input_bytes,
                            has_valid_utf8_non_ascii: line.has_valid_utf8_non_ascii,
                        });
                        if !next.trim().is_empty() {
                            append_cell_text(
                                row.cells.last_mut().expect("just pushed"),
                                next,
                                line.has_invalid_input_bytes,
                                line.has_valid_utf8_non_ascii,
                            );
                        }
                        continue;
                    }
                    append_plain_cells(
                        &mut row.cells,
                        remainder,
                        layout.delimiter,
                        line.has_invalid_input_bytes,
                        line.has_valid_utf8_non_ascii,
                    );
                }
                let row = pending.take().expect("pending row exists");
                data_row_count = advance_past_horizontal_layout_rows(
                    &mut output,
                    row.source,
                    layout,
                    data_row_count,
                );
                push_table_row(&mut output, row.source, row.cells, layout, data_row_count);
                data_row_count = data_row_count.saturating_add(1);
            } else if let Some(cell) = row.cells.last_mut() {
                append_cell_text(
                    cell,
                    text,
                    line.has_invalid_input_bytes,
                    line.has_valid_utf8_non_ascii,
                );
            }
            continue;
        }

        if let Some((before, after)) = text.split_once("T{")
            && (before.is_empty() || before.ends_with(layout.delimiter))
        {
            let mut cells = Vec::new();
            let before = before.trim_end_matches(layout.delimiter);
            if !before.is_empty() {
                append_plain_cells(
                    &mut cells,
                    before,
                    layout.delimiter,
                    line.has_invalid_input_bytes,
                    line.has_valid_utf8_non_ascii,
                );
            }
            cells.push(RawCell {
                text: String::new(),
                text_block: true,
                semantic_content: false,
                has_invalid_input_bytes: line.has_invalid_input_bytes,
                has_valid_utf8_non_ascii: line.has_valid_utf8_non_ascii,
            });
            if !after.trim().is_empty() {
                append_cell_text(
                    cells.last_mut().expect("just pushed"),
                    after,
                    line.has_invalid_input_bytes,
                    line.has_valid_utf8_non_ascii,
                );
            }
            pending = Some(PendingTextBlock {
                source: line.source,
                cells,
            });
            continue;
        }

        let mut cells = Vec::new();
        append_plain_cells(
            &mut cells,
            text,
            layout.delimiter,
            line.has_invalid_input_bytes,
            line.has_valid_utf8_non_ascii,
        );
        data_row_count =
            advance_past_horizontal_layout_rows(&mut output, line.source, layout, data_row_count);
        if let Some((offset, text)) =
            table_extra_data_cells(text, layout.delimiter, layout, data_row_count)
        {
            recoveries.push(TableDataRecovery::ExtraCells {
                source: line.source,
                offset,
                text,
            });
        }
        recoveries.extend(
            table_spanned_data_cells(text, layout.delimiter, layout, data_row_count)
                .into_iter()
                .map(|(offset, text)| TableDataRecovery::SpannedData {
                    source: line.source,
                    offset,
                    text,
                }),
        );
        push_table_row(&mut output, line.source, cells, layout, data_row_count);
        data_row_count = data_row_count.saturating_add(1);
    }
    if let Some(row) = pending {
        // Keep malformed but finite text-block input visible.  Later M6
        // recovery validation will attach the dedicated missing-closer code.
        let source = row.source;
        data_row_count =
            advance_past_horizontal_layout_rows(&mut output, source, layout, data_row_count);
        push_table_row(&mut output, source, row.cells, layout, data_row_count);
        return ParsedDataRows {
            rows: output,
            unclosed_text_blocks: vec![source],
            recoveries,
        };
    }
    ParsedDataRows {
        rows: output,
        unclosed_text_blocks: Vec::new(),
        recoveries,
    }
}

fn table_extra_data_cells(
    text: &str,
    delimiter: char,
    layout: &TableLayout,
    data_row_index: usize,
) -> Option<(usize, Box<str>)> {
    // Multi-row layouts advance through tbl's stateful horizontal/vertical
    // insertion rules. Their excess-cell recovery is per-cell rather than a
    // trailing-field summary, so keep this path to the single repeated layout
    // row form that `tbl_data()` diagnoses with the matching prose.
    if layout.rows.len() != 1 {
        return None;
    }
    let layout = layout
        .rows
        .get(data_row_index.min(layout.rows.len().saturating_sub(1)))?;
    // This recovery is only safe for ordinary field-consuming layout rows.
    // Span/down/rule rows have tbl's stateful insertion rules and report their
    // own per-cell diagnostics instead of a trailing-cell summary.
    if !layout.iter().all(|format| {
        matches!(
            format,
            CellFormat::Left | CellFormat::Center | CellFormat::Right | CellFormat::Number
        )
    }) {
        return None;
    }
    let expected = layout
        .iter()
        .filter(|format| **format != CellFormat::Span)
        .count();
    let starts = table_field_starts(text, delimiter);
    let offset = *starts.get(expected)?;
    (offset < text.len()).then(|| (offset, text[offset..].trim().into()))
}

fn table_spanned_data_cells(
    text: &str,
    delimiter: char,
    layout: &TableLayout,
    data_row_index: usize,
) -> Vec<(usize, Box<str>)> {
    let Some(layout) = layout
        .rows
        .get(data_row_index.min(layout.rows.len().saturating_sub(1)))
    else {
        return Vec::new();
    };
    let starts = table_field_starts(text, delimiter);
    let fields = text.split_terminator(delimiter).collect::<Vec<_>>();
    let mut field_index = 0;
    let mut output = Vec::new();
    for format in layout {
        if *format == CellFormat::Span {
            continue;
        }
        let Some(field) = fields.get(field_index) else {
            break;
        };
        let start = starts.get(field_index).copied().unwrap_or_default();
        let field = field.trim();
        if matches!(
            format,
            CellFormat::Down | CellFormat::HorizontalRule | CellFormat::DoubleHorizontalRule
        ) && !field.is_empty()
            && field != r"\^"
        {
            output.push((start, field.into()));
        }
        field_index += 1;
    }
    output
}

fn table_field_starts(text: &str, delimiter: char) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.char_indices().filter_map(|(index, character)| {
            (character == delimiter).then_some(index + character.len_utf8())
        }))
        .collect()
}

fn is_table_rule(text: &str) -> bool {
    matches!(text.trim(), "_" | "=" | "\\_" | "\\=")
}

fn table_data_rule(text: &str) -> TableTerminalBorder {
    match text.trim() {
        "=" | "\\=" => TableTerminalBorder::Double,
        _ => TableTerminalBorder::Single,
    }
}

fn append_plain_cells(
    cells: &mut Vec<RawCell>,
    value: &str,
    delimiter: char,
    has_invalid_input_bytes: bool,
    has_valid_utf8_non_ascii: bool,
) {
    // `tbl_data()` starts a cell only while input remains after the previous
    // delimiter. Thus `one:two:` has two cells, while `one::three` keeps its
    // meaningful middle empty cell. `split_terminator` has exactly that
    // boundary behavior.
    cells.extend(value.split_terminator(delimiter).map(|text| RawCell {
        text: table_field_text(text),
        text_block: false,
        semantic_content: false,
        has_invalid_input_bytes,
        has_valid_utf8_non_ascii,
    }));
}

/// The engine masks terminal-unsafe ASCII controls before parsing.  In a tbl
/// field that turns `\[u0000]` plus the raw control byte into an authored
/// control escape followed by one ordinary space. libmandoc keeps that final
/// field space, while ordinary tbl padding remains ignorable.
pub(super) fn table_field_text(value: &str) -> String {
    let trimmed = value.trim();
    if value.ends_with(char::is_whitespace) && ends_in_ascii_control_unicode_escape(trimmed) {
        format!("{trimmed} ")
    } else {
        trimmed.to_owned()
    }
}

fn ends_in_ascii_control_unicode_escape(value: &str) -> bool {
    let Some(start) = value.rfind(r"\[u") else {
        return false;
    };
    let Some(encoded) = value[start + 3..].strip_suffix(']') else {
        return false;
    };
    u32::from_str_radix(encoded, 16)
        .ok()
        .and_then(char::from_u32)
        .is_some_and(|character| character.is_ascii_control())
}

fn append_cell_text(
    cell: &mut RawCell,
    value: &str,
    has_invalid_input_bytes: bool,
    has_valid_utf8_non_ascii: bool,
) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    if !cell.text.is_empty() {
        cell.text.push(' ');
    }
    cell.text.push_str(value);
    cell.has_invalid_input_bytes |= has_invalid_input_bytes;
    cell.has_valid_utf8_non_ascii |= has_valid_utf8_non_ascii;
}

fn advance_past_horizontal_layout_rows(
    output: &mut Vec<RowWithColumns>,
    source: NodeId,
    layout: &TableLayout,
    mut data_row_index: usize,
) -> usize {
    let max_columns = layout.rows.iter().map(Vec::len).max().unwrap_or_default();
    while data_row_index + 1 < layout.rows.len() {
        let layout_row = &layout.rows[data_row_index];
        let is_horizontal_only = layout_row.len() >= max_columns
            && layout_row.iter().all(|format| {
                matches!(
                    format,
                    CellFormat::HorizontalRule | CellFormat::DoubleHorizontalRule
                )
            });
        if !is_horizontal_only {
            break;
        }
        // `tbl_data()` materializes an empty data span for a full-width
        // horizontal-only format row, then binds the actual input row to the
        // next format row.  The generated span has the triggering data row's
        // source line just like upstream's `newspan()` call.
        push_table_row(output, source, Vec::new(), layout, data_row_index);
        if let Some(row) = output.last_mut() {
            row.terminal.horizontal_rule = if layout_row.contains(&CellFormat::DoubleHorizontalRule)
            {
                TableTerminalBorder::Double
            } else {
                TableTerminalBorder::Single
            };
        }
        data_row_index += 1;
    }
    data_row_index
}

fn push_table_row(
    output: &mut Vec<RowWithColumns>,
    source: NodeId,
    raw_cells: Vec<RawCell>,
    layout: &TableLayout,
    data_row_index: usize,
) {
    let mut terminal = TableTerminalRow {
        starts_table: false,
        cells: layout
            .terminal_rows
            .get(data_row_index.min(layout.terminal_rows.len().saturating_sub(1)))
            .cloned()
            .unwrap_or_default(),
        data_columns: Vec::new(),
        outer_border: layout.outer_border,
        all_box: layout.all_box,
        centered: layout.centered,
        horizontal_rule: TableTerminalBorder::None,
    };
    let layout_row = layout
        .rows
        .get(data_row_index.min(layout.rows.len().saturating_sub(1)))
        .expect("non-empty layout was checked before data parsing");
    let discard_extra_cells = layout.rows.len() == 1
        && layout_row.iter().all(|format| {
            matches!(
                format,
                CellFormat::Left | CellFormat::Center | CellFormat::Right | CellFormat::Number
            )
        });
    let mut cells = Vec::new();
    let mut columns = Vec::new();
    let mut raw = raw_cells.into_iter();
    for (column, format) in layout_row.iter().copied().enumerate() {
        if format == CellFormat::Span {
            continue;
        }
        let Some(raw) = raw.next() else {
            break;
        };
        let vertical_continuation = format == CellFormat::Down || raw.text == "\\^";
        let empty_text_block = raw.text_block && raw.text.is_empty() && !raw.semantic_content;
        let column_span = layout_row[column + 1..]
            .iter()
            .take_while(|format| **format == CellFormat::Span)
            .count()
            .saturating_add(1)
            .try_into()
            .unwrap_or(u16::MAX);
        cells.push(TableCell {
            // mandoc uses a null cell payload for an empty `T{…T}` block;
            // that remains distinguishable from a present, empty inline
            // field in the owned AST.
            text: (!empty_text_block).then(|| {
                table_cell_text(
                    &raw.text,
                    raw.has_invalid_input_bytes,
                    raw.has_valid_utf8_non_ascii,
                )
            }),
            // The upstream parser only marks a text block after it received
            // content; an empty `T{…T}` therefore has the null payload above
            // but no text-block marker in the legacy projection.
            text_block: raw.text_block && !empty_text_block,
            vertical_continuation,
            column_span,
            row_span: 1,
            alignment: format.alignment(),
        });
        columns.push(column);
    }
    // A single ordinary layout row ignores trailing data fields. Multi-row
    // layouts can advance to a wider row, so retain their surplus fields for
    // the later stateful recovery path instead.
    if !discard_extra_cells {
        for raw in raw {
            // A recovery field following a trailing horizontal span starts
            // after the *occupied* prior field, not merely after its first
            // physical column.  The public AST already retains the span;
            // keep the corresponding renderer-private coordinate so a later
            // wider layout row cannot pull the surplus field back into it.
            let next_column = columns
                .last()
                .zip(cells.last())
                .map_or(0, |(column, cell)| {
                    column.saturating_add(usize::from(cell.column_span.max(1)))
                });
            cells.push(TableCell {
                text: Some(table_cell_text(
                    &raw.text,
                    raw.has_invalid_input_bytes,
                    raw.has_valid_utf8_non_ascii,
                )),
                text_block: raw.text_block,
                vertical_continuation: false,
                column_span: 1,
                row_span: 1,
                alignment: TableAlignment::Left,
            });
            columns.push(next_column);
        }
    }
    terminal.data_columns = columns
        .iter()
        .copied()
        .map(|column| u16::try_from(column).unwrap_or(u16::MAX))
        .collect();
    // A blank data line is a real tbl row with no cells.  Retaining it is
    // observable in the owned AST and also preserves its source location for
    // renderers that choose to represent vertical table spacing.
    output.push(RowWithColumns {
        source,
        cells,
        columns,
        terminal,
    });
}

/// libmandoc exposes tbl's raw UTF-8 scalars in roff's `\[u…]` notation.
/// It substitutes ASCII controls and malformed raw bytes with `?`. The latter
/// arrive in the general AST as Latin-1 scalars, so their scanner provenance
/// prevents a valid UTF-8 spelling of the same scalar from being substituted.
pub(super) fn table_cell_text(
    value: &str,
    has_invalid_input_bytes: bool,
    has_valid_utf8_non_ascii: bool,
) -> Box<str> {
    let mut projected = String::with_capacity(value.len());
    for character in value.chars() {
        if (character.is_control()
            && !(has_valid_utf8_non_ascii && matches!(character, '\u{0080}'..='\u{009f}')))
            || (has_invalid_input_bytes && matches!(character, '\u{0080}'..='\u{00ff}'))
        {
            projected.push('?');
        } else if character.is_ascii() {
            projected.push(character);
        } else {
            use std::fmt::Write as _;
            write!(projected, r"\[u{:04X}]", u32::from(character))
                .expect("writing to a String cannot fail");
        }
    }
    projected.into_boxed_str()
}

fn apply_vertical_spans(rows: &mut [RowWithColumns]) {
    for row_index in 0..rows.len() {
        let continuations = rows[row_index]
            .cells
            .iter()
            .enumerate()
            .filter_map(|(cell_index, cell)| {
                cell.vertical_continuation
                    .then_some((cell_index, rows[row_index].columns[cell_index]))
            })
            .collect::<Vec<_>>();
        for (_, column) in continuations {
            for previous_index in (0..row_index).rev() {
                let Some(previous_cell_index) = rows[previous_index]
                    .columns
                    .iter()
                    .enumerate()
                    .find_map(|(index, start)| {
                        let cell = &rows[previous_index].cells[index];
                        let end = start.saturating_add(usize::from(cell.column_span));
                        (*start..end).contains(&column).then_some(index)
                    })
                else {
                    continue;
                };
                if !rows[previous_index].cells[previous_cell_index].vertical_continuation {
                    rows[previous_index].cells[previous_cell_index].row_span = rows[previous_index]
                        .cells[previous_cell_index]
                        .row_span
                        .saturating_add(1);
                    break;
                }
            }
            // tbl keeps the authored field text even when that field is a
            // vertical continuation.  The public legacy projection exposes
            // both facts, which is important for its recovery diagnostics.
        }
    }
}

fn enforce_table_limits(
    rows: &mut Vec<RowWithColumns>,
    _layout: &TableLayout,
    limits: &Limits,
) -> Option<LimitFinding> {
    if rows.len() > limits.max_table_rows {
        rows.truncate(limits.max_table_rows);
        return Some(table_limit(
            crate::DiagnosticCode::LIMIT_TABLE_ROWS,
            "tbl preprocessing exceeds max_table_rows and retained a finite row prefix",
        ));
    }

    let mut cell_count = 0_usize;
    let mut text_bytes = 0_usize;
    let mut failure = None;
    for (row_index, row) in rows.iter().enumerate() {
        if row.columns.len() > limits.max_table_columns {
            failure = Some((
                row_index,
                table_limit(
                    crate::DiagnosticCode::LIMIT_TABLE_COLUMNS,
                    "tbl preprocessing exceeds max_table_columns and retained a finite row prefix",
                ),
            ));
            break;
        }
        if cell_count.saturating_add(row.cells.len()) > limits.max_table_cells {
            failure = Some((
                row_index,
                table_limit(
                    crate::DiagnosticCode::LIMIT_TABLE_CELLS,
                    "tbl preprocessing exceeds max_table_cells and retained a finite row prefix",
                ),
            ));
            break;
        }
        if row.cells.iter().any(|cell| {
            usize::from(cell.column_span) > limits.max_table_span
                || usize::from(cell.row_span) > limits.max_table_span
        }) {
            failure = Some((
                row_index,
                table_limit(
                    crate::DiagnosticCode::LIMIT_TABLE_SPAN,
                    "tbl preprocessing exceeds max_table_span and retained a finite row prefix",
                ),
            ));
            break;
        }
        let row_text_bytes = row
            .cells
            .iter()
            .filter_map(|cell| cell.text.as_deref())
            .map(str::len)
            .sum::<usize>();
        if text_bytes.saturating_add(row_text_bytes) > limits.max_table_text_bytes {
            failure = Some((
                row_index,
                table_limit(
                    crate::DiagnosticCode::LIMIT_TABLE_TEXT_BYTES,
                    "tbl preprocessing exceeds max_table_text_bytes and retained a finite row prefix",
                ),
            ));
            break;
        }
        cell_count = cell_count.saturating_add(row.cells.len());
        text_bytes = text_bytes.saturating_add(row_text_bytes);
    }
    if let Some((row_index, limit)) = failure {
        rows.truncate(row_index);
        Some(limit)
    } else {
        None
    }
}

fn table_limit(code: &'static str, message: &'static str) -> LimitFinding {
    LimitFinding {
        code,
        message,
        location: None,
    }
}
