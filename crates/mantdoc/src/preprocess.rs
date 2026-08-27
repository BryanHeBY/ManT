//! Bounded tbl(7) and eqn(7) preprocessing before macro-package lowering.
//!
//! Roff execution deliberately retains a flat event stream.  The tbl and eqn
//! languages consume ranges of that stream, so they have to run before the
//! man(7) and mdoc(7) structural passes attach ordinary source events to
//! macro-package blocks.  This module keeps the resulting public tree
//! renderer-neutral: every tbl data row is a [`NodeKind::Table`] node and
//! every display equation is a [`NodeKind::Equation`] node.

use crate::{
    Limits, NodeId, NodeKind, SourceSpan, TableAlignment, TableCell,
    ast::{
        DocumentBuilder, EquationTerminal, EquationTerminalToken, TableTerminalBorder,
        TableTerminalCell, TableTerminalFont, TableTerminalRow,
    },
};

const LEGACY_EQUATION_TREE_DEPTH_MESSAGE: &str =
    "equation tree exceeded the 256-level copy limit; deeper equation content was omitted";

/// Bounded preprocessing result consumed by the parser boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PreprocessOutcome {
    /// First preprocessing budget that retained only a finite semantic prefix.
    pub(crate) limit: Option<LimitFinding>,
    /// Recoverable malformed tbl/eqn range findings in source order.
    pub(crate) recoveries: Vec<PreprocessRecovery>,
    /// Recoveries with source-dependent legacy wording.
    pub(crate) dynamic_recoveries: Vec<DynamicPreprocessRecovery>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DynamicPreprocessRecovery {
    pub(crate) code: &'static str,
    pub(crate) severity: crate::Severity,
    pub(crate) message: Box<str>,
    pub(crate) location: Option<crate::SourceSpan>,
}

/// A checked tbl/eqn budget reported at the parser boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LimitFinding {
    /// Stable parser-owned diagnostic code.
    pub(crate) code: &'static str,
    /// Stable explanatory wording independent of the code.
    pub(crate) message: &'static str,
    /// Source location of the range that triggered the budget, when known.
    pub(crate) location: Option<crate::SourceSpan>,
}

/// One malformed but recoverable tbl or eqn structure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreprocessRecovery {
    /// Stable parser-owned diagnostic code.
    pub(crate) code: &'static str,
    /// Stable explanatory wording independent of the code.
    pub(crate) message: &'static str,
    /// Source location of the malformed opener.
    pub(crate) location: Option<crate::SourceSpan>,
}

/// Replace complete `.TS`/`.TE` and `.EQ`/`.EN` source ranges with normalized
/// table rows and display equations.
///
/// A tbl layout can contain multiple format rows.  Each data row is paired
/// with the next layout row (the last layout row repeats), and `S`/`^` cells
/// are retained as the public column- and row-span semantics rather than as
/// visible strings.  Text blocks deliberately become one cell even though
/// their source spans several physical roff events.
#[allow(clippy::too_many_lines)] // The source-range state machine stays linear and auditable.
pub(crate) fn structure(builder: &mut DocumentBuilder, limits: &Limits) -> PreprocessOutcome {
    let mut outcome = PreprocessOutcome::default();
    let root = DocumentBuilder::root();
    let Some(flat) = builder.children(root).map(<[NodeId]>::to_vec) else {
        return outcome;
    };
    let mut output = Vec::new();
    let mut inline_delimiters = None;
    let mut remembered_inline_delimiters = None;
    let mut index = 0;
    while index < flat.len() {
        let node = flat[index];
        match builder.node_macro_name(node) {
            Some("TS") => {
                let Some(end) = find_closer(builder, &flat, index + 1, "TE") else {
                    outcome.recoveries.push(PreprocessRecovery {
                        code: crate::DiagnosticCode::TBL_UNCLOSED_TABLE,
                        message: "tbl .TS range reaches end of input without a matching .TE",
                        location: builder.node_location(node),
                    });
                    output.push(node);
                    index += 1;
                    continue;
                };
                let table = parse_table_rows(builder, node, &flat[index + 1..end], limits);
                if let Some(limit) = table.limit {
                    record_limit(&mut outcome, limit, builder.node_location(node));
                }
                outcome.recoveries.extend(table.recoveries);
                outcome.dynamic_recoveries.extend(table.dynamic_recoveries);
                let no_data = table.rows.is_empty();
                let generated_nodes = table.rows.len().saturating_add(usize::from(no_data));
                if builder.node_count().saturating_add(generated_nodes) > limits.max_nodes {
                    record_limit(
                        &mut outcome,
                        LimitFinding {
                            code: crate::DiagnosticCode::LIMIT_NODES,
                            message: "tbl or eqn preprocessing exceeds max_nodes and retained the raw event",
                            location: builder.node_location(node),
                        },
                        None,
                    );
                    output.extend_from_slice(&flat[index..=end]);
                    index = end + 1;
                    continue;
                }
                if no_data {
                    // `tbl_end()` turns an otherwise empty `.TS` range into
                    // the same public spacing element mandoc would have
                    // retained for the closing request, while the parser
                    // report carries the no-data error at the opener.
                    if let Some(spacing) = builder.push(root, NodeKind::Element) {
                        let closer = flat[end];
                        let _ = builder.macro_name(spacing, "sp");
                        let _ = builder.set_node_preprocessor_opener(spacing, "TS");
                        let _ = builder.set_node_location(spacing, builder.node_location(closer));
                        let _ = builder.set_node_flags(
                            spacing,
                            builder.node_flags(closer).unwrap_or_default(),
                        );
                        output.push(spacing);
                    }
                } else {
                    for (row_index, row) in table.rows.into_iter().enumerate() {
                        let Some(table) = builder.push(root, NodeKind::Table) else {
                            break;
                        };
                        let _ = builder.set_node_location(table, builder.node_location(row.source));
                        let mut flags = builder.node_flags(row.source).unwrap_or_default();
                        // A normalized tbl row is a generated structural event,
                        // not the source text line from which its cells came.
                        // The legacy owned AST therefore never exposes it as a
                        // line-start node, including an empty data row.
                        flags.line_start = false;
                        let _ = builder.set_node_flags(table, flags);
                        let _ = builder.set_node_table_cells(table, row.cells);
                        let _ = builder.set_node_table_terminal(table, row.terminal);
                        if row_index == 0 {
                            let _ = builder.set_node_preprocessor_opener(table, "TS");
                        }
                        output.push(table);
                    }
                }
                index = end + 1;
            }
            Some("EQ") => {
                let Some(end) = find_closer(builder, &flat, index + 1, "EN") else {
                    outcome.recoveries.push(PreprocessRecovery {
                        code: crate::DiagnosticCode::EQN_UNCLOSED_DISPLAY,
                        message: "eqn .EQ range reaches end of input without a matching .EN",
                        location: builder.node_location(node),
                    });
                    output.push(node);
                    index += 1;
                    continue;
                };
                let equation = parse_equation(builder, &flat[index + 1..end], limits);
                if let Some(limit) = equation.limit {
                    record_limit(&mut outcome, limit, builder.node_location(node));
                }
                if equation.recursive_definition {
                    outcome.recoveries.push(PreprocessRecovery {
                        code: crate::DiagnosticCode::EQN_RECURSIVE_DEFINITION,
                        message: "input stack limit exceeded, infinite loop?",
                        location: builder.node_location(node),
                    });
                }
                if let Some(request) = equation
                    .empty_request
                    .as_deref()
                    .filter(|request| !request.is_empty())
                {
                    outcome.dynamic_recoveries.push(DynamicPreprocessRecovery {
                        code: crate::DiagnosticCode::EQN_EMPTY_REQUEST,
                        severity: crate::Severity::Warning,
                        message: format!("skipping empty request: {request}").into(),
                        location: builder.node_location(node),
                    });
                }
                for operator in &equation.missing_boxes {
                    outcome.dynamic_recoveries.push(DynamicPreprocessRecovery {
                        code: crate::DiagnosticCode::EQN_MISSING_BOX,
                        severity: crate::Severity::Warning,
                        message: format!("missing eqn box, using \"\": {operator}").into(),
                        location: builder.node_location(node),
                    });
                }
                let expression = equation.expression;
                let terminal = equation.terminal;
                let has_delimiter_changes = !equation.delimiter_changes.is_empty();
                for delimiters in equation.delimiter_changes {
                    inline_delimiters = match delimiters {
                        DelimiterChange::Disable => None,
                        DelimiterChange::Enable(delimiters) => {
                            remembered_inline_delimiters = Some(delimiters);
                            Some(delimiters)
                        }
                        DelimiterChange::EnablePrevious => remembered_inline_delimiters,
                    };
                }
                if expression.is_empty()
                    && !has_delimiter_changes
                    && !equation.recursive_definition
                    && equation.empty_request.as_deref().is_none_or(str::is_empty)
                {
                    index = end + 1;
                    continue;
                }
                if builder.node_count() >= limits.max_nodes {
                    record_limit(
                        &mut outcome,
                        LimitFinding {
                            code: crate::DiagnosticCode::LIMIT_NODES,
                            message: "tbl or eqn preprocessing exceeds max_nodes and retained the raw event",
                            location: builder.node_location(node),
                        },
                        None,
                    );
                    output.extend_from_slice(&flat[index..=end]);
                    index = end + 1;
                    continue;
                }
                if let Some(equation) = builder.push(root, NodeKind::Equation) {
                    let _ = builder.set_node_location(equation, builder.node_location(node));
                    let _ = builder
                        .set_node_flags(equation, builder.node_flags(node).unwrap_or_default());
                    if !expression.is_empty() {
                        let _ = builder.set_node_equation(equation, expression);
                        let _ = builder.set_node_equation_terminal(equation, terminal);
                    }
                    output.push(equation);
                }
                index = end + 1;
            }
            _ => {
                if let Some(delimiters) = inline_delimiters {
                    output.extend(split_inline_node(
                        builder,
                        root,
                        node,
                        delimiters,
                        limits,
                        &mut outcome,
                    ));
                } else {
                    output.push(node);
                }
                index += 1;
            }
        }
    }
    let _ = builder.replace_children(root, &output);
    outcome
}

