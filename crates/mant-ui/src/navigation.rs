//! Builds the sidebar's fixed-height visual rows from logical navigation nodes.
//!
//! Long labels deliberately have two states: inactive nodes remain one row and
//! retain both identifying ends through middle truncation, while the selected
//! node expands to as many wrapped rows as required. Keeping both policies in
//! one model also lets scrolling reason about the selected node's whole range.

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use mant_render::cells::{graphemes, prefix_columns, suffix_columns};

use crate::{NavNode, text::sanitize_terminal_text, theme};

pub(crate) mod search;
mod tree;
pub(crate) use tree::TreePlan;
#[cfg(test)]
pub(crate) use tree::{start_layout_trace, take_layout_trace};
const TRUNCATION_MARKER: &str = "...";

pub(crate) struct NavigationRow {
    pub(crate) node_index: usize,
    pub(crate) line: Line<'static>,
}

#[cfg(test)]
pub(crate) fn rows(
    nodes: &[NavNode],
    visible: &[usize],
    selected: usize,
    expanded: &HashSet<String>,
    full_labels: bool,
    width: usize,
) -> Vec<NavigationRow> {
    let plan = TreePlan::new(nodes);
    visible
        .iter()
        .flat_map(|index| {
            planned_node_lines(
                &plan,
                *index,
                *index == selected,
                expanded.contains(&nodes[*index].id),
                full_labels,
                width,
            )
        })
        .collect()
}

/// Decorate validated owner associations without changing source labels or IDs.
pub(crate) fn rows_with_references(
    plan: &TreePlan<'_>,
    visible: &[usize],
    selected: usize,
    expanded: &HashSet<String>,
    full_labels: bool,
    width: usize,
    badges: &HashMap<String, String>,
) -> Vec<NavigationRow> {
    #[cfg(test)]
    plan.record_layout(width);
    visible
        .iter()
        .flat_map(|index| {
            let node = &plan.nodes[*index];
            let prefixes = plan.prefixes(
                *index,
                *index == selected,
                expanded.contains(&node.id),
                width,
            );
            let mut lines = node_lines_with_prefixes(
                node,
                *index,
                *index == selected,
                full_labels,
                width,
                &prefixes,
                plan,
            );
            let Some(badge) = badges.get(&node.id) else {
                return lines;
            };
            if width == 0 {
                return vec![NavigationRow {
                    node_index: *index,
                    line: Line::default(),
                }];
            }
            let first = &lines[0].line.spans;
            let prefix = Span::styled(
                bounded_tree_prefix(&first[0].content, width),
                first[0].style,
            );
            let continuation = Span::styled(prefixes.continuation, prefix.style);
            let style = theme::navigation_style(node.kind, *index == selected);
            let available = width
                .saturating_sub(prefix.width().max(continuation.width()))
                .max(1);
            let title = sanitize_terminal_text(if *index == selected || full_labels {
                node.full_title.as_deref().unwrap_or(&node.title)
            } else {
                &node.title
            });
            let badge = sanitize_terminal_text(badge);
            let expanded_label = *index == selected || full_labels;
            let (title, badge) = reference_title_parts(&title, &badge, available, expanded_label);
            let link_style = theme::navigation_reference_style(node.kind, *index == selected);
            let original = sanitize_terminal_text(if expanded_label {
                node.full_title.as_deref().unwrap_or(&node.title)
            } else {
                &node.title
            });
            let label = plan.label_styles(*index, &original);
            let mut parts = label_spans(&title, label, node.kind, *index == selected);
            parts.push(Span::styled(badge, link_style));
            let mut current = vec![prefix.clone()];
            let mut used = 0;
            lines.clear();
            for part in parts {
                let text = part.content;
                let role = part.style;
                let mut run = String::new();
                // Ratatui segments each Span independently. Keep contiguous
                // style runs intact and break only at the same grapheme
                // boundaries used by its terminal renderer.
                for grapheme in graphemes(&text) {
                    let columns = grapheme.columns();
                    if used + columns > available && used > 0 {
                        if !run.is_empty() {
                            current.push(Span::styled(std::mem::take(&mut run), role));
                        }
                        lines.push(finish_reference_row(current, *index, width, style));
                        current = vec![continuation.clone()];
                        used = 0;
                    }
                    // Substitute a whole grapheme only when even an otherwise
                    // empty row cannot hold it; never emit a partial cluster.
                    let rendered = if columns > available {
                        "�"
                    } else {
                        grapheme.text()
                    };
                    used += columns.min(available);
                    run.push_str(rendered);
                }
                if !run.is_empty() {
                    current.push(Span::styled(run, role));
                }
            }
            lines.push(finish_reference_row(current, *index, width, style));
            lines
        })
        .collect()
}

