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
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use mant_ir::EntryKind;
use mant_render::cells::{graphemes, prefix_columns, suffix_columns};

use crate::{NavKind, NavNode, text::sanitize_terminal_text, theme};

const NODE_LEFT_PADDING: &str = " ";
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
    visible
        .iter()
        .flat_map(|index| {
            node_lines(
                &nodes[*index],
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
    nodes: &[NavNode],
    visible: &[usize],
    selected: usize,
    expanded: &HashSet<String>,
    full_labels: bool,
    width: usize,
    badges: &HashMap<String, String>,
) -> Vec<NavigationRow> {
    visible
        .iter()
        .flat_map(|index| {
            let node = &nodes[*index];
            let mut lines = node_lines(
                node,
                *index,
                *index == selected,
                expanded.contains(&node.id),
                full_labels,
                width,
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
            let continuation_text = format!(
                "{NODE_LEFT_PADDING}  {}",
                continuation_prefix(node, expanded.contains(&node.id))
            );
            let continuation =
                Span::styled(bounded_tree_prefix(&continuation_text, width), prefix.style);
            let style = first[1].style;
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
            let link_style = style.fg(theme::LINK);
            let mut current = vec![prefix.clone()];
            let mut used = 0;
            lines.clear();
            for (text, role) in [(title, style), (badge, link_style)] {
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
) -> (String, String) {
    if expanded {
        return (title.to_owned(), format!(" {badge}"));
    }
    // Once a deep tree leaves fewer than three cells there is no room for
    // owner + separator + reference marker. Preserve the explicit capability,
    // not a second compact row containing only an invisible separator.
    if available < 3 {
        return (String::new(), prefix_columns(badge, available).to_owned());
    }
    let title = truncate_middle(
        title,
        available.saturating_sub((badge.width() + 1).min(available / 2) + 1),
    );
    let badge = prefix_columns(badge, available.saturating_sub(title.width() + 1));
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
        Style::default().bg(style.bg.unwrap_or(theme::SIDEBAR)),
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

fn node_lines(
    node: &NavNode,
    node_index: usize,
    selected: bool,
    expanded: bool,
    full_labels: bool,
    width: usize,
) -> Vec<NavigationRow> {
    if width == 0 {
        return vec![NavigationRow {
            node_index,
            line: Line::default(),
        }];
    }
    let selection = if selected { "› " } else { "  " };
    let prefix = bounded_tree_prefix(
        &format!(
            "{NODE_LEFT_PADDING}{selection}{}",
            tree_prefix(node, expanded)
        ),
        width,
    );
    let continuation_prefix = bounded_tree_prefix(
        &format!(
            "{NODE_LEFT_PADDING}  {}",
            continuation_prefix(node, expanded)
        ),
        width,
    );
    let foreground = node_foreground(node, selected);
    let background = if selected {
        if node.kind == NavKind::Tldr {
            theme::TLDR_SELECTED
        } else {
            theme::SELECTED
        }
    } else if node.kind == NavKind::Tldr {
        theme::TLDR_NAV
    } else {
        theme::SIDEBAR
    };
    let mut style = Style::default().fg(foreground).bg(background);
    if selected {
        style = style.add_modifier(Modifier::BOLD);
    }

    let title = sanitize_terminal_text(if selected || full_labels {
        node.full_title.as_deref().unwrap_or(&node.title)
    } else {
        &node.title
    });
    let first_title_width = width.saturating_sub(prefix.width()).max(1);
    let wrapped_title_width = width
        .saturating_sub(prefix.width().max(continuation_prefix.width()))
        .max(1);
    let titles = if selected || full_labels {
        wrap_to_width(&title, wrapped_title_width)
    } else {
        vec![truncate_middle(&title, first_title_width)]
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
            let title = if title.width() > width.saturating_sub(line_prefix.width()) {
                // Only chrome substitutes a grapheme wider than its whole cell
                // budget; the original document/search text is unchanged.
                "�".to_owned()
            } else {
                title
            };
            let used = line_prefix.width() + title.width();
            let prefix_color = if selected {
                if line_index == 0 {
                    theme::PEACH
                } else {
                    theme::PINK
                }
            } else {
                theme::OVERLAY
            };
            NavigationRow {
                node_index,
                line: Line::from(vec![
                    Span::styled(
                        line_prefix,
                        Style::default().fg(prefix_color).bg(background),
                    ),
                    Span::styled(title, style),
                    Span::styled(
                        " ".repeat(width.saturating_sub(used)),
                        Style::default().bg(background),
                    ),
                ]),
            }
        })
        .collect()
}

fn bounded_tree_prefix(prefix: &str, width: usize) -> String {
    // Retain the nearest branch/owner marker when ancestor columns no longer
    // fit, always leaving one content cell for label or reference capability.
    suffix_columns(prefix, width.saturating_sub(1)).to_owned()
}

fn node_foreground(node: &NavNode, selected: bool) -> ratatui::style::Color {
    if selected {
        return if node.kind == NavKind::Tldr {
            theme::MAUVE
        } else {
            theme::SELECTED_TEXT
        };
    }
    match node.kind {
        NavKind::Tldr => theme::MAUVE,
        NavKind::Root | NavKind::Section if node.depth == 0 => theme::SUBTEXT_BRIGHT,
        NavKind::Root | NavKind::Section | NavKind::ReferenceGroup => theme::BLUE,
        NavKind::EntryGroup | NavKind::ReferenceNotice => theme::YELLOW,
        NavKind::Entry(EntryKind::Term) => theme::STRONG,
        NavKind::Entry(kind) => theme::entry_color(kind),
        NavKind::Reference => theme::LINK,
    }
}

fn tree_prefix(node: &NavNode, expanded: bool) -> String {
    if node.kind == NavKind::Tldr {
        return "◆ ".to_owned();
    }
    let mut prefix = "│ ".repeat(node.depth);
    if node.depth == 0 {
        if node.has_children {
            prefix.push_str("│ ");
        }
    } else {
        prefix.push_str(if node.is_last && !expanded {
            "╰─"
        } else {
            "├─"
        });
    }
    prefix.push_str(if node.has_children {
        if expanded { "▾ " } else { "▸ " }
    } else if matches!(node.kind, NavKind::Entry(_)) {
        "◇ "
    } else {
        "· "
    });
    prefix
}

fn continuation_prefix(node: &NavNode, expanded: bool) -> String {
    if node.kind == NavKind::Tldr {
        return "  ".to_owned();
    }
    let mut prefix = "│ ".repeat(node.depth);
    if node.depth > 0 {
        prefix.push_str(if node.is_last && !expanded {
            "  "
        } else {
            "│ "
        });
    } else if node.has_children {
        prefix.push_str("│ ");
    }
    if node.has_children && expanded {
        prefix.push_str("│ ");
    }
    prefix.push_str("  ");
    prefix
}

pub(crate) fn truncate_middle(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.to_owned();
    }
    let marker_width = TRUNCATION_MARKER.width();
    if width <= marker_width {
        return TRUNCATION_MARKER.chars().take(width).collect();
    }

    // Manual outlines often contain many headings with the same opening words,
    // so spend roughly one third of the remaining columns on that context and
    // two thirds on the more discriminating suffix.
    let remaining = width - marker_width;
    let prefix_width = remaining / 3;
    let suffix_width = remaining - prefix_width;
    format!(
        "{}{}{}",
        prefix_columns(value, prefix_width),
        TRUNCATION_MARKER,
        suffix_columns(value, suffix_width)
    )
}

fn wrap_to_width(value: &str, width: usize) -> Vec<String> {
    if value.width() <= width {
        return vec![value.to_owned()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in value.split_whitespace() {
        let separator = usize::from(!current.is_empty());
        if current.width() + separator + word.width() <= width {
            if separator == 1 {
                current.push(' ');
            }
            current.push_str(word);
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        let mut remaining = word;
        while remaining.width() > width {
            let split = prefix_columns(remaining, width).len();
            if split == 0 {
                let Some(grapheme) = graphemes(remaining).next() else {
                    break;
                };
                remaining = &remaining[grapheme.text().len()..];
                lines.push("�".to_owned());
                continue;
            }
            lines.push(remaining[..split].to_owned());
            remaining = &remaining[split..];
        }
        current.push_str(remaining);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
    use unicode_width::UnicodeWidthStr;

    use super::{node_lines, node_row_range, truncate_middle};
    use crate::{NavKind, NavNode, theme};

    fn rendered_symbols(rows: &[super::NavigationRow], width: u16) -> Vec<String> {
        let height = u16::try_from(rows.len()).expect("test rows fit a terminal");
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
        for (index, row) in rows.iter().enumerate() {
            let y = u16::try_from(index).expect("test row fits a terminal");
            row.line
                .clone()
                .render(Rect::new(0, y, width, 1), &mut buffer);
        }
        buffer
            .content
            .iter()
            .map(|cell| cell.symbol().to_owned())
            .collect()
    }

    #[test]
    fn associated_unicode_titles_render_as_whole_terminal_graphemes() {
        let nodes = vec![node("Cafe\u{301} 👩‍💻")];
        let badges = [("node".to_owned(), "↗ Cafe\u{301}👩‍💻".to_owned())]
            .into_iter()
            .collect();
        for width in [11_u16, 12, 16, 80] {
            for (selected, full) in [(0, false), (usize::MAX, true), (usize::MAX, false)] {
                let rows = super::rows_with_references(
                    &nodes,
                    &[0],
                    selected,
                    &HashSet::new(),
                    full,
                    usize::from(width),
                    &badges,
                );
                let symbols = rendered_symbols(&rows, width);
                assert!(
                    rows.iter()
                        .all(|row| row.line.width() <= usize::from(width))
                );
                // Inspect real terminal cells, not just reconstructed strings:
                // scalar Spans preserve the latter while corrupting the former.
                for symbol in &symbols {
                    if symbol.contains('\u{301}') {
                        assert_eq!(symbol, "e\u{301}");
                    }
                    if symbol.contains(['👩', '💻', '\u{200d}']) {
                        assert_eq!(symbol, "👩‍💻");
                    }
                }
                if selected == 0 || full || width == 80 {
                    assert!(symbols.iter().any(|symbol| symbol == "e\u{301}"));
                    assert!(symbols.iter().any(|symbol| symbol == "👩‍💻"));
                }
                if selected == usize::MAX && !full {
                    assert_eq!(rows.len(), 1);
                }
            }
        }
    }

    #[test]
    fn ordinary_navigation_uses_the_same_grapheme_safe_width_boundaries() {
        for width in [10_u16, 11, 12, 16, 80] {
            let rows = node_lines(
                &node("Cafe\u{301}👩‍💻Suffix"),
                0,
                true,
                false,
                false,
                usize::from(width),
            );
            let symbols = rendered_symbols(&rows, width);
            assert!(symbols.iter().any(|symbol| symbol == "e\u{301}"));
            if width > 10 {
                assert!(symbols.iter().any(|symbol| symbol == "👩‍💻"));
            } else {
                assert!(symbols.iter().any(|symbol| symbol == "�"));
            }
            assert!(
                !symbols
                    .iter()
                    .any(|symbol| matches!(symbol.as_str(), "👩" | "💻"))
            );
            assert!(
                rows.iter()
                    .all(|row| row.line.width() <= usize::from(width))
            );
        }
        assert_eq!(super::prefix_columns("👩‍💻x", 1), "");
        assert_eq!(super::prefix_columns("e\u{301}x", 1), "e\u{301}");
        assert_eq!(super::suffix_columns("x👩‍💻", 1), "");
        assert_eq!(super::suffix_columns("xe\u{301}", 1), "e\u{301}");
        assert_eq!(truncate_middle("012345👩‍💻", 6), "0...👩‍💻");
    }

    #[test]
    fn associated_badges_wrap_with_reference_style_without_recoloring_the_owner() {
        let mut owner = node("日本 command");
        owner.kind = NavKind::Entry(mant_ir::EntryKind::Command);
        let nodes = vec![owner];
        let badges = [("node".to_owned(), "↗ target#section".to_owned())]
            .into_iter()
            .collect();
        for width in [18, 30, 80] {
            for selected in [0, usize::MAX] {
                let rows = super::rows_with_references(
                    &nodes,
                    &[0],
                    selected,
                    &HashSet::new(),
                    false,
                    width,
                    &badges,
                );
                assert!(rows.iter().all(|row| row.line.width() <= width));
                let text = rows
                    .iter()
                    .map(|row| row.line.to_string())
                    .collect::<String>();
                assert!(text.contains('↗'), "{width}: {text}");
                assert!(
                    rows.iter()
                        .flat_map(|row| &row.line.spans)
                        .any(
                            |span| span.content.contains('↗') && span.style.fg == Some(theme::LINK)
                        )
                );
                if selected == usize::MAX {
                    assert_eq!(rows.len(), 1);
                    assert!(rows[0].line.spans.iter().any(|span| span.style.fg
                        == Some(theme::entry_color(mant_ir::EntryKind::Command))));
                } else {
                    let expected = format!(
                        "{}  {}",
                        super::NODE_LEFT_PADDING,
                        super::continuation_prefix(&nodes[0], false)
                    );
                    assert!(
                        rows.iter()
                            .skip(1)
                            .all(|row| row.line.spans[0].content == expected)
                    );
                }
            }
        }
    }

    #[test]
    fn deep_associated_nodes_keep_markers_inside_tiny_viewports() {
        let badges = [("node".to_owned(), "↗ target#part".to_owned())]
            .into_iter()
            .collect();
        for depth in [4, 8] {
            let mut owner = node("日本 command");
            owner.depth = depth;
            owner.has_children = true;
            let nodes = vec![owner];
            for width in [0, 1, 4, 8] {
                let plain = node_lines(&nodes[0], 0, true, true, true, width);
                assert!(plain.iter().all(|row| row.line.width() <= width));
                for (selected, full) in [(0, false), (usize::MAX, true), (usize::MAX, false)] {
                    let rows = super::rows_with_references(
                        &nodes,
                        &[0],
                        selected,
                        &["node".to_owned()].into_iter().collect(),
                        full,
                        width,
                        &badges,
                    );
                    assert!(
                        rows.iter().all(|row| row.line.width() <= width),
                        "depth={depth}, width={width}"
                    );
                    if width > 0 {
                        assert!(rows.iter().any(|row| row.line.to_string().contains('↗')));
                    }
                    if selected == usize::MAX && !full {
                        assert_eq!(rows.len(), 1);
                    }
                }
            }
        }
    }

    fn node(title: &str) -> NavNode {
        NavNode {
            id: "node".to_owned(),
            target_id: "node".to_owned(),
            title: title.to_owned(),
            full_title: None,
            depth: 1,
            kind: NavKind::Section,
            has_children: false,
            is_last: true,
            parent_id: Some("parent".to_owned()),
        }
    }

    #[test]
    fn inactive_long_titles_keep_both_identifying_ends_on_one_row() {
        let rows = node_lines(
            &node("Options Controlling the Kind of Output"),
            0,
            false,
            false,
            false,
            31,
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].line.to_string(), "   │ ╰─· Option...ind of Output");
        assert_eq!(rows[0].line.width(), 31);
    }

    #[test]
    fn selected_long_titles_expand_without_losing_text_or_background() {
        let rows = node_lines(
            &node("Options Controlling the Kind of Output"),
            0,
            true,
            false,
            false,
            31,
        );
        let visible_title = rows
            .iter()
            .map(|row| row.line.to_string())
            .collect::<Vec<_>>()
            .join(" ");

        assert_eq!(rows.len(), 2);
        assert!(visible_title.contains("Options Controlling"));
        assert!(visible_title.contains("the Kind of Output"));
        assert!(rows.iter().all(|row| row.line.width() == 31));
        assert!(rows.iter().all(|row| {
            row.line
                .spans
                .iter()
                .all(|span| span.style.bg == Some(theme::SELECTED))
        }));
    }

    #[test]
    fn semantic_entries_separate_compact_identity_from_complete_forms() {
        let mut entry = node("-L");
        entry.kind = NavKind::Entry(mant_ir::EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option,
        });
        entry.full_title =
            Some("-L [bind_address:]port:host:hostport | -L local_socket:remote_socket".to_owned());

        let compact = node_lines(&entry, 0, false, false, false, 31);
        let selected = node_lines(&entry, 0, true, false, false, 31);
        let full = node_lines(&entry, 0, false, false, true, 31);

        assert_eq!(compact.len(), 1);
        assert!(compact[0].line.to_string().contains("-L"));
        assert!(!compact[0].line.to_string().contains("bind_address"));
        assert!(selected.len() > 1);
        assert_eq!(full.len(), selected.len());
        let complete = full
            .iter()
            .map(|row| row.line.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(complete.contains("bind_address"));
        assert!(complete.contains("local_socket"));
    }

    #[test]
    fn wrapped_last_leaf_ends_its_branch_on_continuation_rows() {
        let rows = node_lines(
            &node("Options Controlling the Kind of Output"),
            0,
            true,
            false,
            false,
            31,
        );
        let text = rows
            .iter()
            .map(|row| row.line.to_string())
            .collect::<Vec<_>>();

        assert!(text.len() > 1);
        assert!(text[0].starts_with(" › │ ╰─· "));
        assert!(text[1].starts_with("   │     "));
        assert!(!text[1].starts_with("   │ │   "));
    }

    #[test]
    fn navigation_titles_cannot_emit_terminal_controls() {
        let rows = node_lines(&node("unsafe\u{1b}[31m\nname"), 0, true, false, false, 31);
        let text = rows
            .iter()
            .map(|row| row.line.to_string())
            .collect::<String>();

        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains('\n'));
        assert!(text.contains('�'));
    }

    #[test]
    fn row_ranges_include_every_continuation_line() {
        let nodes = vec![node("Options Controlling the Kind of Output")];
        let rows = super::rows(&nodes, &[0], 0, &HashSet::new(), false, 18);

        assert_eq!(node_row_range(&rows, 0), Some(0..rows.len()));
        assert!(rows.len() > 1);
    }

    #[test]
    fn expanded_selected_parents_keep_guides_through_continuation_rows() {
        let mut parent = node("A deliberately long expanded parent title");
        parent.depth = 0;
        parent.has_children = true;
        parent.parent_id = None;

        let rows = node_lines(&parent, 0, true, true, false, 23);
        let text = rows
            .iter()
            .map(|row| row.line.to_string())
            .collect::<Vec<_>>();

        assert!(text.len() > 1);
        assert!(text[0].starts_with(" › │ ▾ "));
        assert!(text[1].starts_with("   │ │   "));
        assert!(
            rows[1]
                .line
                .spans
                .iter()
                .all(|span| span.style.bg == Some(theme::SELECTED))
        );
    }

    #[test]
    fn nested_rows_keep_two_column_tree_guides() {
        let mut leaf = node("Leaf");
        leaf.is_last = false;

        let row = node_lines(&leaf, 0, false, false, false, 24)
            .remove(0)
            .line
            .to_string();

        assert!(row.starts_with("   │ ├─· Leaf"));
    }

    #[test]
    fn middle_truncation_is_terminal_column_aware() {
        let truncated = truncate_middle("编译器选项与输出格式", 10);
        assert!(truncated.width() <= 10);
        assert!(truncated.contains("..."));
    }
}