fn record_limit(
    outcome: &mut PreprocessOutcome,
    mut limit: LimitFinding,
    fallback_location: Option<crate::SourceSpan>,
) {
    if outcome.limit.is_some() {
        return;
    }
    if limit.location.is_none() {
        limit.location = fallback_location;
    }
    outcome.limit = Some(limit);
}

enum InlineFragment {
    Text(String),
    Equation(String),
}

fn split_inline_node(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    node: NodeId,
    delimiters: (char, char),
    limits: &Limits,
    outcome: &mut PreprocessOutcome,
) -> Vec<NodeId> {
    match builder.node_kind(node) {
        Some(NodeKind::Text) => {
            split_inline_text_node(builder, parent, node, delimiters, limits, outcome)
        }
        Some(NodeKind::Element) => {
            let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) else {
                return vec![node];
            };
            let mut rewritten = Vec::with_capacity(children.len());
            let mut changed = false;
            for child in children {
                let pieces =
                    split_inline_text_node(builder, node, child, delimiters, limits, outcome);
                changed |= pieces.len() != 1 || pieces[0] != child;
                rewritten.extend(pieces);
            }
            if changed {
                let _ = builder.replace_children(node, &rewritten);
            }
            vec![node]
        }
        _ => vec![node],
    }
}

fn split_inline_text_node(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    node: NodeId,
    delimiters: (char, char),
    limits: &Limits,
    outcome: &mut PreprocessOutcome,
) -> Vec<NodeId> {
    let Some(text) = builder.node_text(node) else {
        return vec![node];
    };
    let fragments = match split_inline_fragments(text, delimiters, limits) {
        Ok(fragments) => fragments,
        Err(limit) => {
            record_limit(outcome, limit, builder.node_location(node));
            return vec![node];
        }
    };
    if matches!(fragments.as_slice(), [InlineFragment::Text(_)]) {
        return vec![node];
    }
    if builder.node_count().saturating_add(fragments.len()) > limits.max_nodes {
        record_limit(
            outcome,
            LimitFinding {
                code: crate::DiagnosticCode::LIMIT_NODES,
                message: "inline eqn preprocessing exceeds max_nodes and retained the raw text event",
                location: builder.node_location(node),
            },
            None,
        );
        return vec![node];
    }
    let location = builder.node_location(node);
    let original_flags = builder.node_flags(node).unwrap_or_default();
    let mut output = Vec::with_capacity(fragments.len());
    let text_precedes_equation = fragments
        .windows(2)
        .map(|pair| matches!(pair, [InlineFragment::Text(_), InlineFragment::Equation(_)]))
        .collect::<Vec<_>>();
    let ends_with_equation = matches!(fragments.last(), Some(InlineFragment::Equation(_)));
    let last_fragment = fragments.len().saturating_sub(1);
    for (index, fragment) in fragments.into_iter().enumerate() {
        let kind = match fragment {
            InlineFragment::Text(_) => NodeKind::Text,
            InlineFragment::Equation(_) => NodeKind::Equation,
        };
        let Some(replacement) = builder.push(parent, kind) else {
            break;
        };
        let mut flags = original_flags;
        if index > 0 {
            flags.line_start = false;
        }
        let fragment_location = if index == 0 {
            location.clone()
        } else {
            location.as_ref().and_then(|location| {
                crate::SourceSpan::new(
                    location.source,
                    location.start.saturating_add(1),
                    location.end.saturating_add(1),
                )
                .ok()
            })
        };
        let _ = builder.set_node_location(replacement, fragment_location);
        let _ = builder.set_node_flags(replacement, flags);
        match fragment {
            InlineFragment::Text(mut value) => {
                // mandoc's AST inserts a zero-width escape between prose and
                // an inline equation box. It is public owned-tree text, not
                // a renderer instruction, and prevents the preceding word
                // from absorbing the equation during later man/mdoc passes.
                if text_precedes_equation.get(index).copied().unwrap_or(false) {
                    value.push_str("\\&");
                }
                let _ = builder.text(replacement, value);
            }
            InlineFragment::Equation(value) => {
                let _ = builder.set_node_equation(replacement, value);
            }
        }
        output.push(replacement);
        if ends_with_equation && index == last_fragment {
            // A terminal inline formula has a second public zero-width text
            // boundary in the legacy owned AST.  It is distinct from the
            // preceding boundary and remains at the source line's base span.
            let Some(boundary) = builder.push(parent, NodeKind::Text) else {
                break;
            };
            let mut boundary_flags = original_flags;
            boundary_flags.line_start = false;
            let _ = builder.set_node_location(boundary, location.clone());
            let _ = builder.set_node_flags(boundary, boundary_flags);
            let _ = builder.text(boundary, "\\&");
            output.push(boundary);
        }
    }
    if output.is_empty() {
        vec![node]
    } else {
        output
    }
}

fn split_inline_fragments(
    value: &str,
    delimiters: (char, char),
    limits: &Limits,
) -> Result<Vec<InlineFragment>, LimitFinding> {
    let (opening, closing) = delimiters;
    let mut output = Vec::new();
    let mut remaining = value;
    while let Some(open_index) = remaining.find(opening) {
        let after_open = open_index + opening.len_utf8();
        let Some(close_relative) = remaining[after_open..].find(closing) else {
            break;
        };
        let close_index = after_open + close_relative;
        if open_index > 0 {
            output.push(InlineFragment::Text(remaining[..open_index].to_owned()));
        }
        let expression = normalize_inline_equation(&remaining[after_open..close_index], limits)?;
        if expression.is_empty() {
            output.push(InlineFragment::Text(
                remaining[open_index..close_index + closing.len_utf8()].to_owned(),
            ));
        } else {
            output.push(InlineFragment::Equation(expression));
        }
        remaining = &remaining[close_index + closing.len_utf8()..];
    }
    if output.is_empty() {
        return Ok(vec![InlineFragment::Text(value.to_owned())]);
    }
    if !remaining.is_empty() {
        output.push(InlineFragment::Text(remaining.to_owned()));
    }
    Ok(output)
}

fn normalize_inline_equation(value: &str, limits: &Limits) -> Result<String, LimitFinding> {
    let tokens = equation_tokens(value);
    if tokens.len() > limits.max_equation_tokens {
        return Err(equation_limit(
            crate::DiagnosticCode::LIMIT_EQUATION_TOKENS,
            "inline eqn preprocessing exceeds max_equation_tokens and retained the raw text event",
        ));
    }
    if equation_depth(&tokens) > limits.max_equation_depth {
        return Err(equation_limit(
            crate::DiagnosticCode::LIMIT_EQUATION_DEPTH,
            "inline eqn preprocessing exceeds max_equation_depth and retained the raw text event",
        ));
    }
    let definitions = std::collections::BTreeMap::new();
    let mut expansion_steps = 0;
    let mut recursive_definition = false;
    let tokens = expand_definitions(
        &tokens,
        &definitions,
        limits,
        &mut expansion_steps,
        &mut recursive_definition,
    )
    .map_err(|failure| failure.limit)?;
    Ok(normalize_equation_tokens(&tokens))
}

fn find_closer(
    builder: &DocumentBuilder,
    nodes: &[NodeId],
    start: usize,
    closer: &str,
) -> Option<usize> {
    nodes[start..]
        .iter()
        .position(|node| builder.node_macro_name(*node) == Some(closer))
        .map(|offset| start + offset)
}

