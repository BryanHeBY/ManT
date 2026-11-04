use super::*;
use crate::mandoc::inline::plain_text;
use mant_ir::LinkTarget;

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn owner(name: &str) -> String {
    format!("{INTERNAL_FIELD_WORD}{name}")
}

fn reset_work() {
    OWNER_HISTORY_STORES.with(|work| work.set(0));
    OWNER_TEXT_REBUILDS.with(|work| work.set(0));
    OWNER_CHARS_PROJECTED.with(|work| work.set(0));
    ANNOTATION_NODES_PROJECTED.with(|work| work.set(0));
    ROOT_BOUNDARIES_PROJECTED.with(|work| work.set(0));
}

fn manual(children: Vec<Inline>) -> Inline {
    Inline::Link {
        target: LinkTarget::Manual {
            name: "scanf".into(),
            manual_section: Some("3".into()),
        },
        title: Some("authored title".into()),
        children,
    }
}

#[test]
fn leading_device_padding_stays_outside_nested_annotations_and_authored_spaces() {
    // The complete pair-linked-prefix-name source first ran pinned five
    // profiles: Xr's label is scanf(3), and term_field's advance precedes
    // the first graph (term.c:389-427), outside that source annotation.
    // The actual source byte SP below is deliberately part of the label.
    let selected = owner("manual");
    let content = vec![
        Inline::anchor(selected.clone()),
        Inline::anchor("authored-target"),
        Inline::Emphasis {
            children: vec![Inline::Code {
                value: " α(3)".into(),
            }],
        },
    ];
    let annotation = Inline::Strong {
        children: vec![manual(content)],
    };
    let mut nodes = vec![annotation.clone()];
    let _ = project_native_positions(&mut nodes, &[], &[(selected, 0, 2, false)], 0, false);
    assert_eq!(nodes, [text("  "), annotation]);
    assert_eq!(plain_text(&nodes), "   α(3)");
    assert_eq!(mant_ir::inline_scalar_len(&nodes), 7);
}

#[test]
fn internal_device_padding_preserves_the_existing_link_identity_and_hard_rows() {
    // Only the leading generated prefix can cross an annotation boundary.
    // Moving an internal gap would split one Link identity into repeated
    // occurrences. An authored SP or hard row is a real content barrier.
    for (first, scalar, expected) in [(" ", 1, "    α"), ("A", 1, "A\n   α")] {
        let selected = owner("internal");
        let mut children = vec![Inline::anchor(selected.clone()), text(first)];
        if first == "A" {
            children.push(Inline::line_break());
        }
        children.push(text("α"));
        let mut nodes = vec![manual(children)];
        let _ =
            project_native_positions(&mut nodes, &[], &[(selected, scalar, 3, false)], 0, false);
        let [
            Inline::Link {
                target,
                title,
                children,
            },
        ] = nodes.as_slice()
        else {
            panic!("one original Link owner must remain");
        };
        assert_eq!(
            target,
            &LinkTarget::Manual {
                name: "scanf".into(),
                manual_section: Some("3".into()),
            }
        );
        assert_eq!(title.as_deref(), Some("authored title"));
        assert_eq!(plain_text(children), expected);
    }
}

#[test]
fn annotated_padding_projection_visits_each_piece_without_rescanning_text() {
    for words in [64, 256, 1024] {
        let mut nodes = Vec::new();
        let mut padding = Vec::new();
        for index in 0..words {
            let selected = owner(&index.to_string());
            nodes.push(Inline::Strong {
                children: vec![manual(vec![Inline::anchor(selected.clone()), text("α")])],
            });
            padding.push((selected, 0, 1, false));
        }
        reset_work();
        let _ = project_native_positions(&mut nodes, &[], &padding, 0, false);
        assert_eq!(nodes.len(), 2 * words);
        assert_eq!(OWNER_CHARS_PROJECTED.with(std::cell::Cell::get), words);
        assert_eq!(
            ANNOTATION_NODES_PROJECTED.with(std::cell::Cell::get),
            5 * words
        );
        assert_eq!(
            ROOT_BOUNDARIES_PROJECTED.with(std::cell::Cell::get),
            2 * words + 1
        );
    }
}

