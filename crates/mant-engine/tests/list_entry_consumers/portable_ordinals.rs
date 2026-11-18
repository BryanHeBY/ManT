//! Real export and readback at the `CommonMark` ordered marker boundary.

use super::{item, query, semantic_read};
use mant_codec::encode::render_markdown;
use mant_ir::{Block, Inline, LayoutHint, ListKind, ResolvedContent};
use mant_loader::{load_markdown_text, load_roff_bytes};
use mant_render::render_excerpt_markdown;

const MAX: u64 = 999_999_999;
const STARTS: [Option<u64>; 9] = [
    None,
    Some(0),
    Some(1),
    Some(MAX - 1),
    Some(MAX),
    Some(MAX + 1),
    Some(u32::MAX as u64),
    Some(u64::MAX - 1),
    Some(u64::MAX),
];

fn list(query: &ResolvedContent) -> (ListKind, &[mant_ir::ListItem]) {
    let document = query.document.as_ref().unwrap();
    let [Block::List { kind, items, .. }] = document.blocks.as_slice() else {
        panic!("wanted one list, found {:?}", document.blocks);
    };
    (*kind, items)
}

fn text(block: &Block) -> String {
    let Block::Paragraph { children, .. } = block else {
        panic!("wanted a paragraph, found {block:?}");
    };
    mant_ir::inline_plain_text(children)
}

