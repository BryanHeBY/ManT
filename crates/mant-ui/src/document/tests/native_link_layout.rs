//! Actual codec/loader and terminal consume the same optional source receipt.
use super::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};
const SRC: &str = ".Dd November 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd use\n.Sh DESCRIPTION\n.No HEAD\nY\n.No \"\\h'|1n'R\"\n.Lk https://z.test Z U\nEND\n.br\n.No OTHER\n.Lk https://z.test AGAIN\n.Sh NEXT\nDone.\n";

#[test]
fn native_layout_activates_disjoint_members_without_changing_legacy_payload() {
    let legacy = mant_loader::load_roff_bytes(SRC.as_bytes()).unwrap();
    let snapshot = mant_loader::load_roff_bytes_with_link_layout(SRC.as_bytes()).unwrap();
    let bundle = snapshot.content();
    let layout = snapshot.receipts();
    assert_eq!(
        serde_json::to_vec(&mant_protocol::QueryBundle::from(&legacy)).unwrap(),
        serde_json::to_vec(&mant_protocol::QueryBundle::from(bundle)).unwrap(),
    );
    let [receipt] = layout else {
        panic!("one split authored link");
    };
    assert_eq!(receipt.members, [0..1, 2..3]);
    assert_eq!(
        mant_ir::inline_plain_text(&receipt.expected_children),
        "ZYU"
    );
    let original = bundle.document.as_ref().unwrap();
    assert!(matches!(receipt.origin.resolve_link(original),
        Some(Inline::Link { children, .. }) if mant_ir::inline_plain_text(children) == "ZYU"));
    let mut occurrences = Vec::new();
    mant_ir::scan_navigation_scope(
        original,
        mant_ir::ReferenceScope::Document,
        mant_ir::ReferenceScanLimits::default(),
        mant_ir::NavigationScanOptions::default(),
        |event, _| {
            if let mant_ir::NavigationEvent::Link(link) = event
                && link.target == &receipt.target
            {
                occurrences.push(link.location.to_owned().unwrap());
            }
            std::ops::ControlFlow::Continue(())
        },
    );
    assert_eq!(
        occurrences.len(),
        2,
        "two logical source occurrences, not three paint runs"
    );
    assert_eq!(occurrences[0], receipt.origin);
    assert_ne!(occurrences[0], occurrences[1]);
    let opt_in = DocumentView::with_native_links(&snapshot);
    assert_rendered_members(&DocumentView::new(&legacy), &opt_in);
    let mut stale = layout.to_vec();
    stale[0].target = mant_ir::LinkTarget::External {
        uri: "https://different.test".into(),
    };
    assert!(
        crate::document::link_members::LinkMembers::validate(bundle.document.as_ref(), &stale)
            .is_none()
    );
    stale = layout.to_vec();
    stale.push(stale[0].clone());
    assert!(
        crate::document::link_members::LinkMembers::validate(bundle.document.as_ref(), &stale)
            .is_none()
    );
    stale = layout.to_vec();
    stale[0].members = vec![0..2, 1..3];
    assert!(
        crate::document::link_members::LinkMembers::validate(bundle.document.as_ref(), &stale)
            .is_none()
    );
}
fn assert_rendered_members(legacy: &DocumentView, projected: &DocumentView) {
    for width in [28, 40, 78] {
        let rendered = projected.render(width);
        let default = legacy.render(width);
        let [group] = rendered.search("ZYU").try_into().unwrap();
        let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
        let mut cells = Buffer::empty(area);
        Paragraph::new(rendered.text.clone()).render(area, &mut cells);
        let mut default_cells = Buffer::empty(area);
        Paragraph::new(default.text.clone()).render(area, &mut default_cells);

        for (offset, symbol, linked) in [(0, "Z", true), (1, "Y", false), (2, "U", true)] {
            let column = group.start_column + offset;
            assert_eq!(
                cells[(column.try_into().unwrap(), group.row.try_into().unwrap())].symbol(),
                symbol
            );
            let position = (
                u16::try_from(column).unwrap(),
                u16::try_from(group.row).unwrap(),
            );
            assert_eq!(
                cells[position].fg == default_cells[position].fg,
                linked,
                "width {width}: only authored members get link style"
            );
            let found = rendered.link_target_at(group.row, column);
            if linked {
                assert!(
                    matches!(found, Some(LinkTarget::External(uri)) if uri.as_str() == "https://z.test"),
                    "width {width} {symbol}"
                );
            } else {
                assert_eq!(found, None, "width {width} foreign atom {symbol}");
            }
            assert!(
                matches!(default.link_target_at(group.row, column), Some(LinkTarget::External(uri)) if uri.as_str() == "https://z.test")
            );
        }
        let [again] = rendered.search("AGAIN").try_into().unwrap();
        assert!(
            matches!(rendered.link_target_at(again.row, again.start_column), Some(LinkTarget::External(uri)) if uri.as_str() == "https://z.test")
        );
        assert_eq!(
            rendered.links.len(),
            3,
            "three physical runs, two authored occurrences"
        );
        let [from] = rendered.search("HRAZYU").try_into().unwrap();
        let [last] = rendered.search("END").try_into().unwrap();
        let selection = crate::document::RenderedSelection {
            anchor: crate::document::TextPosition {
                row: from.row,
                column: from.start_column,
            },
            focus: crate::document::TextPosition {
                row: last.row,
                column: last.end_column - 1,
            },
        };
        assert_eq!(
            rendered.selected_text(selection),
            if width == 28 {
                "HRAZYU: https://z.test\n   END"
            } else {
                "HRAZYU: https://z.test END"
            },
            "width {width}: exact copy, including the renderer's soft wrap",
        );
        assert_eq!(
            default.selected_text(selection),
            rendered.selected_text(selection)
        );
    }
}