#[test]
fn late_padding_rebases_a_live_checkpoint_past_the_complete_old_link_and_suffix() {
    // A private builder invariant, not a claim that this exact interleaving
    // is reachable from a particular roff macro. A prior annotation can
    // gain a root padding node while a newer scope holds a root address.
    let selected = owner("old-link");
    let old_link = manual(vec![Inline::anchor(selected.clone()), text("scanf(3)")]);
    let mut builder = super::super::super::InlineBuilder::new();
    builder.nodes = vec![old_link.clone(), text("A")];
    let checkpoint = builder.begin_output_checkpoint();
    let edits = project_native_positions(
        &mut builder.nodes,
        &[],
        &[(selected, 0, 1, false)],
        0,
        false,
    );
    builder.remap_output_projection(&edits);
    builder.nodes.push(text("TAIL"));
    builder.wrap_output_since(checkpoint, |children| vec![Inline::Strong { children }]);
    assert_eq!(
        builder.nodes,
        [
            text(" "),
            old_link,
            text("A"),
            Inline::Strong {
                children: vec![text("TAIL")]
            }
        ],
    );
}

#[test]
fn text_splits_tabs_and_presentation_carriers_keep_live_scope_boundaries() {
    for tabs in [false, true] {
        let selected = owner("split");
        let mut builder = super::super::super::InlineBuilder::new();
        builder.nodes = vec![
            Inline::anchor(selected.clone()),
            Inline::Code {
                value: if tabs { "α\tB" } else { "αB" }.into(),
            },
            text("A"),
        ];
        let checkpoint = builder.begin_output_checkpoint();
        let edits = if tabs {
            project_native_positions_with_tabs(
                &mut builder.nodes,
                &[],
                &[(selected.clone(), 1, 2, false)],
                &[super::super::tab_projection::TabExpansion {
                    owner: selected,
                    scalar: 1,
                    spaces: 4,
                }],
                0,
                false,
            )
        } else {
            project_row_presentations(
                &mut builder.nodes,
                &[(
                    selected,
                    1,
                    mant_ir::RowLayoutHint {
                        indent_columns: 2,
                        ..mant_ir::RowLayoutHint::default()
                    },
                )],
                0,
            )
        };
        builder.remap_output_projection(&edits);
        builder.nodes.push(text("TAIL"));
        builder.wrap_output_since(checkpoint, |children| vec![Inline::Emphasis { children }]);
        assert_eq!(
            builder.nodes.pop(),
            Some(Inline::Emphasis {
                children: vec![text("TAIL")]
            })
        );
        assert_eq!(
            plain_text(&builder.nodes),
            if tabs { "α      BA" } else { "αBA" }
        );
    }
}

