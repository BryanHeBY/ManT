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

mod eqn;
mod tbl;

#[cfg(feature = "render")]
pub(crate) use eqn::normalize_equation_symbol;
#[cfg(test)]
use eqn::trailing_empty_request;
use eqn::{
    DelimiterChange, equation_depth, equation_limit, equation_tokens, expand_definitions,
    normalize_equation_tokens, parse_equation,
};
use tbl::parse_table_rows;
#[cfg(test)]
use tbl::{CellFormat, parse_format_row, parse_layout_line, table_cell_text, table_field_text};

#[cfg(test)]
mod tests;
