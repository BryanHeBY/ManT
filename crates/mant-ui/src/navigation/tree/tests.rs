use std::collections::{HashMap, HashSet};

use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
use unicode_width::UnicodeWidthStr;

use super::{Branch, TreePlan};
use crate::{NavKind, NavNode, navigation, theme};

fn node(id: &str, parent: Option<&str>, depth: usize, kind: NavKind, children: bool) -> NavNode {
    NavNode {
        id: id.into(),
        target_id: id.into(),
        title: id.into(),
        full_title: None,
        depth,
        kind,
        has_children: children,
        is_last: false,
        parent_id: parent.map(str::to_owned),
    }
}

fn mixed_forest() -> Vec<NavNode> {
    let mut nodes = vec![
        node("ROOT", None, 0, NavKind::Section, true),
        node("LEAF", Some("ROOT"), 1, NavKind::Section, false),
        node("GROUP", Some("ROOT"), 1, NavKind::EntryGroup, true),
        node(
            "ENTRY",
            Some("GROUP"),
            2,
            NavKind::Entry(mant_ir::EntryKind::Term),
            false,
        ),
        node("REFS", Some("ROOT"), 1, NavKind::ReferenceGroup, true),
        node("LINK", Some("REFS"), 2, NavKind::Reference, false),
        node("NEXT", None, 0, NavKind::Section, false),
    ];
    for node in &mut nodes {
        node.is_last = true;
    }
    nodes
}

#[test]
fn final_siblings_not_expandability_or_stale_flags_determine_guides() {
    let nodes = mixed_forest();
    let plan = TreePlan::new(&nodes);
    assert!(plan.branches[0].next_sibling);
    assert!(plan.branches[1].next_sibling);
    assert!(plan.branches[2].next_sibling);
    assert!(!plan.branches[4].next_sibling);
    for expanded in [false, true] {
        let p = plan.prefixes(4, false, expanded, 80);
        assert!(p.first.starts_with("   ╰─"));
        assert_eq!(p.first.width(), p.continuation.width());
        assert!(
            plan.prefixes(3, false, expanded, 80)
                .first
                .starts_with("   │ ╰─")
        );
        assert!(
            plan.prefixes(5, false, expanded, 80)
                .first
                .starts_with("     ╰─")
        );
        assert_eq!(plan.prefixes(0, false, expanded, 80).first.width(), 5);
        assert_eq!(plan.prefixes(6, false, expanded, 80).first.width(), 5);
        for index in [1, 2, 4] {
            assert_eq!(plan.prefixes(index, false, expanded, 80).first.width(), 7);
        }
    }
}

#[test]
fn mixed_tree_guides_reach_the_expected_terminal_cells() {
    let nodes = mixed_forest();
    let plan = TreePlan::new(&nodes);
    let rows = navigation::rows_with_references(
        &plan,
        &(0..nodes.len()).collect::<Vec<_>>(),
        usize::MAX,
        &HashSet::from(["ROOT".into(), "GROUP".into(), "REFS".into()]),
        false,
        40,
        &HashMap::new(),
    );
    let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 7));
    let expected = [
        "   ▾ ROOT",
        "   ├─· LEAF",
        "   ├─▾ GROUP",
        "   │ ╰─◇ ENTRY",
        "   ╰─▾ REFS",
        "     ╰─· LINK",
        "   · NEXT",
    ];
    assert_eq!(
        rows.len(),
        expected.len(),
        "every logical row must be rendered"
    );
    for (index, (row, expected)) in rows.iter().zip(expected).enumerate() {
        let y = u16::try_from(index).unwrap();
        row.line.clone().render(Rect::new(0, y, 40, 1), &mut buffer);
        for (x, symbol) in expected.chars().enumerate() {
            assert_eq!(
                buffer[(u16::try_from(x).unwrap(), y)].symbol(),
                symbol.to_string()
            );
        }
        assert_eq!(row.node_index, index);
    }
}

