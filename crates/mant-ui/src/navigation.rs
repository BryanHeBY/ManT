//! Builds the sidebar's fixed-height visual rows from logical navigation nodes.
//!
//! Long labels deliberately have two states: inactive nodes remain one row and
//! retain both identifying ends through middle truncation, while the selected
//! node expands to as many wrapped rows as required. Keeping both policies in
//! one model also lets scrolling reason about the selected node's whole range.

use std::{collections::HashSet, ops::Range};

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{NavItem, NavKind, theme};

const ITEM_LEFT_PADDING: &str = " ";
const TRUNCATION_MARKER: &str = "...";

pub(crate) struct NavigationRow {
    pub(crate) item_index: usize,
    pub(crate) line: Line<'static>,
}

pub(crate) fn rows(
    items: &[NavItem],
    visible: &[usize],
    selected: usize,
    expanded: &HashSet<String>,
    width: usize,
) -> Vec<NavigationRow> {
    visible
        .iter()
        .flat_map(|index| {
            item_lines(
                &items[*index],
                *index,
                *index == selected,
                expanded.contains(&items[*index].id),
                width,
            )
        })
        .collect()
}

/// Returns the complete half-open row range occupied by one navigation item.
pub(crate) fn item_row_range(rows: &[NavigationRow], item_index: usize) -> Option<Range<usize>> {
    let start = rows.iter().position(|row| row.item_index == item_index)?;
    let end = rows
        .iter()
        .rposition(|row| row.item_index == item_index)?
        .saturating_add(1);
    Some(start..end)
}

fn item_lines(
    item: &NavItem,
    item_index: usize,
    selected: bool,
    expanded: bool,
    width: usize,
) -> Vec<NavigationRow> {
    let selection = if selected { "› " } else { "  " };
    let prefix = format!(
        "{ITEM_LEFT_PADDING}{selection}{}",
        tree_prefix(item, expanded)
    );
    let continuation_prefix = format!(
        "{ITEM_LEFT_PADDING}  {}",
        continuation_prefix(item, expanded)
    );
    let foreground = if selected {
        if item.kind == NavKind::Tldr {
            theme::MAUVE
        } else {
            theme::SELECTED_TEXT
        }
    } else {
        match item.kind {
            NavKind::Tldr => theme::MAUVE,
            NavKind::Root | NavKind::Section if item.depth == 0 => theme::SUBTEXT_BRIGHT,
            NavKind::Root | NavKind::Section => theme::BLUE,
            NavKind::EntryGroup => theme::YELLOW,
            NavKind::Option => theme::GREEN,
        }
    };
    let background = if selected {
        if item.kind == NavKind::Tldr {
            theme::TLDR_SELECTED
        } else {
            theme::SELECTED
        }
    } else if item.kind == NavKind::Tldr {
        theme::TLDR_NAV
    } else {
        theme::SIDEBAR
    };
    let mut style = Style::default().fg(foreground).bg(background);
    if selected {
        style = style.add_modifier(Modifier::BOLD);
    }

    let first_title_width = width.saturating_sub(prefix.width()).max(1);
    let wrapped_title_width = width
        .saturating_sub(prefix.width().max(continuation_prefix.width()))
        .max(1);
    let titles = if selected {
        wrap_to_width(&item.title, wrapped_title_width)
    } else {
        vec![truncate_middle(&item.title, first_title_width)]
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
                item_index,
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

fn tree_prefix(item: &NavItem, expanded: bool) -> String {
    if item.kind == NavKind::Tldr {
        return "◆ ".to_owned();
    }
    let mut prefix = "│ ".repeat(item.depth);
    if item.depth == 0 {
        if item.has_children {
            prefix.push_str("│ ");
        }
    } else {
        prefix.push_str(if item.is_last && !expanded {
            "╰─"
        } else {
            "├─"
        });
    }
    prefix.push_str(if item.has_children {
        if expanded { "▾ " } else { "▸ " }
    } else if item.kind == NavKind::Option {
        "◇ "
    } else {
        "· "
    });
    prefix
}

fn continuation_prefix(item: &NavItem, expanded: bool) -> String {
    if item.kind == NavKind::Tldr {
        return "  ".to_owned();
    }
    let mut prefix = "│ ".repeat(item.depth);
    if item.depth > 0 || item.has_children {
        prefix.push_str("│ ");
    }
    if item.has_children && expanded {
        prefix.push_str("│ ");
    }
    prefix.push_str("  ");
    prefix
}

fn truncate_middle(value: &str, width: usize) -> String {
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
        take_prefix_columns(value, prefix_width),
        TRUNCATION_MARKER,
        take_suffix_columns(value, suffix_width)
    )
}

fn take_prefix_columns(value: &str, width: usize) -> &str {
    &value[..byte_index_at_width(value, width)]
}

fn take_suffix_columns(value: &str, width: usize) -> &str {
    let mut used = 0;
    let mut start = value.len();
    for (index, character) in value.char_indices().rev() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width {
            break;
        }
        used += character_width;
        start = index;
    }
    &value[start..]
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
            let split = byte_index_at_width(remaining, width);
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

fn byte_index_at_width(value: &str, width: usize) -> usize {
    if width == 0 {
        return 0;
    }
    let mut used = 0;
    for (index, character) in value.char_indices() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width {
            return if index == 0 {
                character.len_utf8()
            } else {
                index
            };
        }
        used += character_width;
    }
    value.len()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use unicode_width::UnicodeWidthStr;

    use super::{item_lines, item_row_range, truncate_middle};
    use crate::{NavItem, NavKind, theme};

    fn item(title: &str) -> NavItem {
        NavItem {
            id: "node".to_owned(),
            target_id: "node".to_owned(),
            title: title.to_owned(),
            depth: 1,
            kind: NavKind::Section,
            has_children: false,
            is_last: true,
            parent_id: Some("parent".to_owned()),
        }
    }

    #[test]
    fn inactive_long_titles_keep_both_identifying_ends_on_one_row() {
        let rows = item_lines(
            &item("Options Controlling the Kind of Output"),
            0,
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
        let rows = item_lines(
            &item("Options Controlling the Kind of Output"),
            0,
            true,
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
    fn row_ranges_include_every_continuation_line() {
        let items = vec![item("Options Controlling the Kind of Output")];
        let rows = super::rows(&items, &[0], 0, &HashSet::new(), 18);

        assert_eq!(item_row_range(&rows, 0), Some(0..rows.len()));
        assert!(rows.len() > 1);
    }

    #[test]
    fn expanded_selected_parents_keep_guides_through_continuation_rows() {
        let mut parent = item("A deliberately long expanded parent title");
        parent.depth = 0;
        parent.has_children = true;
        parent.parent_id = None;

        let rows = item_lines(&parent, 0, true, true, 23);
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
        let mut leaf = item("Leaf");
        leaf.is_last = false;

        let row = item_lines(&leaf, 0, false, false, 24)
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
