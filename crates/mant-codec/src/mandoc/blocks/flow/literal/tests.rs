use super::{CompletedTail, LiteralFlow, append_completed_rows, retire_completed_tail};
use mant_ir::{Inline, LinkTarget};

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.to_owned(),
    }
}

#[test]
fn completed_tail_retirement_visits_each_node_once() {
    // Exact raw no-fill sources with 256/1024/4096 empty rows ran pristine
    // first. term.c::term_vspace() completes each row independently; this
    // structural counter verifies that transfer does not rescan retained
    // identities for every one of those rows. No timing threshold is used.
    for rows in [256_u16, 1024, 4096, u16::MAX] {
        let mut nodes = vec![text("BEFORE"), Inline::line_break()];
        for _ in 0..rows {
            nodes.push(text(""));
            nodes.push(Inline::line_break());
        }
        nodes.extend((0..usize::from(rows)).map(|index| Inline::anchor(format!("target-{index}"))));
        let total_nodes = nodes.len();
        let mut tail = CompletedTail {
            remaining: usize::from(rows),
            active: true,
            visits: 0,
        };
        tail.retire(&mut nodes);
        assert_eq!(tail.visits, total_nodes, "one pass for {rows} rows");
        assert_eq!(tail.remaining, 0);
        assert_eq!(nodes.len(), usize::from(rows) + 2);
        assert_eq!(nodes[0], text("BEFORE"));
        assert_eq!(nodes[1], Inline::line_break());
        for (index, target) in nodes[2..].iter().enumerate() {
            assert_eq!(*target, Inline::anchor(format!("target-{index}")));
        }
    }
}

#[test]
fn completed_tail_retirement_keeps_wrapped_identity_and_authored_spaces() {
    let target = LinkTarget::External {
        uri: "https://example.org".to_owned(),
    };
    let mut nodes = vec![
        text("BEFORE"),
        Inline::line_break(),
        Inline::Strong {
            children: vec![text(""), Inline::line_break()],
        },
        Inline::Link {
            title: None,
            target: target.clone(),
            children: vec![Inline::anchor("authored")],
        },
    ];
    retire_completed_tail(&mut nodes, 1);
    assert_eq!(
        nodes,
        vec![
            text("BEFORE"),
            Inline::line_break(),
            Inline::Strong { children: vec![] },
            Inline::Link {
                title: None,
                target,
                children: vec![Inline::anchor("authored")],
            },
        ]
    );

    // A real authored blank occupies its own row and is not an empty
    // witness. Metadata cannot make the earlier delimiter removable.
    let mut authored = vec![Inline::line_break(), text(" "), Inline::anchor("tail")];
    let expected = authored.clone();
    retire_completed_tail(&mut authored, 1);
    assert_eq!(authored, expected);
    retire_completed_tail(&mut authored, 0);
    assert_eq!(authored, expected);
}

#[test]
fn typed_completed_rows_keep_content_outside_the_spacing_budget() {
    use crate::mandoc::inline::CompletedRowOrigin::{Layout, LiteralText};
    use mant_ir::Block;
    // The corresponding raw/sp/raw and sp/raw/sp sources ran pristine in
    // five profiles first. Only the request rows enter the bounded gap
    // plan; an empty TEXT row remains literal content after either boundary.
    let mut blocks = vec![Block::Preformatted {
        inline_layout: mant_ir::InlineLayout::default(),
        children: vec![text("BEFORE")],
        language: None,
        layout: mant_ir::LayoutHint::default(),
        source: None,
    }];
    append_completed_rows(
        &mut blocks,
        &[LiteralText, Layout, LiteralText],
        mant_ir::LayoutHint::default(),
    );
    assert!(matches!(blocks.as_slice(), [
        Block::Preformatted { children: before, .. },
        Block::VerticalSpace { lines: 1, .. },
        Block::Preformatted { children: after, .. },
    ] if mant_ir::inline_plain_text(before) == "BEFORE\n" && after == &[text("")]));

    let mut owner = LiteralFlow::new();
    owner.nodes = vec![Inline::anchor("authored")];
    let output = owner.take(0.into());
    assert!(
        matches!(output.as_slice(), [Block::Preformatted { children, .. }]
        if children == &[Inline::anchor("authored")])
    );
    assert!(
        !mant_ir::geometry::has_literal_rows(match &output[0] {
            Block::Preformatted { children, .. } => children,
            _ => unreachable!(),
        }),
        "identity alone cannot add a physical prefix row"
    );
}