#[test]
fn materialized_origins_map_prefix_insertions_and_removed_root_carriers() {
    let selected = owner("origin");
    let mut nodes = vec![
        text("A"),
        Inline::line_break(),
        Inline::anchor(selected.clone()),
        text("B"),
        text("C"),
    ];
    let edits = project_native_positions(&mut nodes, &[(selected, 0, 6, false)], &[], 2, true);
    let mut positions = [0, 1, 2, 3, 4, 5];
    for position in &mut positions {
        edits.remap(position);
    }
    assert_eq!(positions, [0, 2, 3, 4, 5, 6]);
    assert_eq!(plain_text(&nodes), "A\nBC");

    let selected = owner("removed");
    let mut nodes = vec![
        Inline::anchor(selected.clone()),
        Inline::anchor(format!("{INTERNAL_ROW_ORIGIN}2")),
        text("B"),
    ];
    let edits = project_native_positions(&mut nodes, &[], &[(selected, 0, 1, false)], 0, true);
    let mut positions = [0, 1, 2, 3];
    for position in &mut positions {
        edits.remap(position);
    }
    assert_eq!(positions, [1, 3, 3, 4]);
    assert_eq!(plain_text(&nodes), " B");
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
        let _ = project_native_positions(&mut nodes, &[(selected, 0, 0, false)], &[], 0, false);
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
    let _ = project_native_positions(
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
    let _ = project_native_positions(
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
    let _ = project_native_positions(&mut nodes, &[], &[(hidden, 0, 1, true)], 0, false);
    assert_eq!(plain_text(&nodes), "X :");
}

#[test]
fn first_origin_is_layout_while_authored_padding_keeps_its_scalars() {
    // term_field() supplies an accepted owner/scalar receipt. The initial
    // origin used to become Text padding: it must never be guessed from the
    // author's spaces or NBSP. Exact first-origin/space fixtures ran pinned
    // ASCII/UTF-8/HTML/tree/lint before adding these projection assertions.
    for text in ["α", " α", "\u{a0}α"] {
        let selected = owner("first");
        let mut nodes = vec![
            Inline::anchor(selected.clone()),
            Inline::Text { value: text.into() },
        ];
        let _ = project_native_positions(&mut nodes, &[(selected, 0, 6, false)], &[], 0, true);
        super::super::projection::strip_native_projection_markers(&mut nodes);
        let layout = take_inline_layout(&mut nodes);
        assert_eq!(layout.row_indent(0), 6);
        assert_eq!(plain_text(&nodes), text);
        assert_eq!(mant_ir::inline_scalar_len(&nodes), text.chars().count());
        assert_eq!(nodes, [Inline::Text { value: text.into() }]);
    }
}

#[test]
fn origins_and_alternatives_survive_retirement_then_rebase_to_the_next_owner() {
    let mut nodes = vec![Inline::Text { value: "A".into() }];
    push_row_break(&mut nodes, 2);
    nodes.push(Inline::anchor(super::super::INTERNAL_TERM_ALTERNATIVE));
    // A later accepted print replaces the origin after the source boundary
    // was marked. Resolve output indices only after that native retirement.
    assert_eq!(apply_tail_origin(&mut nodes, 6), (Some(true), None));
    nodes.push(Inline::Text { value: "B".into() });
    let breaks = take_definition_term_breaks(&mut nodes);
    assert_eq!(breaks.len(), 1);
    let mut next = nodes.split_off(breaks[0] + 1);
    assert_eq!(nodes.pop(), Some(Inline::line_break()));
    next.insert(0, split_row_origin(&mut nodes).unwrap());
    let first = take_inline_layout(&mut nodes);
    let second = take_inline_layout(&mut next);
    assert!(first.is_empty());
    assert_eq!(second.row_indent(0), 6);
    assert_eq!(plain_text(&nodes), "A");
    assert_eq!(plain_text(&next), "B");
}

#[test]
fn zero_origins_and_replacements_keep_the_delimiters_stable() {
    let mut nodes = vec![text("A"), Inline::line_break(), text("B")];
    assert_eq!(set_last_break_origin(&mut nodes, 0), (true, None));
    assert_eq!(nodes.len(), 3);
    let (found, edit) = set_last_break_origin(&mut nodes, 6);
    assert!(found);
    let edit = edit.unwrap();
    let mut positions = [0, 1, 2, 3];
    for position in &mut positions {
        edit.remap(position);
    }
    assert_eq!(positions, [0, 2, 3, 4]);
    assert_eq!(set_last_break_origin(&mut nodes, 2), (true, None));
    assert_eq!(set_last_break_origin(&mut nodes, 0), (true, None));
    assert_eq!(nodes.len(), 4);
    assert!(take_inline_layout(&mut nodes).is_empty());
    assert_eq!(plain_text(&nodes), "A\nB");
}

#[test]
fn nested_carriers_leave_root_vector_addresses_unchanged() {
    let mut nodes = vec![
        Inline::Strong {
            children: vec![text("A"), Inline::line_break()],
        },
        text("B"),
    ];
    assert_eq!(set_last_break_origin(&mut nodes, 6), (true, None));
    assert_eq!(nodes.len(), 2);
    assert_eq!(set_last_break_origin(&mut nodes, 2), (true, None));
    assert_eq!(take_inline_layout(&mut nodes).row_indent(1), 2);
    assert_eq!(plain_text(&nodes), "A\nB");
}

#[test]
fn semantic_output_checkpoints_survive_earlier_nonzero_carrier_insertions() {
    let mut builder = super::super::super::InlineBuilder::new();
    builder.nodes = vec![text("A"), Inline::line_break()];
    let checkpoint = builder.begin_output_checkpoint();
    builder.nodes.push(text("Z"));
    let (_, edit) = set_last_break_origin(&mut builder.nodes, 6);
    builder.remap_output_positions(edit.unwrap());
    assert!(builder.output_since_has_non_whitespace_glyph(checkpoint.clone()));
    builder.wrap_output_since(checkpoint, |children| vec![Inline::Strong { children }]);
    assert_eq!(take_inline_layout(&mut builder.nodes).row_indent(1), 6);
    assert_eq!(
        builder.nodes,
        [
            text("A"),
            Inline::line_break(),
            Inline::Strong {
                children: vec![text("Z")]
            },
        ]
    );
    assert!(
        builder
            .output_positions
            .as_ref()
            .unwrap()
            .borrow()
            .is_empty()
    );
}

#[test]
fn checkpoint_positions_are_lazy_and_only_live_scopes_remain_registered() {
    fn active(builder: &super::super::super::InlineBuilder) -> usize {
        builder
            .output_positions
            .as_ref()
            .map_or(0, |registry| registry.borrow().len())
    }
    let mut builder = super::super::super::InlineBuilder::new();
    assert!(
        builder.output_positions.is_none(),
        "ordinary output allocates no registry"
    );
    for _ in 0..8192 {
        let checkpoint = builder.begin_output_checkpoint();
        assert_eq!(active(&builder), 1);
        assert!(!builder.output_since_has_non_whitespace_glyph(checkpoint));
        assert_eq!(
            active(&builder),
            0,
            "lookup-only scopes unregister on return"
        );
    }
    let outer = builder.begin_output_checkpoint();
    let retained = outer.clone();
    let inner = builder.begin_output_checkpoint();
    assert_eq!(active(&builder), 2);
    drop(outer);
    assert_eq!(active(&builder), 2, "a clone still owns the outer position");
    drop(inner);
    assert_eq!(active(&builder), 1);
    drop(retained);
    assert_eq!(active(&builder), 0);
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _checkpoint = builder.begin_output_checkpoint();
        assert_eq!(active(&builder), 1);
        panic!("scope unwind");
    }));
    assert!(unwound.is_err());
    assert_eq!(
        active(&builder),
        0,
        "unwind unregisters the last scope handle"
    );
}

#[test]
fn appended_scopes_keep_the_bare_boundary_and_rebase_their_live_start() {
    let mut builder = super::super::super::InlineBuilder::new();
    builder.nodes = vec![text("A"), Inline::line_break()];
    builder.append_scope(
        |builder| {
            assert_eq!(builder.nodes.last(), Some(&Inline::line_break()));
            let (_, edit) = set_last_break_origin(&mut builder.nodes, 6);
            builder.remap_output_positions(edit.unwrap());
            builder.nodes.push(text("Z"));
        },
        |children| vec![Inline::Emphasis { children }],
    );
    assert_eq!(take_inline_layout(&mut builder.nodes).row_indent(1), 6);
    assert_eq!(
        builder.nodes,
        [
            text("A"),
            Inline::line_break(),
            Inline::Emphasis {
                children: vec![text("Z")]
            }
        ]
    );
    assert!(
        builder
            .output_positions
            .as_ref()
            .unwrap()
            .borrow()
            .is_empty()
    );
}