fn reference_title_parts(
    title: &str,
    badge: &str,
    available: usize,
    expanded: bool,
) -> (Vec<LabelPiece>, String) {
    if expanded {
        return (vec![LabelPiece::source(title, 0)], format!(" {badge}"));
    }
    // Once a deep tree leaves fewer than three cells there is no room for
    // owner + separator + reference marker. Preserve the explicit capability,
    // not a second compact row containing only an invisible separator.
    if available < 3 {
        return (Vec::new(), prefix_columns(badge, available).to_owned());
    }
    let title = truncate_label(
        title,
        available.saturating_sub((badge.width() + 1).min(available / 2) + 1),
    );
    let used = title.iter().map(|piece| piece.text.width()).sum::<usize>();
    let badge = prefix_columns(badge, available.saturating_sub(used + 1));
    (title, format!(" {badge}"))
}

fn finish_reference_row(
    mut spans: Vec<Span<'static>>,
    node_index: usize,
    width: usize,
    style: Style,
) -> NavigationRow {
    let used: usize = spans.iter().map(Span::width).sum();
    spans.push(Span::styled(
        " ".repeat(width.saturating_sub(used)),
        Style {
            bg: style.bg,
            ..Style::default()
        },
    ));
    NavigationRow {
        node_index,
        line: Line::from(spans),
    }
}

/// Returns the complete half-open row range occupied by one outline node.
pub(crate) fn node_row_range(rows: &[NavigationRow], node_index: usize) -> Option<Range<usize>> {
    let start = rows.iter().position(|row| row.node_index == node_index)?;
    let end = rows
        .iter()
        .rposition(|row| row.node_index == node_index)?
        .saturating_add(1);
    Some(start..end)
}

#[cfg(test)]
fn planned_node_lines(
    plan: &TreePlan<'_>,
    node_index: usize,
    selected: bool,
    expanded: bool,
    full_labels: bool,
    width: usize,
) -> Vec<NavigationRow> {
    let prefixes = plan.prefixes(node_index, selected, expanded, width);
    node_lines_with_prefixes(
        &plan.nodes[node_index],
        node_index,
        selected,
        full_labels,
        width,
        &prefixes,
        plan,
    )
}

fn node_lines_with_prefixes(
    node: &NavNode,
    node_index: usize,
    selected: bool,
    full_labels: bool,
    width: usize,
    prefixes: &tree::Prefixes,
    plan: &TreePlan<'_>,
) -> Vec<NavigationRow> {
    if width == 0 {
        return vec![NavigationRow {
            node_index,
            line: Line::default(),
        }];
    }
    let prefix = &prefixes.first;
    let continuation_prefix = &prefixes.continuation;
    let style = theme::navigation_style(node.kind, selected);

    let title = sanitize_terminal_text(if selected || full_labels {
        node.full_title.as_deref().unwrap_or(&node.title)
    } else {
        &node.title
    });
    let label = plan.label_styles(node_index, &title);
    let first_title_width = width.saturating_sub(prefix.width()).max(1);
    let wrapped_title_width = width
        .saturating_sub(prefix.width().max(continuation_prefix.width()))
        .max(1);
    let titles = if selected || full_labels {
        wrap_to_width(&title, wrapped_title_width)
    } else {
        vec![truncate_label(&title, first_title_width)]
    };

    titles
        .into_iter()
        .enumerate()
        .map(|(line_index, title)| {
            let line_prefix = if line_index == 0 {
                prefix.clone()
            } else {
                continuation_prefix.clone()
            };
            let title_width = title.iter().map(|piece| piece.text.width()).sum::<usize>();
            let title = if title_width > width.saturating_sub(line_prefix.width()) {
                // Only chrome substitutes a grapheme wider than its whole cell
                // budget; the original document/search text is unchanged.
                vec![LabelPiece::generated("�")]
            } else {
                title
            };
            let used =
                line_prefix.width() + title.iter().map(|piece| piece.text.width()).sum::<usize>();
            let prefix_role = if selected {
                if line_index == 0 {
                    theme::StyleRole::TreeFocus
                } else {
                    theme::StyleRole::TreeContinuationFocus
                }
            } else {
                theme::StyleRole::TreeGuide
            };
            let mut spans = vec![Span::styled(
                line_prefix,
                Style {
                    bg: style.bg,
                    ..theme::style(prefix_role)
                },
            )];
            spans.extend(label_spans(&title, label, node.kind, selected));
            spans.push(Span::styled(
                " ".repeat(width.saturating_sub(used)),
                Style {
                    bg: style.bg,
                    ..Style::default()
                },
            ));
            NavigationRow {
                node_index,
                line: Line::from(spans),
            }
        })
        .collect()
}