#[derive(Clone)]
struct SourceLine {
    source: NodeId,
    text: String,
    macro_name: Option<Box<str>>,
    /// A bare roff control line is stored at the byte after its introducer.
    /// tbl nevertheless treats it as a literal layout terminator.
    layout_control_prefix: bool,
    has_invalid_input_bytes: bool,
    has_valid_utf8_non_ascii: bool,
    table_input_text: Option<Box<str>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CellFormat {
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
struct ParsedLayoutRow {
    cells: Vec<CellFormat>,
    terminal_cells: Vec<TableTerminalCell>,
    invalid_fonts: Vec<TableFontIssue>,
    excessive_spacings: Vec<TableSpacingIssue>,
    /// Byte offsets of vertical bars beyond tbl's two-per-cell limit.
    vertical_bar_offsets: Vec<usize>,
    /// Byte offsets of actual `^` cell descriptors, rather than `^` bytes
    /// that occurred while scanning another part of the layout language.
    leading_down_offsets: Vec<usize>,
    first_cell_offset: Option<usize>,
    leading_vertical_bars: usize,
}

struct ParsedLayoutLine {
    rows: Vec<ParsedLayoutRow>,
    /// A layout such as `|.` has no actual cell descriptor, but tbl still
    /// carries its initial rule into the one-column recovery row.  This is
    /// presentation-only metadata for the terminal renderer; the public AST
    /// keeps the normal recovered single cell.
    leading_vertical_bars: usize,
    complete: bool,
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

struct ParsedRow {
    source: NodeId,
    cells: Vec<TableCell>,
    terminal: TableTerminalRow,
}

struct ParsedTable {
    rows: Vec<ParsedRow>,
    limit: Option<LimitFinding>,
    recoveries: Vec<PreprocessRecovery>,
    dynamic_recoveries: Vec<DynamicPreprocessRecovery>,
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
fn parse_table_rows(
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

fn source_line(builder: &DocumentBuilder, node: NodeId) -> Option<SourceLine> {
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
fn parse_format_row(value: &str) -> Vec<CellFormat> {
    parse_layout_line(value)
        .rows
        .into_iter()
        .next()
        .map_or_else(Vec::new, |row| row.cells)
}

fn parse_layout_line(value: &str) -> ParsedLayoutLine {
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
fn table_field_text(value: &str) -> String {
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
fn table_cell_text(
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

struct ParsedEquation {
    expression: String,
    terminal: EquationTerminal,
    limit: Option<LimitFinding>,
    delimiter_changes: Vec<DelimiterChange>,
    recursive_definition: bool,
    empty_request: Option<Box<str>>,
    missing_boxes: Vec<&'static str>,
}

#[derive(Clone, Copy)]
enum DelimiterChange {
    Disable,
    Enable((char, char)),
    EnablePrevious,
}

#[allow(clippy::too_many_lines)] // Keeps definition, delimiter, and prefix-budget state in source order.
fn parse_equation(builder: &DocumentBuilder, nodes: &[NodeId], limits: &Limits) -> ParsedEquation {
    let mut definitions = std::collections::BTreeMap::<String, Vec<EquationToken>>::new();
    let mut tokens = Vec::new();
    let mut source_token_count = 0_usize;
    let mut expansion_steps = 0_usize;
    let mut limit = None;
    let mut delimiter_changes = Vec::new();
    let mut recursive_definition = false;
    let mut empty_request = None;
    for line in nodes.iter().filter_map(|node| source_line(builder, *node)) {
        let mut line = line.text.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((change, remainder)) = parse_delimiter_directive(line) {
            delimiter_changes.push(change);
            line = remainder;
            if line.is_empty() {
                continue;
            }
        }
        if line.strip_prefix("tdefine").is_some() {
            empty_request.get_or_insert_with(|| trailing_empty_request(line).into());
            continue;
        }
        if let Some(name) = parse_undef(line) {
            definitions.remove(name);
            continue;
        }
        if let Some((name, replacement, remainder)) = parse_definition(line) {
            empty_request.get_or_insert_with(|| trailing_empty_request(line).into());
            if !definitions.contains_key(&name)
                && definitions.len() >= limits.max_equation_definitions
            {
                limit = Some(equation_limit(
                    crate::DiagnosticCode::LIMIT_EQUATION_DEFINITIONS,
                    "eqn preprocessing exceeds max_equation_definitions and retained a finite expression prefix",
                ));
                break;
            }
            let replacement = equation_tokens(&replacement);
            recursive_definition |= replacement
                .iter()
                .any(|token| !token.quoted && token.text == name);
            definitions.insert(name, replacement);
            line = remainder;
            if let Some((before, name, after)) = split_inline_undef(line) {
                let raw = equation_tokens(before);
                let expanded = expand_definitions(
                    &raw,
                    &definitions,
                    limits,
                    &mut expansion_steps,
                    &mut recursive_definition,
                )
                .unwrap_or_else(|failure| {
                    limit.get_or_insert(failure.limit);
                    failure.prefix
                });
                tokens.extend(expanded);
                definitions.remove(name);
                line = after;
            }
            if line.is_empty() {
                continue;
            }
        }
        let raw_tokens =
            consume_inline_definition_requests(&equation_tokens(line), &mut definitions);
        let remaining = limits
            .max_equation_tokens
            .saturating_sub(source_token_count);
        let accepted = raw_tokens.len().min(remaining);
        source_token_count = source_token_count.saturating_add(accepted);
        let expanded = match expand_definitions(
            &raw_tokens[..accepted],
            &definitions,
            limits,
            &mut expansion_steps,
            &mut recursive_definition,
        ) {
            Ok(expanded) => expanded,
            Err(failure) => {
                if limit.is_none() {
                    limit = Some(failure.limit);
                }
                failure.prefix
            }
        };
        let remaining = limits.max_equation_tokens.saturating_sub(tokens.len());
        if expanded.len() > remaining {
            tokens.extend(expanded.into_iter().take(remaining));
            if limit.is_none() {
                limit = Some(equation_limit(
                    crate::DiagnosticCode::LIMIT_EQUATION_TOKENS,
                    "eqn definition expansion exceeds max_equation_tokens and retained a finite expression prefix",
                ));
            }
            break;
        }
        tokens.extend(expanded);
        if accepted < raw_tokens.len() {
            limit = Some(equation_limit(
                crate::DiagnosticCode::LIMIT_EQUATION_TOKENS,
                "eqn preprocessing exceeds max_equation_tokens and retained a finite expression prefix",
            ));
            break;
        }
        if limit.is_some() {
            break;
        }
    }
    let (bounded_tokens, depth_truncated) =
        truncate_equation_tokens(&tokens, limits.max_equation_depth);
    if depth_truncated {
        if limit.is_none() {
            limit = Some(display_equation_depth_limit(limits));
        }
        tokens = bounded_tokens;
    }
    // mandoc aborts the complete display once recursive substitution is
    // observed: it does not retain tokens before or after the recursive
    // reference as a partial equation.  Preserve the null `eqn` AST value
    // while still reporting the recoverable input-stack diagnostic.
    let (expression, missing_boxes) = if recursive_definition {
        (String::new(), Vec::new())
    } else {
        normalize_equation_tokens_with_missing_boxes(&tokens)
    };
    ParsedEquation {
        expression,
        terminal: EquationTerminal {
            tokens: tokens
                .iter()
                .map(|token| EquationTerminalToken {
                    text: token.text.clone().into_boxed_str(),
                    quoted: token.quoted,
                })
                .collect(),
        },
        limit,
        delimiter_changes,
        recursive_definition,
        empty_request,
        missing_boxes,
    }
}

fn parse_delimiter_directive(value: &str) -> Option<(DelimiterChange, &str)> {
    let value = value.strip_prefix("delim")?.trim_start();
    if let Some(remainder) = value.strip_prefix("off") {
        return Some((DelimiterChange::Disable, remainder.trim_start()));
    }
    if let Some(remainder) = value.strip_prefix("on") {
        return Some((DelimiterChange::EnablePrevious, remainder.trim_start()));
    }
    let mut characters = value.char_indices();
    let (_, opening) = characters.next()?;
    let (closing_index, closing) = characters.next()?;
    let remainder = &value[closing_index + closing.len_utf8()..];
    Some((
        DelimiterChange::Enable((opening, closing)),
        remainder.trim_start(),
    ))
}

fn parse_definition(line: &str) -> Option<(String, String, &str)> {
    let remainder = line
        .strip_prefix("define")
        .or_else(|| line.strip_prefix("ndefine"))?
        .trim_start();
    let mut parts = remainder.splitn(2, char::is_whitespace);
    let name = parts.next()?.to_owned();
    let replacement = parts.next().unwrap_or_default().trim();
    if name.is_empty() {
        return None;
    }
    let mut characters = replacement.char_indices();
    let Some((_, delimiter)) = characters.next() else {
        return Some((name, String::new(), ""));
    };
    if matches!(delimiter, '\'' | '"' | '/' | '|' | ':' | '!')
        && let Some((closing, _)) = characters.find(|(_, character)| *character == delimiter)
    {
        return Some((
            name,
            replacement[delimiter.len_utf8()..closing].to_owned(),
            replacement[closing + delimiter.len_utf8()..].trim_start(),
        ));
    }
    Some((name, replacement.trim_matches(['\'', '"']).to_owned(), ""))
}

fn parse_undef(line: &str) -> Option<&str> {
    let remainder = line.strip_prefix("undef")?.trim_start();
    remainder.split_whitespace().next()
}

fn trailing_empty_request(line: &str) -> String {
    let words = line.split_whitespace().collect::<Vec<_>>();
    let Some(index) = words
        .iter()
        .rposition(|word| matches!(*word, "define" | "undef" | "tdefine"))
    else {
        return String::new();
    };
    let request = words[index];
    match request {
        "define" | "undef" if index + 1 == words.len() => request.to_owned(),
        "define" if index + 2 == words.len() => format!("{request} {}", words[index + 1]),
        "tdefine" if index + 1 >= words.len().saturating_sub(1) => request.to_owned(),
        _ => String::new(),
    }
}

fn consume_inline_definition_requests(
    tokens: &[EquationToken],
    definitions: &mut std::collections::BTreeMap<String, Vec<EquationToken>>,
) -> Vec<EquationToken> {
    let mut output = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if !token.quoted && token.text == "undef" {
            if let Some(name) = tokens.get(index + 1).filter(|name| !name.quoted) {
                definitions.remove(&name.text);
                index += 2;
                continue;
            }
            break;
        }
        if !token.quoted && matches!(token.text.as_str(), "define" | "tdefine") {
            break;
        }
        output.push(token.clone());
        index += 1;
    }
    output
}

fn split_inline_undef(line: &str) -> Option<(&str, &str, &str)> {
    let words = line
        .split_whitespace()
        .map(|word| (word.as_ptr() as usize - line.as_ptr() as usize, word));
    for (offset, word) in words {
        if word != "undef" {
            continue;
        }
        let after_request = &line[offset + word.len()..];
        let name = after_request.split_whitespace().next()?;
        let name_offset = name.as_ptr() as usize - line.as_ptr() as usize;
        return Some((&line[..offset], name, &line[name_offset + name.len()..]));
    }
    None
}

#[derive(Clone, Debug)]
struct EquationToken {
    text: String,
    quoted: bool,
}

impl EquationToken {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            quoted: false,
        }
    }
}

fn equation_depth(tokens: &[EquationToken]) -> usize {
    let mut depth = 0_usize;
    let mut maximum = 0_usize;
    for token in tokens {
        match token.text.as_str() {
            "{" => {
                depth = depth.saturating_add(1);
                maximum = maximum.max(depth);
            }
            "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    maximum
}

/// Drop the innermost brace-contained equation content beyond a configured
/// depth while retaining a balanced token sequence that the normalizer can
/// process without recursive stack growth.
fn truncate_equation_tokens(
    tokens: &[EquationToken],
    maximum_depth: usize,
) -> (Vec<EquationToken>, bool) {
    let mut output = Vec::with_capacity(tokens.len().min(maximum_depth.saturating_mul(2)));
    let mut depth = 0_usize;
    let mut discarded_depth = None;
    let mut truncated = false;

    for token in tokens {
        if let Some(start) = discarded_depth {
            match token.text.as_str() {
                "{" => depth = depth.saturating_add(1),
                "}" => {
                    depth = depth.saturating_sub(1);
                    if depth < start {
                        discarded_depth = None;
                    }
                }
                _ => {}
            }
            continue;
        }

        match token.text.as_str() {
            "{" if depth >= maximum_depth => {
                depth = depth.saturating_add(1);
                discarded_depth = Some(depth);
                truncated = true;
            }
            "{" => {
                depth = depth.saturating_add(1);
                output.push(token.clone());
            }
            "}" => {
                depth = depth.saturating_sub(1);
                output.push(token.clone());
            }
            _ => output.push(token.clone()),
        }
    }

    (output, truncated)
}

fn display_equation_depth_limit(limits: &Limits) -> LimitFinding {
    if limits.max_equation_depth == 256 {
        equation_limit(
            crate::DiagnosticCode::LEGACY_EQUATION_TREE_DEPTH_LIMIT,
            LEGACY_EQUATION_TREE_DEPTH_MESSAGE,
        )
    } else {
        equation_limit(
            crate::DiagnosticCode::LIMIT_EQUATION_DEPTH,
            "eqn preprocessing exceeds max_equation_depth and retained a finite expression prefix",
        )
    }
}

enum EquationWork {
    Token(EquationToken),
    CloseDefinition(String),
}

struct ExpansionFailure {
    limit: LimitFinding,
    prefix: Vec<EquationToken>,
}

fn expand_definitions(
    tokens: &[EquationToken],
    definitions: &std::collections::BTreeMap<String, Vec<EquationToken>>,
    limits: &Limits,
    expansion_steps: &mut usize,
    recursive_definition: &mut bool,
) -> Result<Vec<EquationToken>, ExpansionFailure> {
    let mut work = tokens
        .iter()
        .rev()
        .cloned()
        .map(EquationWork::Token)
        .collect::<Vec<_>>();
    let mut active = std::collections::BTreeSet::new();
    let mut output = Vec::new();
    while let Some(item) = work.pop() {
        if *expansion_steps >= limits.max_equation_expansion_steps {
            return Err(ExpansionFailure {
                limit: equation_limit(
                    crate::DiagnosticCode::LIMIT_EQUATION_EXPANSION_STEPS,
                    "eqn preprocessing exceeds max_equation_expansion_steps and retained a finite expression prefix",
                ),
                prefix: output,
            });
        }
        *expansion_steps = expansion_steps.saturating_add(1);
        match item {
            EquationWork::CloseDefinition(name) => {
                active.remove(&name);
            }
            EquationWork::Token(token) => {
                if !token.quoted
                    && let Some(replacement) = definitions.get(&token.text)
                {
                    if active.insert(token.text.clone()) {
                        work.push(EquationWork::CloseDefinition(token.text));
                        work.extend(replacement.iter().rev().cloned().map(EquationWork::Token));
                        continue;
                    }
                    *recursive_definition = true;
                    continue;
                }
                if output.len() >= limits.max_equation_tokens {
                    return Err(ExpansionFailure {
                        limit: equation_limit(
                            crate::DiagnosticCode::LIMIT_EQUATION_TOKENS,
                            "eqn definition expansion exceeds max_equation_tokens and retained a finite expression prefix",
                        ),
                        prefix: output,
                    });
                }
                output.push(token);
            }
        }
    }
    Ok(output)
}

fn equation_limit(code: &'static str, message: &'static str) -> LimitFinding {
    LimitFinding {
        code,
        message,
        location: None,
    }
}

fn equation_tokens(value: &str) -> Vec<EquationToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = None;
    for character in value.chars() {
        if let Some(quote) = quoted {
            if character == quote {
                if !current.is_empty() {
                    tokens.push(EquationToken {
                        text: std::mem::take(&mut current),
                        quoted: true,
                    });
                }
                quoted = None;
            } else {
                current.push(character);
            }
            continue;
        }
        match character {
            '\'' | '"' => {
                if !current.is_empty() {
                    tokens.push(EquationToken::plain(std::mem::take(&mut current)));
                }
                quoted = Some(character);
            }
            // eqn(7) treats braces as grammar tokens and ignores `^`/`~` as
            // whitespace. Other punctuation remains in its source text box;
            // the legacy owned-AST projection later renders its unquoted
            // visible infix pieces separately.
            '{' | '}' => {
                if !current.is_empty() {
                    tokens.push(EquationToken::plain(std::mem::take(&mut current)));
                }
                tokens.push(EquationToken::plain(character.to_string()));
            }
            '^' | '~' => {
                if !current.is_empty() {
                    tokens.push(EquationToken::plain(std::mem::take(&mut current)));
                }
            }
            character if character.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(EquationToken::plain(std::mem::take(&mut current)));
                }
            }
            _ => current.push(character),
        }
    }
    if !current.is_empty() {
        tokens.push(EquationToken {
            text: current,
            quoted: quoted.is_some(),
        });
    }
    tokens
}

fn normalize_equation_tokens(tokens: &[EquationToken]) -> String {
    EquationNormalizer::new(tokens).normalize()
}

fn normalize_equation_tokens_with_missing_boxes(
    tokens: &[EquationToken],
) -> (String, Vec<&'static str>) {
    EquationNormalizer::new(tokens).normalize_with_missing_boxes()
}

/// Flatten the subset of the eqn box tree that the legacy C shim exposes in
/// `Node::equation`.  This is intentionally not a renderer: font and accent
/// boxes are retained by mandoc's renderers but omitted by its owned AST
/// projection, whereas positions and delimiters remain observable text.
struct EquationNormalizer<'a> {
    tokens: &'a [EquationToken],
    index: usize,
    missing_boxes: Vec<&'static str>,
}

impl<'a> EquationNormalizer<'a> {
    fn new(tokens: &'a [EquationToken]) -> Self {
        Self {
            tokens,
            index: 0,
            missing_boxes: Vec::new(),
        }
    }

    fn normalize(&mut self) -> String {
        self.sequence(None)
    }

    fn normalize_with_missing_boxes(mut self) -> (String, Vec<&'static str>) {
        let expression = self.sequence(None);
        (expression, self.missing_boxes)
    }

    fn sequence(&mut self, terminator: Option<Terminator>) -> String {
        let mut values = Vec::new();
        while let Some(token) = self.peek().cloned() {
            if !token.quoted && terminator.is_some_and(|expected| expected.matches(&token.text)) {
                self.index += 1;
                break;
            }
            if token.quoted {
                values.push(self.atom());
                continue;
            }
            match token.text.as_str() {
                "}" | "right" => {
                    // An unmatched close is diagnosed by the full grammar;
                    // the compatibility projection simply does not render it.
                    self.index += 1;
                    if token.text == "right" {
                        self.index = self
                            .index
                            .saturating_add(usize::from(self.peek().is_some()));
                    }
                }
                "{" => {
                    self.index += 1;
                    let value = self.sequence(Some(Terminator::Brace));
                    if !value.is_empty() {
                        values.push(value);
                    }
                }
                "left" => {
                    self.index += 1;
                    let opening = self.next_text().unwrap_or_default();
                    let content = self.sequence(Some(Terminator::Right));
                    let closing = self.take_closing_delimiter();
                    values.push(format!(
                        "{}{}{}",
                        normalize_left_equation_delimiter(&opening),
                        content,
                        normalize_right_equation_delimiter(&closing)
                    ));
                }
                "sqrt" => {
                    self.index += 1;
                    let value = self.atom();
                    values.push(format!("sqrt({value})"));
                }
                "sub" | "from" => {
                    let paired = if token.text == "sub" { "sup" } else { "to" };
                    self.index += 1;
                    let lower = self.position_atom();
                    let left = values.pop().unwrap_or_default();
                    if self
                        .peek()
                        .is_some_and(|token| !token.quoted && token.text == paired)
                    {
                        self.index += 1;
                        let upper = self.position_atom();
                        values.push(format!("{left} _ {lower} ^ {upper}"));
                    } else {
                        values.push(format!("{left} _ {lower}"));
                    }
                }
                "sup" | "to" | "over" => {
                    let operator = match token.text.as_str() {
                        "sup" | "to" => "^",
                        "over" => "/",
                        _ => unreachable!(),
                    };
                    self.index += 1;
                    let right = if token.text == "over" {
                        self.atom()
                    } else {
                        self.position_atom()
                    };
                    let left = values.pop();
                    if left.is_none() && token.text == "over" {
                        self.missing_boxes.push("over");
                    }
                    let left = left.unwrap_or_default();
                    values.push(format!("{left} {operator} {right}"));
                }
                // These grammar tokens affect layout, font, or decoration
                // only. `above` is a pile separator rather than a fraction;
                // `copy_equation()` intentionally omits accent boxes.
                "above" | "mark" | "lineup" | "dyad" | "vec" | "under" | "bar" | "tilde"
                | "hat" | "dot" | "dotdot" | "roman" | "bold" | "italic" | "fat" | "pile"
                | "lpile" | "rpile" | "cpile" | "ccol" | "lcol" | "rcol" | "matrix" | "define"
                | "ndefine" | "tdefine" | "undef" | "delim" => self.index += 1,
                // Size, global size, global font, and horizontal/vertical
                // movements consume one argument but do not affect the AST's
                // renderer-neutral text.
                "size" | "gsize" | "gfont" | "fwd" | "back" | "down" | "up" => {
                    self.index += 1;
                    self.index = self
                        .index
                        .saturating_add(usize::from(self.peek().is_some()));
                }
                _ => values.push(self.atom()),
            }
        }
        join_equation_terms(&values)
    }

    fn atom(&mut self) -> String {
        let Some(token) = self.peek().cloned() else {
            return String::new();
        };
        if token.quoted {
            self.index += 1;
            return token.text;
        }
        match token.text.as_str() {
            "{" => {
                self.index += 1;
                self.sequence(Some(Terminator::Brace))
            }
            "left" => {
                self.index += 1;
                let opening = self.next_text().unwrap_or_default();
                let content = self.sequence(Some(Terminator::Right));
                let closing = self.take_closing_delimiter();
                format!(
                    "{}{}{}",
                    normalize_left_equation_delimiter(&opening),
                    content,
                    normalize_right_equation_delimiter(&closing)
                )
            }
            "sqrt" => {
                self.index += 1;
                format!("sqrt({})", self.atom())
            }
            // A binary `over` where an operand is required constructs the
            // same empty fraction box that mandoc exposes through its owned
            // AST. The enclosing operator emits the single recovery finding
            // for its missing left box; this nested malformed box is visible
            // only through the stable ` / ` projection.
            "over" => {
                self.index += 1;
                " / ".to_owned()
            }
            // Font boxes are transparent in the legacy owned-AST equation
            // projection: preserve their governed atom but not the layout
            // instruction itself. This matters when a font prefix appears
            // directly as a fraction, subscript, or superscript operand.
            "roman" | "bold" | "italic" | "fat" => {
                self.index += 1;
                self.atom()
            }
            _ => {
                self.index += 1;
                split_compound_equation_text(&token.text)
                    .unwrap_or_else(|| normalize_equation_symbol(&token.text).to_owned())
            }
        }
    }

    /// Position operators require an operand, but a second grammar keyword
    /// occupies that slot while contributing no text (`x sub 1 sup sup`).
    /// Consume that malformed keyword so it cannot re-enter the outer stream
    /// as literal equation content.
    fn position_atom(&mut self) -> String {
        if self.peek().is_some_and(|token| {
            !token.quoted && matches!(token.text.as_str(), "sub" | "from" | "sup" | "to" | "over")
        }) {
            self.index += 1;
            return String::new();
        }
        self.atom()
    }

    fn take_closing_delimiter(&mut self) -> String {
        self.next_text().unwrap_or_default()
    }

    fn next_text(&mut self) -> Option<String> {
        let token = self.peek()?.text.clone();
        self.index += 1;
        Some(token)
    }

    fn peek(&self) -> Option<&EquationToken> {
        self.tokens.get(self.index)
    }
}

#[derive(Clone, Copy)]
enum Terminator {
    Brace,
    Right,
}

impl Terminator {
    fn matches(self, token: &str) -> bool {
        matches!((self, token), (Self::Brace, "}") | (Self::Right, "right"))
    }
}

fn normalize_left_equation_delimiter(value: &str) -> &str {
    match value {
        "ceiling" => "\\[lc]",
        "floor" => "\\[lf]",
        other => other,
    }
}

fn normalize_right_equation_delimiter(value: &str) -> &str {
    match value {
        "ceiling" => "\\[rc]",
        "floor" => "\\[rf]",
        other => other,
    }
}

/// Match the shim's visible projection of an unquoted text box containing
/// simple infix punctuation. The eqn parser retains the source box, but the
/// legacy owned-AST walk publishes its visible operands with separators. Keep
/// multi-character relation operators intact and never apply this to quoted
/// text boxes.
fn split_compound_equation_text(value: &str) -> Option<String> {
    let characters = value.chars().collect::<Vec<_>>();
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut split = false;
    for (index, character) in characters.iter().copied().enumerate() {
        let previous = index.checked_sub(1).and_then(|index| characters.get(index));
        let next = characters.get(index + 1);
        let operator = matches!(character, '+' | '-' | '/')
            && previous
                .is_some_and(|character| !matches!(character, '+' | '-' | '/' | '<' | '>' | '='))
            && next
                .is_some_and(|character| !matches!(character, '+' | '-' | '/' | '<' | '>' | '='));
        if operator {
            if current.is_empty() {
                return None;
            }
            parts.push(std::mem::take(&mut current));
            parts.push(character.to_string());
            split = true;
        } else {
            current.push(character);
        }
    }
    if !split || current.is_empty() {
        return None;
    }
    parts.push(current);
    Some(parts.join(" "))
}

pub(crate) fn normalize_equation_symbol(value: &str) -> &str {
    match value {
        "ldots" => "...",
        "alpha" => "\\[*a]",
        "beta" => "\\[*b]",
        "chi" => "\\[*x]",
        "delta" => "\\[*d]",
        "epsilon" => "\\[*e]",
        "eta" => "\\[*y]",
        "gamma" => "\\[*g]",
        "iota" => "\\[*i]",
        "kappa" => "\\[*k]",
        "lambda" => "\\[*l]",
        "mu" => "\\[*m]",
        "nu" => "\\[*n]",
        "omega" => "\\[*w]",
        "omicron" => "\\[*o]",
        "phi" => "\\[*f]",
        "pi" => "\\[*p]",
        "psi" => "\\[*q]",
        "rho" => "\\[*r]",
        "sigma" => "\\[*s]",
        "tau" => "\\[*t]",
        "theta" => "\\[*h]",
        "upsilon" => "\\[*u]",
        "xi" => "\\[*c]",
        "zeta" => "\\[*z]",
        "DELTA" => "\\[*D]",
        "GAMMA" => "\\[*G]",
        "LAMBDA" => "\\[*L]",
        "OMEGA" => "\\[*W]",
        "PHI" => "\\[*F]",
        "PI" => "\\[*P]",
        "PSI" => "\\[*Q]",
        "SIGMA" => "\\[*S]",
        "THETA" => "\\[*H]",
        "UPSILON" => "\\[*U]",
        "XI" => "\\[*C]",
        "inter" => "\\[ca]",
        "union" => "\\[cu]",
        "prod" => "\\[product]",
        "int" => "\\[integral]",
        "sum" => "\\[sum]",
        "grad" | "del" => "\\[gr]",
        "times" => "\\[mu]",
        "cdot" => "\\[pc]",
        "nothing" => "\\[&]",
        "approx" => "\\[~~]",
        "prime" => "\\[fm]",
        "half" => "\\[12]",
        "partial" => "\\[pd]",
        "inf" => "\\[if]",
        ">>" => "\\[>>]",
        "<<" => "\\[<<]",
        "<-" => "\\[<-]",
        "->" => "\\[->]",
        "+-" => "\\[+-]",
        "!=" => "\\[!=]",
        "==" => "\\[==]",
        "<=" => "\\[<=]",
        ">=" => "\\[>=]",
        "-" => "\\[-]",
        other => other,
    }
}

fn join_equation_terms(tokens: &[String]) -> String {
    let mut output = String::new();
    for token in tokens {
        let close = matches!(token.as_str(), ")" | "]");
        if !output.is_empty() && !close && !output.ends_with(['(', '[']) {
            output.push(' ');
        }
        output.push_str(token);
    }
    output
}

#[cfg(test)]
mod tests {
    use crate::{
        DiagnosticCode, NodeKind, Parser, ParserConfig, Severity, Source, SourceName,
        TableAlignment,
    };

    #[cfg(feature = "render")]
    use super::TableTerminalBorder;
    use super::{
        CellFormat, parse_layout_line, table_cell_text, table_field_text, trailing_empty_request,
    };

    fn parse(bytes: &[u8]) -> crate::ParseReport {
        let name = SourceName::new("preprocess.1").unwrap();
        Parser::default().parse(Source::new(&name, bytes)).unwrap()
    }

    #[test]
    fn table_cells_project_unicode_controls_as_question_marks() {
        assert_eq!(
            table_cell_text("before\0\u{001f}\u{0080}after", false, false).as_ref(),
            "before???after"
        );
        assert_eq!(
            table_cell_text("normal text", false, false).as_ref(),
            "normal text"
        );
        assert_eq!(
            table_cell_text("raw \u{00bf}", true, false).as_ref(),
            "raw ?"
        );
        assert_eq!(
            table_cell_text("UTF-8 \u{00bf}", false, true).as_ref(),
            "UTF-8 \\[u00BF]"
        );
        assert_eq!(
            table_cell_text("UTF-8 \u{0080}", false, true).as_ref(),
            "UTF-8 \\[u0080]"
        );
    }

    #[test]
    fn retains_one_masked_space_after_an_ascii_control_unicode_escape() {
        assert_eq!(table_field_text(r"\[u0000] "), r"\[u0000] ");
        assert_eq!(table_field_text(r"\[u00A0] "), r"\[u00A0]");
        assert_eq!(table_field_text("ordinary "), "ordinary");
    }

    #[test]
    fn converts_basic_tbl_rows_and_display_eqn_before_man_lowering() {
        let report = parse(
            b".TH PREPROCESS 1\n.SH DESCRIPTION\n.TS\ntab(:);\nl r.\nleft:right\n.TE\n.EQ\ndelim $$\nx sup 2\n.EN\n",
        );
        let nodes = report.document.preorder().collect::<Vec<_>>();
        let table = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::Table)
            .unwrap();
        assert_eq!(table.table_cells().len(), 2);
        assert_eq!(table.table_cells()[0].text.as_deref(), Some("left"));
        assert_eq!(table.table_cells()[1].text.as_deref(), Some("right"));
        assert_eq!(table.table_cells()[1].alignment, TableAlignment::Right);
        let equation = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::Equation)
            .unwrap();
        assert_eq!(equation.equation(), Some("x ^ 2"));
        assert!(
            nodes
                .iter()
                .all(|node| { !matches!(node.macro_name(), Some("TS" | "TE" | "EQ" | "EN")) })
        );
    }

    #[test]
    fn table_font_modifiers_do_not_form_extra_layout_columns() {
        assert_eq!(
            super::parse_format_row("cb|cfCW|ci"),
            vec![
                super::CellFormat::Center,
                super::CellFormat::Center,
                super::CellFormat::Center,
            ]
        );
        assert_eq!(
            super::parse_format_row("cFI | cf(foobar) | cFB"),
            vec![
                super::CellFormat::Center,
                super::CellFormat::Center,
                super::CellFormat::Center,
            ]
        );
    }

    #[test]
    fn tbl_width_modifiers_keep_their_terminal_cell_widths() {
        let parsed = parse_layout_line("lw2 lw(2n) lw(0.16i).");
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(
            parsed.rows[0]
                .terminal_cells
                .iter()
                .map(|cell| cell.minimum_width)
                .collect::<Vec<_>>(),
            vec![Some(2), Some(2), Some(2)]
        );
    }

    #[test]
    fn comma_separated_tbl_layout_rows_keep_independent_span_state() {
        let parsed = parse_layout_line("l|p-1l bsil|||l,l|l\t ilb^|i||l.");
        assert!(parsed.complete);
        assert_eq!(parsed.rows.len(), 2);
        assert_eq!(
            parsed.rows[0].cells,
            vec![
                CellFormat::Left,
                CellFormat::Left,
                CellFormat::Span,
                CellFormat::Left,
                CellFormat::Left,
            ]
        );
        assert_eq!(
            parsed.rows[1].cells,
            vec![
                CellFormat::Left,
                CellFormat::Left,
                CellFormat::Left,
                CellFormat::Down,
                CellFormat::Left,
            ]
        );
        assert_eq!(parsed.rows[0].vertical_bar_offsets, vec![13]);
        assert_eq!(parsed.rows[1].vertical_bar_offsets, vec![28]);
        assert_eq!(parsed.rows[1].leading_down_offsets, vec![24]);
    }

    #[test]
    fn invalid_tbl_fonts_do_not_terminate_the_layout() {
        let report = parse(
            b".TH TABLE 1\n.SH DESCRIPTION\n.TS\nlfI lfX\nlfB lfIB\nlfI lf.\nlfB lfI.\nitalic\tone char\nbold\ttwo chars\nitalic\tdot\nbold\titalic\n.TE\n",
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::TBL_UNKNOWN_FONT,
                    "unknown font, skipping request: TS fX",
                ),
                (
                    DiagnosticCode::TBL_UNKNOWN_FONT,
                    "unknown font, skipping request: TS fIB",
                ),
                (
                    DiagnosticCode::TBL_UNKNOWN_FONT,
                    "unknown font, skipping request: TS f.",
                ),
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let position = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (position.line, position.column)
            })
            .collect::<Vec<_>>();
        assert_eq!(positions, [(4, 7), (5, 7), (6, 7)]);
        let rows = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows.iter()
                .map(|row| {
                    row.table_cells()
                        .iter()
                        .map(|cell| cell.text.as_deref().unwrap())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>(),
            [
                vec!["italic", "one char"],
                vec!["bold", "two chars"],
                vec!["italic", "dot"],
                vec!["bold", "italic"],
            ]
        );
    }

    #[test]
    fn tbl_excessive_spacing_reports_the_first_spacing_digit() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\nR 10 L.\nleft\tright\n.TE\n");
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(
            diagnostic.code.as_str(),
            DiagnosticCode::TBL_EXCESSIVE_SPACING
        );
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(
            diagnostic.message.as_ref(),
            "ignoring excessive spacing in tbl layout: 10"
        );
        let position = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (4, 3));
    }

    #[test]
    fn tbl_layout_reports_a_leading_span_without_rewriting_cells() {
        let report = parse(
            b".TH TABLE 1\n.SH DESCRIPTION\n.TS\nS L S S\nL L L L L L.\nspan\tend\n1\t2\t3\t4\t5\t6\n.TE\n",
        );
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code.as_str(), DiagnosticCode::TBL_LEADING_SPAN);
        assert_eq!(diagnostic.message.as_ref(), "tbl line starts with span");
        let position = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (4, 1));
        let rows = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .map(|row| {
                    row.table_cells()
                        .iter()
                        .map(|cell| cell.text.as_deref())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>(),
            [
                vec![Some("span"), Some("end")],
                vec![
                    Some("1"),
                    Some("2"),
                    Some("3"),
                    Some("4"),
                    Some("5"),
                    Some("6")
                ],
            ]
        );
        assert_eq!(rows[0].table_cells()[0].text.as_deref(), Some("span"));
        assert_eq!(rows[0].table_cells()[0].column_span, 3);
    }

    #[test]
    fn tbl_text_block_macro_is_retained_as_text_and_reported() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\nl.\nT{\n.SM abcd\nT}\n.TE\n");
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code.as_str(), DiagnosticCode::TBL_MACRO);
        assert_eq!(diagnostic.severity, Severity::Unsupported);
        assert_eq!(
            diagnostic.message.as_ref(),
            "ignoring macro in table: SM abcd"
        );
        let position = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (6, 2));
        let table = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Table)
            .unwrap();
        assert_eq!(table.table_cells()[0].text.as_deref(), Some("abcd"));
    }

    #[test]
    fn nested_tbl_opener_is_reported_without_a_synthetic_empty_row() {
        let report = parse(
            b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ntab(:);\nl | l .\na:b\n_\nc:d\n.TS\ne:f\n.TE\n",
        );
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code.as_str(), DiagnosticCode::TBL_MACRO);
        assert_eq!(diagnostic.severity, Severity::Unsupported);
        assert_eq!(diagnostic.message.as_ref(), "ignoring macro in table: TS");
        let position = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (9, 4));
        assert_eq!(
            report
                .document
                .preorder()
                .filter(|node| node.kind() == NodeKind::Table)
                .count(),
            4
        );
    }

    #[test]
    fn tbl_option_recovery_preserves_following_layout_and_reports_each_fault() {
        let report = parse(
            b".TH TBL-OPTIONS 1\n.SH DESCRIPTION\n.TS\ntab decimalpoint (,x) %foo box;\nn n .\n10.0\t0.01\n.TE\n.TS\n , box,tab(:)\tdelim($$); l l .\na:b\n.TE\n",
        );
        let actual = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let position = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (
                    diagnostic.code.as_str(),
                    diagnostic.severity,
                    diagnostic.message.as_ref(),
                    (position.line, position.column),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            [
                (
                    DiagnosticCode::TBL_OPTION_ARGUMENT,
                    Severity::Error,
                    "missing tbl option argument: tab",
                    (4, 5),
                ),
                (
                    DiagnosticCode::TBL_OPTION_ARGUMENT_SIZE,
                    Severity::Error,
                    "wrong tbl option argument size: decimalpoint want 1 have 2",
                    (4, 19),
                ),
                (
                    DiagnosticCode::TBL_OPTION_CHARACTER,
                    Severity::Error,
                    "non-alphabetic character in tbl options: %",
                    (4, 23),
                ),
                (
                    DiagnosticCode::TBL_UNKNOWN_OPTION,
                    Severity::Error,
                    "skipping unknown tbl option: foo",
                    (4, 24),
                ),
                (
                    DiagnosticCode::TBL_EQN_DELIMITER_OPTION,
                    Severity::Unsupported,
                    "eqn delim option in tbl: $$",
                    (9, 21),
                ),
            ]
        );
        assert_eq!(
            report
                .document
                .preorder()
                .filter(|node| node.kind() == NodeKind::Table)
                .count(),
            2
        );
    }

    #[test]
    fn tbl_single_layout_discards_and_reports_extra_data_cells() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ntab(:);\nl l.\na:b:stray\n.TE\n");
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(
            diagnostic.code.as_str(),
            DiagnosticCode::TBL_EXTRA_DATA_CELLS
        );
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(
            diagnostic.message.as_ref(),
            "ignoring extra tbl data cells: stray"
        );
        let position = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (6, 5));
        let table = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Table)
            .unwrap();
        assert_eq!(
            table
                .table_cells()
                .iter()
                .map(|cell| cell.text.as_deref())
                .collect::<Vec<_>>(),
            [Some("a"), Some("b")]
        );
    }

    #[test]
    fn empty_tbl_reports_at_the_opener_and_retains_closing_spacing() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\nl.\n.TE\n");
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code.as_str(), DiagnosticCode::TBL_NO_DATA);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.message.as_ref(), "tbl without any data cells");
        let position = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (3, 2));
        let spacing = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("sp"))
            .unwrap();
        assert_eq!(spacing.kind(), NodeKind::Element);
        let position = report
            .document
            .source_position(spacing.location().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (5, 2));
    }

    #[test]
    fn empty_tbl_layouts_report_their_terminator() {
        let report = parse(
            b".TH TABLE 1\n.SH DESCRIPTION\n.TS\n.\nfirst text\n.TE\n.TS\n|.\nsecond text\n.TE\n",
        );
        assert_eq!(report.diagnostics.len(), 2);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code.as_str(),
                        diagnostic.severity,
                        diagnostic.message.as_ref(),
                    )
                })
                .collect::<Vec<_>>(),
            [
                (
                    DiagnosticCode::TBL_EMPTY_LAYOUT,
                    Severity::Error,
                    "empty tbl layout",
                ),
                (
                    DiagnosticCode::TBL_EMPTY_LAYOUT,
                    Severity::Error,
                    "empty tbl layout",
                ),
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let position = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (position.line, position.column)
            })
            .collect::<Vec<_>>();
        assert_eq!(positions, [(4, 2), (8, 3)]);
    }

    #[test]
    fn blank_tbl_data_line_is_an_empty_non_line_start_row() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\nl.\nfirst\n\nlast\n.TE\n");
        let rows = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].table_cells()[0].text.as_deref(), Some("first"));
        assert!(rows[1].table_cells().is_empty());
        assert_eq!(rows[2].table_cells()[0].text.as_deref(), Some("last"));
        assert!(rows.iter().all(|row| !row.flags().line_start));
    }

    #[test]
    fn table_data_continuations_join_before_escape_recovery() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\nl.\nfirst \\\nsecond\n.TE\n");
        assert!(report.diagnostics.is_empty());
        let row = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Table)
            .unwrap();
        assert_eq!(row.table_cells()[0].text.as_deref(), Some("first second"));
        let position = report
            .document
            .source_position(row.location().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (6, 1));
    }

    #[test]
    fn table_rules_remain_empty_rows_without_consuming_layout() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\nl\nr.\n_\nleft\nright\n.TE\n");
        let rows = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 3);
        assert!(rows[0].table_cells().is_empty());
        assert_eq!(rows[1].table_cells()[0].text.as_deref(), Some("left"));
        assert_eq!(rows[1].table_cells()[0].alignment, TableAlignment::Left);
        assert_eq!(rows[2].table_cells()[0].text.as_deref(), Some("right"));
        assert_eq!(rows[2].table_cells()[0].alignment, TableAlignment::Right);
    }

    #[cfg(feature = "render")]
    #[test]
    fn standalone_tbl_vertical_layout_line_carries_into_the_next_data_layout() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\nl\n|\nr.\nfirst\n_\nsecond\n.TE\n");
        let rows = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[2]
                .table_terminal()
                .expect("generated tbl row has device metadata")
                .cells[0]
                .before_vertical_rules,
            1
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn tbl_z_modifier_retains_private_width_ignored_state() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\nn, nz.\n12.34\n1000.0\n.TE\n");
        let rows = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        assert!(
            rows[1]
                .table_terminal()
                .expect("generated tbl row has device metadata")
                .cells[0]
                .width_ignored
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn tbl_x_modifier_retains_private_width_expanding_state() {
        let report =
            parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ntab(:);\nlx lx l.\nx:x:value\n.TE\n");
        let row = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Table)
            .expect("generated tbl row");
        let terminal = row
            .table_terminal()
            .expect("generated tbl row has device metadata");
        assert!(terminal.cells[0].width_expanding);
        assert!(terminal.cells[1].width_expanding);
        assert!(!terminal.cells[2].width_expanding);
    }

    #[cfg(feature = "render")]
    #[test]
    fn tbl_horizontal_layout_cells_keep_their_consumed_data_columns() {
        let report = parse(
            b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ntab(:);\n_ _\nl l\n- -\nl r\n_ ^\nr.\ncolum one:column two\nleft:right\nnot:printed\nright:left\n.TE\n",
        );
        let rows = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 6);
        let terminal = rows[4]
            .table_terminal()
            .expect("generated tbl row has device metadata");
        assert_eq!(terminal.data_columns, [0, 1]);
        assert_eq!(
            terminal.cells[0].horizontal_rule,
            TableTerminalBorder::Single
        );
    }

    #[test]
    fn numeric_table_columns_project_as_right_aligned() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\nn.\n42\n.TE\n");
        let table = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Table)
            .unwrap();
        assert_eq!(table.table_cells()[0].alignment, TableAlignment::Right);
    }

    #[test]
    fn horizontal_only_layout_rows_materialize_before_real_data() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\n_\nl.\nvalue\n.TE\n");
        let rows = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].table_cells().is_empty());
        assert_eq!(rows[1].table_cells()[0].text.as_deref(), Some("value"));
    }

    #[test]
    fn short_layout_omits_empty_trailing_raw_cells() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ntab(:);\nl l.\nleft:right:\n.TE\n");
        let table = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Table)
            .unwrap();
        assert_eq!(table.table_cells().len(), 2);
        assert_eq!(table.table_cells()[0].text.as_deref(), Some("left"));
        assert_eq!(table.table_cells()[1].text.as_deref(), Some("right"));
    }

    #[test]
    fn table_layout_retains_horizontal_and_vertical_spans() {
        let report = parse(
            b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ntab(:);\nl s r\n^ s r.\nfirst:right\nignored:continued\n.TE\n",
        );
        let rows = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].table_cells().len(), 2);
        assert_eq!(rows[0].table_cells()[0].column_span, 2);
        assert_eq!(rows[0].table_cells()[0].row_span, 2);
        assert_eq!(rows[0].table_cells()[1].alignment, TableAlignment::Right);
        assert!(rows[1].table_cells()[0].vertical_continuation);
        assert_eq!(rows[1].table_cells()[0].text.as_deref(), Some("ignored"));
    }

    #[test]
    fn table_text_blocks_stay_one_logical_cell() {
        let report = parse(
            b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ntab(:);\nl l.\nT{\nfirst line\nsecond line\nT}:short\n.TE\n",
        );
        let table = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Table)
            .unwrap();
        assert_eq!(table.table_cells().len(), 2);
        assert!(table.table_cells()[0].text_block);
        assert_eq!(
            table.table_cells()[0].text.as_deref(),
            Some("first line second line")
        );
        assert_eq!(table.table_cells()[1].text.as_deref(), Some("short"));
    }

    #[test]
    fn adjacent_table_text_blocks_on_one_row_remain_pending() {
        let report = parse(
            b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ntab(:);\nl l l.\nT{\nfirst\nT}:middle:T{\nlast\nT}\n.TE\n",
        );
        let tables = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .collect::<Vec<_>>();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].table_cells().len(), 3);
        assert!(tables[0].table_cells()[0].text_block);
        assert_eq!(tables[0].table_cells()[0].text.as_deref(), Some("first"));
        assert_eq!(tables[0].table_cells()[1].text.as_deref(), Some("middle"));
        assert!(tables[0].table_cells()[2].text_block);
        assert_eq!(tables[0].table_cells()[2].text.as_deref(), Some("last"));
    }

    #[test]
    fn empty_table_text_blocks_use_a_null_cell_payload() {
        let report = parse(b".TH TABLE 1\n.SH DESCRIPTION\n.TS\nl l.\ntable\tT{\nT}\n.TE\n");
        let table = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Table)
            .unwrap();
        assert_eq!(table.table_cells()[0].text.as_deref(), Some("table"));
        assert!(!table.table_cells()[1].text_block);
        assert_eq!(table.table_cells()[1].text, None);
    }

    #[test]
    fn table_layout_resets_keep_the_original_delimiter() {
        let report = parse(
            b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ntab(:);\nl l.\nleft:right\n.T&\nr r.\nsecond:third\n.TE\n",
        );
        let rows = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].table_cells()[1].alignment, TableAlignment::Left);
        assert_eq!(rows[1].table_cells()[0].alignment, TableAlignment::Right);
        assert_eq!(rows[1].table_cells()[1].text.as_deref(), Some("third"));
    }

    #[test]
    fn equation_normalization_retains_infix_and_visible_forms() {
        let report = parse(
            b".TH EQUATION 1\n.SH DESCRIPTION\n.EQ\ndefine dots 'ldots'\nx hat sub 1 sup 2 + sqrt { a over 2 } + dots\n.EN\n",
        );
        let equation = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Equation)
            .and_then(crate::NodeRef::equation)
            .unwrap();
        // Accent boxes influence rendered layout but the legacy owned-AST
        // equation projection intentionally omits them.
        assert_eq!(equation, "x _ 1 ^ 2 + sqrt(a / 2) + ...");
    }

    #[test]
    fn paired_position_operators_consume_missing_keyword_operands() {
        let report =
            parse(b".TH EQUATION 1\n.EQ\nx from a to to\n.EN\n.EQ\nx sub 1 sup sup\n.EN\n");
        let equations = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Equation)
            .filter_map(crate::NodeRef::equation)
            .collect::<Vec<_>>();
        assert_eq!(equations, ["x _ a ^ ", "x _ 1 ^ "]);
    }

    #[test]
    fn malformed_fraction_uses_empty_boxes_and_reports_the_display_opener() {
        let report = parse(b".TH EQUATION 1\n.EQ\nover over\n.EN\n");
        let equation = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Equation)
            .and_then(crate::NodeRef::equation)
            .unwrap();
        assert_eq!(equation, " /  / ");
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code.as_str(), DiagnosticCode::EQN_MISSING_BOX);
        assert_eq!(
            diagnostic.message.as_ref(),
            "missing eqn box, using \"\": over"
        );
        let location = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((location.line, location.column), (2, 2));
    }

    #[test]
    fn font_prefixes_are_transparent_inside_equation_operands() {
        let report = parse(
            b".TH EQUATION 1\n.EQ\nbold a over bold c ; roman I sub bold I sup italic I\n.EN\n",
        );
        let equation = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Equation)
            .and_then(crate::NodeRef::equation)
            .unwrap();
        assert_eq!(equation, "a / c ; I _ I ^ I");
    }

    #[test]
    fn equation_quoted_atoms_and_compound_text_boxes_keep_distinct_identity() {
        let tokens =
            super::equation_tokens("epsilon \"epsilon\" prime \"prime\" epsilon-prime sqrt a+b");
        assert!(tokens[1].quoted);
        assert!(tokens[3].quoted);
        assert_eq!(
            super::normalize_equation_tokens(&tokens),
            "\\[*e] epsilon \\[fm] prime epsilon - prime sqrt(a + b)"
        );
    }

    #[test]
    fn inline_delimiters_split_prose_and_macro_arguments_into_equation_nodes() {
        let report = parse(
            b".TH EQN 1\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\nfor $i sub 1$ values\n.BR Dp \"$x sup 2$\"\n",
        );
        let equations = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Equation)
            .filter_map(crate::NodeRef::equation)
            .collect::<Vec<_>>();
        assert_eq!(equations, ["i _ 1", "x ^ 2"]);
        let visible_text = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(visible_text.contains(&"for \\&"));
        assert!(visible_text.contains(&" values"));
    }

    #[test]
    fn inline_delimiter_changes_are_ordered_across_display_equations() {
        let report = parse(
            b".TH EQN 1\n.SH DESCRIPTION\n.EQ\ndelim []alpha\n.EN\ninline [beta]\n.EQ\ndelim offgamma\n.EN\ninline [delta]\n.EQ\ndelim onepsilon\n.EN\ninline [zeta]\n.EQ\ndelim $$\ndelim off\n.EN\ninline $eta$\n.EQ\ndelim on\n.EN\ninline $theta$\n",
        );
        let equations = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Equation)
            .collect::<Vec<_>>();
        assert_eq!(equations.len(), 8);
        assert_eq!(
            equations
                .iter()
                .filter_map(|node| node.equation())
                .collect::<Vec<_>>(),
            ["\\[*a]", "\\[*b]", "\\[*g]", "\\[*e]", "\\[*z]", "\\[*h]"]
        );
        let visible_text = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(visible_text.contains(&"inline [delta]"));
        assert!(visible_text.contains(&"inline $eta$"));
        assert_eq!(
            visible_text.iter().filter(|text| **text == "\\&").count(),
            3
        );
    }

    #[test]
    fn inline_equation_delimiters_remain_opaque_inside_tbl_cells() {
        let report = parse(
            b".TH TABLE-EQN 1\n.EQ\ndelim %%\n.EN\n.TS\nl l.\n%0%\tfor values in %[ 0 , ~pi over 2 ]%\n.TE\n",
        );
        let table = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Table)
            .unwrap();
        assert_eq!(table.table_cells()[0].text.as_deref(), Some("%0%"));
        assert_eq!(
            table.table_cells()[1].text.as_deref(),
            Some("for values in %[ 0 , ~pi over 2 ]%")
        );
    }

    #[test]
    fn equation_normalization_matches_legacy_layout_only_and_symbol_boxes() {
        let report = parse(
            b".TH EQN-COMPAT 1\n.EQ\nalpha hat sub 1 sup 2\nleft ceiling sqrt { a over 2 } right ceiling\nroman x size 12 y gsize 9 z\npile { one above two }\n.EN\n",
        );
        let equation = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Equation)
            .and_then(crate::NodeRef::equation)
            .unwrap();
        assert_eq!(
            equation,
            "\\[*a] _ 1 ^ 2 \\[lc]sqrt(a / 2)\\[rc] x y z one two"
        );
    }

    #[test]
    fn equation_definitions_support_delimiters_and_undef_without_visible_leaks() {
        let report =
            parse(b".TH EQN-DEFINE 1\n.EQ\ndefine item /alpha/\nitem\nundef item\nitem\n.EN\n");
        let equation = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Equation)
            .and_then(crate::NodeRef::equation)
            .unwrap();
        assert_eq!(equation, "\\[*a] item");
    }

    #[test]
    fn detects_trailing_empty_eqn_definition_requests() {
        assert_eq!(
            trailing_empty_request("define bruch 'over' 1 define"),
            "define"
        );
        assert_eq!(trailing_empty_request("define bruch"), "define bruch");
        assert_eq!(trailing_empty_request("alpha undef"), "undef");
        assert_eq!(trailing_empty_request("tdefine bruch"), "tdefine");
        assert!(trailing_empty_request("define item /alpha/").is_empty());
    }

    #[test]
    fn recursive_equation_definition_keeps_an_empty_equation_and_recovery() {
        let report = parse(b".TH EQN-RECURSION 1\n.EQ\ndefine key 'prefix key suffix' key\n.EN\n");
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["input stack limit exceeded, infinite loop?"]
        );
        let equation = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Equation)
            .expect("recursive display retains a finite equation node");
        assert_eq!(equation.equation(), None);
    }

    #[test]
    fn equation_definition_budget_returns_the_expanded_prefix() {
        let mut config = ParserConfig::default();
        config.limits.max_equation_tokens = 2;
        let report = Parser::new(config)
            .parse(Source::new(
                &SourceName::new("equation-definition-limit.1").unwrap(),
                b".TH EQN-LIMIT 1\n.EQ\ndefine item /a b c/\nitem\n.EN\n",
            ))
            .unwrap();
        assert!(report.statistics.truncated);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::LIMIT_EQUATION_TOKENS
        );
        let equation = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Equation)
            .and_then(crate::NodeRef::equation);
        assert_eq!(equation, Some("a b"));
    }

    #[test]
    fn default_equation_limit_returns_a_finite_legacy_compatible_prefix() {
        let mut source = String::from(".TH DEEP-EQN 1\n.EQ\n");
        for _ in 0..5_000 {
            source.push_str("sqrt { ");
        }
        source.push('x');
        for _ in 0..5_000 {
            source.push_str(" }");
        }
        source.push_str("\n.EN\n");

        let report = parse(source.as_bytes());
        assert!(report.statistics.truncated);
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_str() == DiagnosticCode::LEGACY_EQUATION_TREE_DEPTH_LIMIT
        }));
        let equation = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Equation)
            .and_then(crate::NodeRef::equation)
            .unwrap();
        assert!(equation.len() < 4_000, "equation prefix was not bounded");
    }

    #[test]
    fn equation_delim_on_reuses_only_a_delimiter_configured_in_that_display() {
        let report = parse(
            b".TH EQN-DELIM 1\n.EQ\ndelim %%\ndelim off\ndelim on\ntdefine ignored 'not-visible'\n.EN\nvalue %x sup 2% and $raw$\n",
        );
        let equations = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Equation)
            .filter_map(crate::NodeRef::equation)
            .collect::<Vec<_>>();
        assert_eq!(equations, ["x ^ 2"]);
        let text = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Text)
            .filter_map(crate::NodeRef::text)
            .collect::<Vec<_>>();
        assert!(text.contains(&"value \\&"));
        assert!(text.contains(&" and $raw$"));
    }

    #[test]
    fn table_and_equation_budgets_keep_finite_prefixes_with_typed_codes() {
        let mut table_config = ParserConfig::default();
        table_config.limits.max_table_rows = 1;
        let table = Parser::new(table_config)
            .parse(Source::new(
                &SourceName::new("table-limits.1").unwrap(),
                b".TH TABLE 1\n.TS\nl.\none\ntwo\n.TE\n",
            ))
            .unwrap();
        assert!(table.statistics.truncated);
        assert_eq!(
            table.diagnostics[0].code.as_str(),
            DiagnosticCode::LIMIT_TABLE_ROWS
        );
        assert_eq!(
            table
                .document
                .preorder()
                .filter(|node| node.kind() == NodeKind::Table)
                .count(),
            1
        );

        let mut equation_config = ParserConfig::default();
        equation_config.limits.max_equation_tokens = 3;
        let equation = Parser::new(equation_config)
            .parse(Source::new(
                &SourceName::new("equation-limits.1").unwrap(),
                b".TH EQN 1\n.EQ\na + b + c\n.EN\n",
            ))
            .unwrap();
        assert!(equation.statistics.truncated);
        assert_eq!(
            equation.diagnostics[0].code.as_str(),
            DiagnosticCode::LIMIT_EQUATION_TOKENS
        );
        assert_eq!(
            equation
                .document
                .preorder()
                .find(|node| node.kind() == NodeKind::Equation)
                .and_then(crate::NodeRef::equation),
            Some("a + b")
        );
    }

    #[test]
    fn unclosed_tbl_and_eqn_ranges_report_recoverable_typed_findings() {
        let report = parse(b".TH RECOVERY 1\n.TS\nl.\nT{\nunterminated cell\n.TE\n.EQ\nx sup 2\n");
        let codes = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            [
                DiagnosticCode::EQN_UNCLOSED_DISPLAY,
                DiagnosticCode::TBL_UNCLOSED_TEXT_BLOCK,
            ]
        );
        assert!(!report.statistics.truncated);
    }
}