fn plain(word: String) -> Block {
    Block::Paragraph {
        children: vec![Inline::Text { value: word }],
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn list_starts_keep_item_owners_across_two_markdown_reads() {
    for start in STARTS {
        for count in 1..=3 {
            for extra in 0..3 {
                let kind = ListKind::Ordered { start };
                let mut input = query(false);
                input.document.as_mut().unwrap().blocks = vec![Block::List {
                    kind,
                    compact: extra == 0,
                    layout: LayoutHint::default(),
                    source: None,
                    items: (0..count)
                        .map(|index| {
                            let mut item = item("name", &format!("ITEM-{index}"), false);
                            if extra > 0 {
                                item.blocks.push(plain(format!("CONTINUATION-{index}")));
                            }
                            if extra > 1 {
                                let children = vec![item_for_child(index)];
                                item.blocks.push(Block::List {
                                    kind: ListKind::Bullet,
                                    compact: true,
                                    layout: LayoutHint::default(),
                                    source: None,
                                    items: children,
                                });
                            }
                            item
                        })
                        .collect(),
                }];
                let wire =
                    serde_json::to_string(&mant_protocol::QueryBundle::from(&input)).unwrap();
                let decoded = serde_json::from_str::<mant_protocol::QueryBundle>(&wire).unwrap();
                let decoded = ResolvedContent::from(decoded);
                assert_eq!(
                    list(&decoded).0,
                    kind,
                    "JSON must retain the original start"
                );
                for _ in 0..2 {
                    let markdown = render_markdown(&input);
                    input = load_markdown_text(&markdown, None).unwrap();
                    let (actual, items) = list(&input);
                    assert_eq!(items.len(), count, "{start:?}/{extra}: {markdown}");
                    let explicit = start.is_some_and(|value| value > MAX);
                    if explicit {
                        assert_eq!(actual, ListKind::Bullet, "{markdown}");
                    } else {
                        assert_eq!(actual.ordinal(0), kind.ordinal(0), "{markdown}");
                    }
                    for (index, item) in items.iter().enumerate() {
                        let payload = format!("name — ITEM-{index}: punctuation | stays.");
                        let label = if explicit {
                            format!("{}. {payload}", kind.ordinal(index).unwrap())
                        } else {
                            payload
                        };
                        assert_eq!(item.blocks.len(), extra + 1, "{markdown}");
                        assert_eq!(text(&item.blocks[0]), label, "{markdown}");
                        if extra > 0 {
                            assert_eq!(text(&item.blocks[1]), format!("CONTINUATION-{index}"));
                        }
                        if extra > 1 {
                            let Block::List { items, .. } = &item.blocks[2] else {
                                panic!("lost child list of item {index}: {markdown}");
                            };
                            assert_eq!(items.len(), 1);
                            assert_eq!(text(&items[0].blocks[0]), format!("CHILD-{index}"));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn explicit_number_label_leaves_a_leading_fence_inside_its_item() {
    let mut input = query(false);
    let Block::List { kind, items, .. } = &mut input.document.as_mut().unwrap().blocks[0] else {
        unreachable!();
    };
    *kind = ListKind::Ordered {
        start: Some(MAX + 1),
    };
    items[0].blocks = vec![Block::Preformatted {
        language: Some("text".into()),
        children: vec![Inline::Text {
            value: "CODE".into(),
        }],
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }];
    for _ in 0..2 {
        let markdown = render_markdown(&input);
        input = load_markdown_text(&markdown, None).unwrap();
        let (kind, items) = list(&input);
        assert_eq!(kind, ListKind::Bullet);
        assert_eq!(items.len(), 2);
        assert_eq!(text(&items[0].blocks[0]), "1000000000.");
        let Block::Preformatted { children, .. } = &items[0].blocks[1] else {
            panic!("fence escaped its original item: {markdown}");
        };
        assert_eq!(mant_ir::inline_plain_text(children), "CODE");
    }
}

fn item_for_child(index: usize) -> mant_ir::ListItem {
    mant_ir::ListItem {
        blocks: vec![plain(format!("CHILD-{index}"))],
        layout: mant_ir::ListItemLayout::default(),
        source: None,
        entry: None,
    }
}

#[test]
fn wide_excerpt_uses_a_semantically_intact_single_bullet_owner() {
    let mut input = query(true);
    let Block::List { kind, .. } = &mut input.document.as_mut().unwrap().blocks[0] else {
        unreachable!();
    };
    *kind = ListKind::Ordered { start: Some(MAX) };
    let excerpt = semantic_read::semantic_excerpt(&input, &["run"]).unwrap();
    let markdown = render_excerpt_markdown(&excerpt);
    let readback = load_markdown_text(&markdown, None).unwrap();
    let lists: Vec<_> = readback
        .document
        .as_ref()
        .unwrap()
        .blocks
        .iter()
        .filter(|block| matches!(block, Block::List { .. }))
        .collect();
    let [Block::List { kind, items, .. }] = lists.as_slice() else {
        panic!("expected the excerpt's single list: {markdown}");
    };
    assert_eq!(*kind, ListKind::Bullet);
    assert_eq!(items.len(), 1);
    assert!(text(&items[0].blocks[0]).starts_with("1000000000. run — SECOND"));
}

#[test]
fn native_ordinals_cross_the_markdown_digit_limit_without_escaping_their_list() {
    const HEADER: &str = ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";
    for start in [MAX - 1, MAX, MAX + 1, u64::from(u32::MAX)] {
        // The exact two labels were read in pinned CVS tree/terminal/HTML.
        // ManT retains the source ordinal in IR and selects Markdown syntax
        // based on what the actual Markdown parser accepts.
        let source = format!(
            "{HEADER}.Bl -tag -width Ds\n.It {start}.\nALPHA\n.It {}.\nBETA\n.El\n",
            start + 1
        );
        let input = load_roff_bytes(source.as_bytes()).unwrap();
        let readback = load_markdown_text(&render_markdown(&input), None).unwrap();
        let [Block::List { kind, items, .. }] = readback.document.as_ref().unwrap().sections[1]
            .blocks
            .as_slice()
        else {
            panic!("lost native list: {source}");
        };
        assert_eq!(items.len(), 2);
        if start <= MAX {
            assert_eq!(kind.ordinal(0), Some(start));
            assert_eq!(text(&items[1].blocks[0]), "BETA");
        } else {
            assert_eq!(*kind, ListKind::Bullet);
            assert_eq!(text(&items[1].blocks[0]), format!("{}. BETA", start + 1));
        }
    }
}

#[test]
fn adjacent_number_fallbacks_keep_separate_list_owners_on_each_read() {
    // CVS termp_bl_pre/post and the native tree retain four separate .Bl
    // scopes. A fallback may change list kind, while preserving those owners.
    let source = concat!(
        ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n",
        ".Sh DESCRIPTION\n.Bl -bullet -compact\n.It\nBEFORE\n.El\n",
        ".Bl -tag -width Ds -compact\n.It 1000000000.\nWIDE-A\n.It 1000000001.\nWIDE-B\n.El\n",
        ".Bl -tag -width Ds -compact\n.It 1000000010.\nWIDE-C\n.It 1000000011.\nWIDE-D\n.El\n",
        ".Bl -bullet -compact\n.It\nAFTER\n.El\n"
    );
    let mut input = load_roff_bytes(source.as_bytes()).unwrap();
    let expected: [&[&str]; 4] = [
        &["BEFORE"],
        &["1000000000. WIDE-A", "1000000001. WIDE-B"],
        &["1000000010. WIDE-C", "1000000011. WIDE-D"],
        &["AFTER"],
    ];
    for _ in 0..2 {
        let markdown = render_markdown(&input);
        input = load_markdown_text(&markdown, None).unwrap();
        let document = input.document.as_ref().unwrap();
        let blocks = &document.sections[1].blocks;
        assert_eq!(blocks.len(), expected.len(), "{markdown}");
        for (block, expected) in blocks.iter().zip(expected) {
            let Block::List { kind, items, .. } = block else {
                panic!("expected the original list owner: {markdown}");
            };
            assert_eq!(*kind, ListKind::Bullet);
            assert_eq!(items.len(), expected.len(), "{markdown}");
            for (item, expected) in items.iter().zip(expected) {
                assert_eq!(text(&item.blocks[0]), *expected, "{markdown}");
            }
        }
    }
}