#[test]
fn native_authored_member_crosses_visual_wrap_and_keeps_its_combining_glyph() {
    let source = SRC.replacen(
        ".Lk https://z.test Z U",
        ".Lk https://z.test Z U\\[u0301]abcdefghijklmnopqrstuvwxyzABCDEFGHIJ",
        1,
    );
    let word = "U\u{301}abcdefghijklmnopqrstuvwxyzABCDEFGHIJ";
    let snapshot = mant_loader::load_roff_bytes_with_link_layout(source.as_bytes()).unwrap();
    let [receipt] = snapshot.receipts() else {
        panic!("one long native authored link");
    };
    assert_eq!(receipt.members, [0..1, 2..2 + word.chars().count()]);
    let view = DocumentView::with_native_links(&snapshot);
    let rendered = view.render(28);
    let legacy = DocumentView::new(snapshot.content()).render(28);
    let [match_] = rendered.search(word).try_into().unwrap();
    let last = match_
        .additional_fragments
        .last()
        .expect("the authored word itself wraps");
    assert!(last.row > match_.row);
    let regions = std::iter::once((match_.row, match_.start_column, match_.end_column)).chain(
        match_
            .additional_fragments
            .iter()
            .map(|f| (f.row, f.start_column, f.end_column)),
    );
    let area = Rect::new(0, 0, 28, rendered.row_count.try_into().unwrap());
    let mut cells = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut cells);
    let mut painted = String::new();
    for (row, start, end) in regions {
        for col in start..end {
            assert!(matches!(rendered.link_target_at(row, col),
                Some(LinkTarget::External(uri)) if uri.as_str() == "https://z.test"));
            painted.push_str(cells[(col.try_into().unwrap(), row.try_into().unwrap())].symbol());
        }
    }
    assert_eq!(painted, word);
    let [prefix] = rendered.search("ZY").try_into().unwrap();
    assert_eq!(
        rendered.link_target_at(prefix.row, prefix.start_column + 1),
        None
    );
    let selection = crate::document::RenderedSelection {
        anchor: crate::document::TextPosition {
            row: match_.row,
            column: match_.start_column,
        },
        focus: crate::document::TextPosition {
            row: last.row,
            column: last.end_column - 1,
        },
    };
    let copy = rendered.selected_text(selection);
    assert_eq!(copy, legacy.selected_text(selection));
    assert_eq!(copy.lines().map(str::trim).collect::<String>(), word);

    // The accent occupies its base character's whole glyph. Even an in-range
    // scalar starting at the accent cannot give that glyph a new link owner.
    let mut invalid = snapshot.receipts().to_vec();
    invalid[0].members[1].start += 1;
    assert!(
        crate::document::link_members::LinkMembers::validate(
            snapshot.content().document.as_ref(),
            &invalid,
        )
        .is_none()
    );
}
