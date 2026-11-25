use super::{INTERNAL_FIELD_WORD, Inline, ORDER, apply};
use mant_ir::{LinkTarget, inline_plain_text};

fn link(uri: &str, children: Vec<Inline>) -> Inline {
    Inline::Link {
        target: LinkTarget::External { uri: uri.into() },
        title: None,
        children,
    }
}

fn mark(rank: usize, owner: u64) -> Inline {
    Inline::anchor(format!("{ORDER}6:0:{rank}:2:{owner}"))
}

fn owner(serial: u64) -> Inline {
    Inline::anchor(format!("{INTERNAL_FIELD_WORD}{serial}"))
}

#[test]
fn only_the_moved_glyphs_change_link_positions() {
    // Same receipt as the pinned XF regression. A separate empty target
    // keeps its navigation identity when neither of its cells moved.
    let empty = link("https://empty.test", vec![owner(4)]);
    let mut nodes = vec![
        empty.clone(),
        owner(7),
        mark(1, 7),
        Inline::Text { value: "F".into() },
        link(
            "https://kept.test",
            vec![owner(9), mark(0, 9), Inline::Text { value: "X".into() }],
        ),
    ];
    apply(&mut nodes);
    assert_eq!(inline_plain_text(&nodes), "XF");
    assert_eq!(nodes.first(), Some(&empty));
    let links: Vec<_> = nodes
        .iter()
        .filter_map(|node| match node {
            Inline::Link {
                target: LinkTarget::External { uri },
                children,
                ..
            } => Some((uri.as_str(), inline_plain_text(children))),
            _ => None,
        })
        .collect();
    assert_eq!(
        links,
        [
            ("https://empty.test", String::new()),
            ("https://kept.test", "X".into())
        ]
    );
}

#[test]
fn an_incomplete_paint_group_cannot_publish_a_speculative_copy() {
    let mut nodes = vec![link(
        "https://kept.test",
        vec![
            owner(7),
            Inline::anchor(format!("{ORDER}6:0:0:3:7")),
            Inline::anchor(format!("{ORDER}6:0:2:3:7")),
            Inline::Text { value: "F".into() },
        ],
    )];
    apply(&mut nodes);
    assert_eq!(inline_plain_text(&nodes), "F");
    assert_eq!(nodes.len(), 1);
    assert!(matches!(nodes[0], Inline::Link { .. }));
}

#[test]
fn shared_group_moves_the_glyph_but_not_the_partition_boundary() {
    let mut head = vec![Inline::Strong {
        children: vec![
            Inline::anchor(format!("{ORDER}s6:0:0:3:7")),
            Inline::anchor(format!("{ORDER}s6:0:2:3:7")),
            Inline::Text { value: "J".into() },
        ],
    }];
    let mut body = vec![
        Inline::anchor(format!("{ORDER}s6:0:1:3:9")),
        Inline::Text { value: "B".into() },
    ];
    // BODY's local drain must not run an incomplete group.
    apply(&mut body);
    super::apply_shared(&mut head, &mut body);
    assert_eq!(inline_plain_text(&head), "JB");
    assert_eq!(inline_plain_text(&body), "J");
    assert!(
        body.iter()
            .any(|node| matches!(node, Inline::Strong { .. }))
    );
}

#[test]
fn shared_group_does_not_cross_an_explicit_row_boundary() {
    let mut head = vec![
        Inline::anchor(format!("{ORDER}s6:0:0:3:7")),
        Inline::anchor(format!("{ORDER}s6:0:2:3:7")),
        Inline::Text { value: "J".into() },
        Inline::LineBreak {},
    ];
    let mut body = vec![
        Inline::anchor(format!("{ORDER}s6:0:1:3:9")),
        Inline::Text { value: "B".into() },
    ];
    super::apply_shared(&mut head, &mut body);
    assert_eq!(inline_plain_text(&head), "J\n");
    assert_eq!(inline_plain_text(&body), "B");
}
