mod styles;

use std::collections::HashSet;

use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
use unicode_width::UnicodeWidthStr;

use super::{TreePlan, node_row_range, truncate_middle};
use crate::{NavKind, NavNode, theme};

fn node_lines(
    node: &NavNode,
    index: usize,
    selected: bool,
    expanded: bool,
    full: bool,
    width: usize,
) -> Vec<super::NavigationRow> {
    assert_eq!(index, 0);
    super::planned_node_lines(
        &TreePlan::new(std::slice::from_ref(node)),
        0,
        selected,
        expanded,
        full,
        width,
    )
}

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
                &TreePlan::new(&nodes),
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
    for width in [6_u16, 7, 12, 16, 80] {
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
        if width > 6 {
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
                &TreePlan::new(&nodes),
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
            assert!(rows.iter().flat_map(|row| &row.line.spans).any(|span| {
                span.content.contains('↗')
                    && span.style.fg
                        == Some(if selected == 0 {
                            theme::SELECTED_TEXT
                        } else {
                            theme::LINK
                        })
            }));
            assert!(
                rows.iter()
                    .flat_map(|row| &row.line.spans)
                    .filter(|span| span.content.contains('↗'))
                    .all(|span| span
                        .style
                        .add_modifier
                        .contains(ratatui::style::Modifier::UNDERLINED))
            );
            if selected == usize::MAX {
                assert_eq!(rows.len(), 1);
                assert!(
                    rows[0].line.spans.iter().any(|span| span.style.fg
                        == Some(theme::entry_color(mant_ir::EntryKind::Command)))
                );
            } else {
                let expected = TreePlan::new(&nodes)
                    .prefixes(0, true, false, width)
                    .continuation;
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
        owner.parent_id = Some(format!("ancestor-{}", depth - 1));
        let mut nodes = Vec::new();
        for level in 0..depth {
            let mut ancestor = node("Ancestor");
            ancestor.id = format!("ancestor-{level}");
            ancestor.depth = level;
            ancestor.parent_id = level
                .checked_sub(1)
                .map(|parent| format!("ancestor-{parent}"));
            ancestor.has_children = true;
            nodes.push(ancestor);
        }
        nodes.push(owner);
        let plan = TreePlan::new(&nodes);
        for width in [0, 1, 4, 8] {
            let ordinary_rows = super::planned_node_lines(&plan, depth, true, true, true, width);
            assert!(ordinary_rows.iter().all(|row| row.line.width() <= width));
            for (selected, full) in [(depth, false), (usize::MAX, true), (usize::MAX, false)] {
                let rows = super::rows_with_references(
                    &plan,
                    &[depth],
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
        depth: 0,
        kind: NavKind::Section,
        has_children: false,
        is_last: true,
        parent_id: None,
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
    assert_eq!(rows[0].line.to_string(), "   · Options...e Kind of Output");
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
        .map(|row| row.line.spans[1].content.as_ref())
        .collect::<Vec<_>>()
        .join(" ");

    assert_eq!(rows.len(), 2);
    assert_eq!(visible_title, "Options Controlling the Kind of Output");
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
    assert!(text[0].starts_with(" › · "));
    assert!(text[1].starts_with("     "));
    assert!(!text[1].contains('│'));
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
    assert!(text[0].starts_with(" › ▾ "));
    assert!(text[1].starts_with("   │ "));
    assert_eq!(rows[0].line.spans[0].width(), rows[1].line.spans[0].width());
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
    leaf.depth = 1;
    leaf.parent_id = Some("parent".to_owned());
    leaf.is_last = false;
    let mut parent = node("Parent");
    parent.id = "parent".to_owned();
    parent.has_children = true;
    let nodes = [parent, leaf];
    let row = super::planned_node_lines(&TreePlan::new(&nodes), 1, false, false, false, 24)
        .remove(0)
        .line
        .to_string();

    // Final topology, not the deliberately stale is_last flag, wins.
    assert!(row.starts_with("   ╰─· Leaf"));
}

#[test]
fn middle_truncation_is_terminal_column_aware() {
    let truncated = truncate_middle("编译器选项与输出格式", 10);
    assert!(truncated.width() <= 10);
    assert!(truncated.contains("..."));
}