#[test]
fn expanded_parent_continuations_keep_branch_and_child_slots_in_cells() {
    for selected in [2, 4] {
        let mut wrapped = mixed_forest();
        wrapped[selected].title = "ABCDEFGHIJKLMNOPQRSTUVWXYZ".into();
        let plan = TreePlan::new(&wrapped);
        let rows = navigation::rows_with_references(
            &plan,
            &[selected],
            selected,
            &HashSet::from(["GROUP".into(), "REFS".into()]),
            false,
            14,
            &HashMap::new(),
        );
        assert!(rows.len() > 1);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 14, u16::try_from(rows.len()).unwrap()));
        for (index, row) in rows.iter().enumerate() {
            let y = u16::try_from(index).unwrap();
            row.line.clone().render(Rect::new(0, y, 14, 1), &mut buffer);
            let prefix = match (selected, index) {
                (2, 0) => " › ├─▾ ",
                (4, 0) => " › ╰─▾ ",
                (2, _) => "   │ │ ",
                _ => "     │ ",
            };
            for (x, symbol) in prefix.chars().enumerate() {
                assert_eq!(
                    buffer[(u16::try_from(x).unwrap(), y)].symbol(),
                    symbol.to_string()
                );
            }
            assert!(
                buffer[(7, y)]
                    .symbol()
                    .chars()
                    .all(|c| c.is_ascii_uppercase())
            );
            assert_eq!(row.node_index, selected);
        }
    }
}

#[test]
fn overview_and_synthetic_roots_use_only_their_actual_siblings() {
    for sections in [false, true] {
        for references in [false, true] {
            for unresolved in [false, true] {
                for limited in [false, true] {
                    let mut nodes = vec![
                        node("TLDR", None, 0, NavKind::Tldr, false),
                        node("OVERVIEW", None, 0, NavKind::Root, true),
                        node("ENTRIES", Some("OVERVIEW"), 1, NavKind::EntryGroup, true),
                        node(
                            "VALUE",
                            Some("ENTRIES"),
                            2,
                            NavKind::Entry(mant_ir::EntryKind::Term),
                            false,
                        ),
                    ];
                    if references {
                        nodes.push(node(
                            "REFS",
                            Some("OVERVIEW"),
                            1,
                            NavKind::ReferenceGroup,
                            true,
                        ));
                    }
                    if sections {
                        nodes.push(node("SECTION", None, 0, NavKind::Section, false));
                    }
                    if unresolved {
                        nodes.push(node("UNRESOLVED", None, 0, NavKind::ReferenceGroup, true));
                    }
                    if limited {
                        nodes.push(node("LIMITED", None, 0, NavKind::ReferenceNotice, false));
                    }
                    // Deliberately contradictory historical flags must be ignored.
                    for node in &mut nodes {
                        node.is_last = true;
                    }
                    let plan = TreePlan::new(&nodes);
                    assert_eq!(plan.branches[2].next_sibling, references);
                    assert_eq!(plan.branches[2].parent, Some(1));
                    let roots: Vec<_> = nodes
                        .iter()
                        .enumerate()
                        .filter(|(_, n)| n.parent_id.is_none())
                        .map(|(i, _)| i)
                        .collect();
                    for (position, &index) in roots.iter().enumerate() {
                        assert_eq!(
                            plan.branches[index].next_sibling,
                            position + 1 < roots.len()
                        );
                    }
                    assert_eq!(
                        plan.prefixes(3, false, false, 80)
                            .first
                            .starts_with("   │ "),
                        references
                    );
                }
            }
        }
    }
}

#[test]
fn malformed_parent_relationships_do_not_invent_another_owner() {
    let nodes = [
        node("A", None, 0, NavKind::Root, true),
        node("SKIP", Some("A"), 2, NavKind::Section, false),
        node("B", None, 0, NavKind::Section, false),
        node("CLOSED", Some("A"), 1, NavKind::Section, false),
        node("ORPHAN", None, 2, NavKind::Section, false),
    ];
    let plan = TreePlan::new(&nodes);
    for index in [1, 3, 4] {
        assert_eq!(plan.branches[index].parent, None);
        assert!(!plan.branches[index].next_sibling);
    }
    assert!(plan.branches[0].next_sibling);
    assert!(!plan.branches[2].next_sibling);
    assert_eq!(plan.nodes, nodes);
}

