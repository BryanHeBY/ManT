use super::*;
use crate::mandoc::inline::plain_text;
use mant_ir::LinkTarget;

fn owner(name: &str) -> String {
    format!("{INTERNAL_FIELD_WORD}{name}")
}

fn reset_work() {
    OWNER_HISTORY_STORES.with(|work| work.set(0));
    OWNER_TEXT_REBUILDS.with(|work| work.set(0));
    OWNER_CHARS_PROJECTED.with(|work| work.set(0));
}

#[test]
fn only_words_with_receipt_positions_build_cursor_history_and_text() {
    // Exact 1k/2k/4k/8k plain and br HEAD sources ran five pristine
    // profiles before this counter. Native term_field()397-434 establishes
    // positions; projecting one selected origin cannot rebuild every other
    // formatter word's unchanged IR or remember its scalar history.
    for words in [1024, 2048, 4096, 8192] {
        let selected = owner("selected");
        let mut nodes = vec![
            Inline::anchor(selected.clone()),
            Inline::Text {
                value: "aa".to_owned(),
            },
        ];
        let mut unchanged = Vec::new();
        for index in 1..words {
            nodes.push(Inline::anchor(owner(&index.to_string())));
            let value = "aa".to_owned();
            unchanged.push(value.as_ptr());
            nodes.push(if index % 2 == 0 {
                Inline::Text { value }
            } else {
                Inline::Code { value }
            });
        }
        reset_work();
        project_native_positions(&mut nodes, &[(selected, 0, 0, false)], &[], 0, false);
        assert_eq!(OWNER_HISTORY_STORES.with(std::cell::Cell::get), 1);
        assert_eq!(OWNER_TEXT_REBUILDS.with(std::cell::Cell::get), 1);
        assert_eq!(OWNER_CHARS_PROJECTED.with(std::cell::Cell::get), 2);
        let pointers: Vec<_> = nodes
            .iter()
            .filter_map(|node| match node {
                Inline::Text { value } | Inline::Code { value } => Some(value.as_ptr()),
                _ => None,
            })
            .skip(1)
            .collect();
        assert_eq!(pointers, unchanged, "unselected text must move unchanged");
    }
}

#[test]
fn selected_words_resume_scalar_positions_across_unselected_styles_and_links() {
    // The exact No/Em/Lk zero-advance sources ran pristine first.
    // term_field()389-444 prints one ordered native interval; semantic
    // wrappers cannot reset an accepted owner's scalar cursor. Unicode
    // positions count scalars, independently of UTF-8 byte/cell width.
    let selected = owner("selected");
    let mut nodes = vec![
        Inline::anchor(selected.clone()),
        Inline::Text {
            value: "α".to_owned(),
        },
        Inline::anchor(owner("unselected")),
        Inline::Strong {
            children: vec![Inline::Text {
                value: "UNSELECTED".to_owned(),
            }],
        },
        Inline::Link {
            target: LinkTarget::External {
                uri: "https://example.org".to_owned(),
            },
            title: None,
            children: vec![
                Inline::anchor(selected.clone()),
                Inline::Emphasis {
                    children: vec![Inline::Code {
                        value: "中B".to_owned(),
                    }],
                },
                Inline::anchor(owner("another-unselected")),
                Inline::Text {
                    value: "TAIL".to_owned(),
                },
            ],
        },
    ];
    reset_work();
    project_native_positions(
        &mut nodes,
        &[(selected.clone(), 1, 6, false)],
        &[(selected, 2, 2, false)],
        0,
        false,
    );
    assert_eq!(plain_text(&nodes), "αUNSELECTED中  BTAIL");
    assert_eq!(OWNER_HISTORY_STORES.with(std::cell::Cell::get), 2);
    assert_eq!(OWNER_TEXT_REBUILDS.with(std::cell::Cell::get), 2);
    assert_eq!(OWNER_CHARS_PROJECTED.with(std::cell::Cell::get), 3);
    let Inline::Link { children, .. } = &nodes[4] else {
        panic!("link identity and extent must remain intact");
    };
    let Inline::Emphasis { children } = &children[1] else {
        panic!("style ownership must remain intact");
    };
    assert_eq!(super::super::native_row_origin(&children[0]), Some(6));
    assert!(matches!(&children[1], Inline::Code { value } if value == "中"));
    assert!(matches!(&children[2], Inline::Text { value } if value == "  "));
    assert!(matches!(&children[3], Inline::Code { value } if value == "B"));
}

#[test]
fn a_field_prefix_position_never_creates_a_content_word_cursor() {
    // Accepted prefix origin and native word content are independent:
    // term_field()397-434 writes padding before the graph, and the marker
    // must place that padding without charging it to accepted scalars.
    let mut nodes = vec![
        Inline::anchor(format!("{INTERNAL_FIELD_PREFIX}source")),
        Inline::Text {
            value: "   ".to_owned(),
        },
        Inline::anchor(owner("source")),
        Inline::Text {
            value: "BODY".to_owned(),
        },
    ];
    reset_work();
    project_native_positions(
        &mut nodes,
        &[(format!("{INTERNAL_FIELD_PREFIX}source"), 0, 6, false)],
        &[],
        0,
        false,
    );
    assert_eq!(super::super::native_row_origin(&nodes[0]), Some(6));
    assert_eq!(plain_text(&nodes), "   BODY");
    assert_eq!(OWNER_HISTORY_STORES.with(std::cell::Cell::get), 0);
    assert_eq!(OWNER_TEXT_REBUILDS.with(std::cell::Cell::get), 0);
    assert_eq!(OWNER_CHARS_PROJECTED.with(std::cell::Cell::get), 0);
}

#[test]
fn overstruck_native_graph_keeps_its_padding_before_the_replacement_owner() {
    // Every TAG/HANG Lk \zX source ran pristine first. encode1() writes X
    // before BACKBEFORE covers it with Lk's ':' (term.c:901-927;
    // mdoc_term.c::termp_lk_pre()). term_field() printed the earlier word's
    // padding even though the accepted X has no surviving IR scalar.
    let hidden = owner("hidden");
    let replacement = owner("replacement");
    let mut nodes = vec![
        Inline::Text {
            value: "X".to_owned(),
        },
        Inline::Emphasis {
            children: vec![Inline::anchor(hidden.clone())],
        },
        Inline::anchor(replacement),
        Inline::Text {
            value: ":".to_owned(),
        },
    ];
    project_native_positions(&mut nodes, &[], &[(hidden, 0, 1, true)], 0, false);
    assert_eq!(plain_text(&nodes), "X :");
}