fn bounded_tree_prefix(prefix: &str, width: usize) -> String {
    // Retain the nearest branch/owner marker when ancestor columns no longer
    // fit, always leaving one content cell for label or reference capability.
    suffix_columns(prefix, width.saturating_sub(1)).to_owned()
}

pub(crate) fn truncate_middle(value: &str, width: usize) -> String {
    truncate_label(value, width)
        .into_iter()
        .map(|piece| piece.text)
        .collect()
}

/// Source byte offsets survive wrapping/truncation; generated chrome has none.
struct LabelPiece {
    text: String,
    byte: Option<usize>,
}

impl LabelPiece {
    fn source(text: &str, byte: usize) -> Self {
        Self {
            text: text.to_owned(),
            byte: Some(byte),
        }
    }
    fn generated(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            byte: None,
        }
    }
}

fn label_spans(
    pieces: &[LabelPiece],
    label: Option<&crate::entry::LabelStyles>,
    kind: crate::NavKind,
    selected: bool,
) -> Vec<Span<'static>> {
    if label.is_none() {
        return vec![Span::styled(
            pieces
                .iter()
                .map(|piece| piece.text.as_str())
                .collect::<String>(),
            theme::navigation_style(kind, selected),
        )];
    }
    let base = theme::navigation_style(kind, false);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for piece in pieces {
        let mut byte = piece.byte;
        for glyph in graphemes(&piece.text) {
            let style = label
                .and_then(|label| byte.and_then(|byte| label.role_at(byte)))
                .map_or(base, |role| base.patch(theme::style(role)));
            let style = theme::DEFAULT_THEME.navigation_overlay(style, kind, selected);
            if let Some(last) = spans.last_mut()
                && last.style == style
            {
                last.content.to_mut().push_str(glyph.text());
            } else {
                spans.push(Span::styled(glyph.text().to_owned(), style));
            }
            byte = byte.map(|byte| byte + glyph.text().len());
        }
    }
    // Empty labels still retain the ordinary label slot for existing chrome.
    if spans.is_empty() {
        spans.push(Span::styled(
            String::new(),
            theme::navigation_style(kind, selected),
        ));
    }
    spans
}

fn truncate_label(value: &str, width: usize) -> Vec<LabelPiece> {
    if value.width() <= width {
        return vec![LabelPiece::source(value, 0)];
    }
    let marker_width = TRUNCATION_MARKER.width();
    if width <= marker_width {
        return vec![LabelPiece::generated(
            &TRUNCATION_MARKER.chars().take(width).collect::<String>(),
        )];
    }

    // Manual outlines often contain many headings with the same opening words,
    // so spend roughly one third of the remaining columns on that context and
    // two thirds on the more discriminating suffix.
    let remaining = width - marker_width;
    let prefix_width = remaining / 3;
    let suffix_width = remaining - prefix_width;
    let suffix = suffix_columns(value, suffix_width);
    vec![
        LabelPiece::source(prefix_columns(value, prefix_width), 0),
        LabelPiece::generated(TRUNCATION_MARKER),
        LabelPiece::source(suffix, value.len() - suffix.len()),
    ]
}

fn wrap_to_width(value: &str, width: usize) -> Vec<Vec<LabelPiece>> {
    if value.width() <= width {
        return vec![vec![LabelPiece::source(value, 0)]];
    }
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut used = 0;
    let mut byte = 0;
    for word in value.split_whitespace() {
        // split_whitespace removes/normalizes spaces, but no name/text search
        // is needed to keep the source coordinates of each real word.
        while value[byte..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            byte += value[byte..].chars().next().expect("whitespace").len_utf8();
        }
        let word_byte = byte;
        byte += word.len();
        let separator = usize::from(!current.is_empty());
        if used + separator + word.width() <= width {
            if separator == 1 {
                current.push(LabelPiece::generated(" "));
            }
            current.push(LabelPiece::source(word, word_byte));
            used += separator + word.width();
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            used = 0;
        }
        let mut remaining = word;
        while remaining.width() > width {
            let split = prefix_columns(remaining, width).len();
            if split == 0 {
                let Some(grapheme) = graphemes(remaining).next() else {
                    break;
                };
                remaining = &remaining[grapheme.text().len()..];
                lines.push(vec![LabelPiece::generated("�")]);
                continue;
            }
            lines.push(vec![LabelPiece::source(
                &remaining[..split],
                word_byte + word.len() - remaining.len(),
            )]);
            remaining = &remaining[split..];
        }
        if !remaining.is_empty() {
            current.push(LabelPiece::source(
                remaining,
                word_byte + word.len() - remaining.len(),
            ));
            used += remaining.width();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(vec![LabelPiece::generated("")]);
    }
    lines
}

#[cfg(test)]
mod tests;