#[test]
fn long_titles_and_badges_share_actual_cell_origins_and_styles() {
    for children in [false, true] {
        let mut owner = node("OWNER", None, 0, NavKind::Section, children);
        owner.title = "界Cafe\u{301}👩‍💻界Cafe\u{301}👩‍💻".into();
        let nodes = [owner];
        let plan = TreePlan::new(&nodes);
        for badge in [false, true] {
            let badges = if badge {
                HashMap::from([("OWNER".into(), "↗ 日本e\u{301}👩‍💻".into())])
            } else {
                HashMap::new()
            };
            for width in [0_u16, 1, 2, 4, 6, 7, 12, 20, 80] {
                for (selected, full) in [(0, false), (usize::MAX, true)] {
                    let rows = navigation::rows_with_references(
                        &plan,
                        &[0],
                        selected,
                        &HashSet::from(["OWNER".into()]),
                        full,
                        width.into(),
                        &badges,
                    );
                    let mut buffer =
                        Buffer::empty(Rect::new(0, 0, width, u16::try_from(rows.len()).unwrap()));
                    let prefix_width = plan
                        .prefixes(0, selected == 0, true, width.into())
                        .first
                        .width();
                    for (y, row) in rows.iter().enumerate() {
                        assert_eq!(row.node_index, 0);
                        assert!(row.line.width() <= usize::from(width));
                        row.line.clone().render(
                            Rect::new(0, u16::try_from(y).unwrap(), width, 1),
                            &mut buffer,
                        );
                        if width > 0 {
                            assert_eq!(row.line.spans[0].width(), prefix_width);
                            // The first label run starts immediately after the
                            // identical prefix slot on every physical row.
                            let first = row.line.spans[1].content.as_ref();
                            let symbol =
                                mant_render::cells::graphemes(first).next().unwrap().text();
                            assert_eq!(
                                buffer[(
                                    u16::try_from(prefix_width).unwrap(),
                                    u16::try_from(y).unwrap()
                                )]
                                    .symbol(),
                                symbol
                            );
                        }
                    }
                    let mut trailing = 0;
                    for cell in &buffer.content {
                        if trailing > 0 {
                            // Ratatui resets covered cells of wide graphemes;
                            // the style lives on the leading terminal cell.
                            assert_eq!(cell, &ratatui::buffer::Cell::default());
                            trailing -= 1;
                            continue;
                        }
                        if cell.symbol().contains('\u{301}') {
                            assert_eq!(cell.symbol(), "e\u{301}");
                        }
                        if cell.symbol().contains(['👩', '💻', '\u{200d}']) {
                            assert_eq!(cell.symbol(), "👩‍💻");
                        }
                        if selected == 0 {
                            assert_eq!(cell.bg, theme::SELECTED);
                        }
                        trailing = cell.symbol().width().saturating_sub(1);
                    }
                }
            }
        }
    }
}

#[test]
fn deep_and_wide_plans_keep_fixed_metadata_and_width_bounded_ancestor_work() {
    assert!(std::mem::size_of::<Branch>() <= 3 * std::mem::size_of::<usize>());
    let mut chain = Vec::new();
    for depth in 0_usize..10_000 {
        chain.push(node(
            &format!("n{depth}"),
            depth.checked_sub(1).map(|p| format!("n{p}")).as_deref(),
            depth,
            NavKind::Section,
            true,
        ));
    }
    let plan = TreePlan::new(&chain);
    assert_eq!(plan.branches.len(), chain.len());
    for (index, branch) in plan.branches.iter().enumerate() {
        assert_eq!(branch.parent, index.checked_sub(1));
        assert!(!branch.next_sibling);
    }
    for width in [1, 8, 80] {
        let mut visits = 0;
        for index in [0, 1, 100, 9999] {
            let prefix = plan.prefixes(index, false, true, width);
            assert!(prefix.first.width() < width);
            assert_eq!(prefix.first.width(), prefix.continuation.width());
            assert!(prefix.ancestor_visits <= width / 2);
            visits += prefix.ancestor_visits;
        }
        assert!(visits <= 4 * (width / 2));
        let rows = navigation::rows_with_references(
            &plan,
            &[9999],
            9999,
            &HashSet::new(),
            true,
            width,
            &HashMap::new(),
        );
        assert!(
            rows.iter()
                .all(|row| row.node_index == 9999 && row.line.width() <= width)
        );
    }
    let mut wide = vec![node("root", None, 0, NavKind::Section, true)];
    for index in 0..20_000 {
        wide.push(node(
            &format!("e{index}"),
            Some("root"),
            1,
            NavKind::Entry(mant_ir::EntryKind::Term),
            false,
        ));
    }
    let plan = TreePlan::new(&wide);
    assert_eq!(plan.branches.len(), 20_001);
    for index in 1..wide.len() {
        assert_eq!(plan.branches[index].parent, Some(0));
        assert_eq!(plan.branches[index].next_sibling, index + 1 < wide.len());
        for width in [8, 80] {
            assert_eq!(plan.prefixes(index, false, false, width).ancestor_visits, 0);
        }
    }
    for width in [8, 80] {
        let rows = navigation::rows_with_references(
            &plan,
            &[1, 10_000, 20_000],
            usize::MAX,
            &HashSet::new(),
            false,
            width,
            &HashMap::new(),
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter().map(|row| row.node_index).collect::<Vec<_>>(),
            [1, 10_000, 20_000]
        );
        assert!(rows.iter().all(|row| row.line.width() <= width));
    }
}
