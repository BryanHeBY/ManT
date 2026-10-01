use super::{BTreeMap, Inline, split_native_field_passes};
use crate::mandoc::inline::plain_text;
use mant_ir::LinkTarget;

fn marker(name: &str) -> String {
    format!("{}{name}", super::super::INTERNAL_FIELD_WORD)
}

#[test]
fn accepted_owner_receipt_intersects_each_ordered_range_once() {
    use super::super::super::field_buffer::{
        FieldBuffer, FieldCell, FieldWrite, FillTargets, FlushReceipt,
    };

    // The exact 1024/2048/4096/8192-word HANG sources ran pristine CVS
    // ASCII/UTF-8/lint before this test. term_flushln() consumes accepted
    // passes forward (term.c:123-231); ownership may not rescan all old
    // passes for each word. Count the actual production intersections.
    for words in [1_024, 2_048, 4_096, 8_192] {
        let mut buffer = FieldBuffer::default();
        let mut anchors = Vec::new();
        for index in 0..=words {
            let start = buffer.cells().len();
            buffer.begin_word(false, true, false);
            let mut writes = if index == words {
                vec![
                    FieldWrite::Cell(FieldCell::BreakMarker),
                    FieldWrite::UnprojectedBlank,
                    FieldWrite::Cell(FieldCell::Graph {
                        text: 'D',
                        width: 1,
                    }),
                ]
            } else {
                FieldWrite::literal("aa")
            };
            if index < words {
                writes.push(FieldWrite::Cell(FieldCell::BreakMarker));
            }
            buffer.apply_writes(&writes);
            anchors.push(super::super::super::NativeWordAnchor {
                start,
                owner: marker(&index.to_string()),
                content: start,
                projected_device_padding: 0,
                projected_field_prefix: false,
            });
        }
        let targets = FillTargets {
            first: usize::MAX / 2,
            rest: usize::MAX / 2,
            unbounded: true,
        };
        let FlushReceipt::Rejected { passes, .. } = buffer.flush_receipt(targets, false) else {
            panic!("the graphless final marker must reject");
        };
        assert_eq!(passes.len(), words);
        super::OWNER_PASS_INTERSECTIONS.with(|work| work.set(0));
        let accepted = super::accepted_owner_lengths(&buffer, &anchors, &passes);
        assert_eq!(accepted.len(), words + 1);
        assert_eq!(accepted[&marker("0")].scalars, 2);
        assert!(!accepted[&marker(&words.to_string())].native_cells);
        let intersections = super::OWNER_PASS_INTERSECTIONS.with(std::cell::Cell::get);
        assert!(
            intersections <= 4 * (words + 1),
            "{words} owners revisited {intersections} pass intervals"
        );
    }
}

#[test]
fn native_word_receipt_preserves_ordinary_kept_and_tight_boundaries() {
    use super::super::super::field_buffer::{FieldBuffer, FieldCell, FieldWrite};

    // Exact No A No B / Bk -words / No A Ns No B sources ran pristine
    // CVS ASCII/UTF-8/tree/lint first. term_word() writes its incoming
    // NOSPACE/KEEP separator before promoting PREKEEP (term.c:573-586).
    for (tight, kept, cell) in [
        (false, false, Some(FieldCell::BreakableBlank)),
        (false, true, Some(FieldCell::NonBreakingBlank)),
        (true, false, None),
    ] {
        let mut buffer = FieldBuffer::default();
        assert_eq!(buffer.begin_word(true, true, false), 0);
        buffer.apply_writes(&FieldWrite::literal("A"));
        assert_eq!(
            buffer.begin_word(tight, true, kept),
            usize::from(cell.is_some())
        );
        let receipt = buffer.apply_writes(&FieldWrite::literal("B"));
        assert_eq!(receipt.first_content_cell, if tight { 1 } else { 2 });
        assert_eq!(
            buffer.cells().get(1),
            cell.as_ref().or(Some(&FieldCell::Graph {
                text: 'B',
                width: 1
            }))
        );
    }
}

#[test]
fn repeated_physical_flushes_never_revisit_the_committed_head_prefix() {
    // Both exact sources ran on pristine CVS -Tascii/-Tutf8/-Tlint
    // before this test. NODE_LINE executes term_newln() for each native
    // source row (mdoc_term.c:314-318); term.c:233-237 consumes that
    // field. Its earlier output must not be scanned at the next flush.
    // Count actual production visits instead of using wall-clock timing.
    for words in [128, 512] {
        let source = format!(
            ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Bl -hang -width 4n\n.It Xo\n{}.Xc\n.No BodyWord\n.El\n",
            ".No aa\n".repeat(words)
        );
        super::OWNER_NODES_VISITED.with(|work| work.set(0));
        let document = crate::mandoc::parse_plain_manual(
            std::path::Path::new("native-owner-scan-scale.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert!(!document.sections.is_empty());
        let inspected = super::OWNER_NODES_VISITED.with(std::cell::Cell::get);
        assert!(
            inspected <= words * 8,
            "{words} rows revisited {inspected} owner nodes"
        );
    }
}

#[test]
fn an_accepted_hang_mode_flush_retires_the_field_with_its_row_still_open() {
    use crate::mandoc::formatter::AuthorFlow;
    use crate::mandoc::inline::flow::native_field::FieldFlags;
    use crate::mandoc::inline::{AuthorBreakEffect, InlineBuilder};

    // Exact X/nf/Y/fi HANG input ran on pristine CVS first. fi/nf share
    // pre_br()->term_newln() (roff_term.c:45-58,69-78), and the accepted
    // field reset clears lastcol even when HANG retains its device row
    // (term.c:233-253). Check the buffer at that execution checkpoint,
    // not only the final spelling, which could hide a repeated scan.
    let mut builder = InlineBuilder::new();
    builder.inherit_author_execution_with_effect(
        AuthorFlow::default(),
        false,
        AuthorBreakEffect::Field {
            gap_cells: 1,
            body_width_columns: 6,
            field_width_columns: 6,
            flags: FieldFlags::hang(),
        },
    );
    builder.append_text("X");
    assert!(
        !builder
            .execution
            .definition
            .as_ref()
            .unwrap()
            .field_buffer
            .is_empty()
    );
    builder.fill_mode_boundary();
    assert!(
        builder
            .execution
            .definition
            .as_ref()
            .unwrap()
            .field_buffer
            .is_empty()
    );
    assert_eq!(plain_text(&builder.nodes), "X");
    builder.append_text("Y");
    let field = &builder.execution.definition.as_ref().unwrap().field_buffer;
    assert!(field.cells().iter().all(|cell| !matches!(
        cell,
        crate::mandoc::inline::flow::field_buffer::FieldCell::Graph { text: 'X', .. }
    )));
}

#[test]
fn empty_native_flush_closes_only_an_already_occupied_overrun_row() {
    use crate::mandoc::inline::InlineBuilder;

    // Twelve complete short/near-full column Bd sources ran pristine
    // CVS first, lint=0. term_newln(lastcol || viscol) still reaches
    // term_flushln's vbr=0 tail with an empty buffer (term.c:475-480,
    // 143-146,250-253). No cell and no device row remains a no-op.
    for (word, expected_open) in [("LEFT", true), ("LEFT1234567", false)] {
        let mut builder = InlineBuilder::new();
        builder.begin_column_body(12, 0, false);
        builder.append_text(word);
        builder.execute_native_newline();
        assert!(builder.execution.has_open_native_device_row());
        assert!(builder.definition.as_ref().unwrap().field_buffer.is_empty());
        builder.execute_native_newline();
        assert_eq!(
            builder.execution.has_open_native_device_row(),
            expected_open
        );
        let committed = builder.nodes.clone();
        if !expected_open {
            builder.execute_native_newline();
            assert_eq!(builder.nodes, committed, "the closed row cannot end twice");
        }
    }
    let mut empty = InlineBuilder::new();
    empty.begin_column_body(12, 0, false);
    empty.execute_native_newline();
    assert!(empty.nodes.is_empty());
}

#[test]
fn display_post_clears_native_no_fill_without_ending_the_retained_device_row() {
    use crate::mandoc::inline::InlineBuilder;

    // The exact compact literal-column Marker/AFTER sources ran CVS
    // first. termp_bd_post temporarily sets BRNEVER, calls term_newln,
    // then clears BRNEVER irrespective of a NOBREAK open device row
    // (mdoc_term.c:1474-1483).
    let mut builder = InlineBuilder::new();
    builder.begin_column_body(20, 0, false);
    builder.execution.no_fill_word_active = true;
    builder.append_text("Marker");
    builder.finish_display_body(Some(libmandoc_rs::DisplayKind::Literal));
    assert!(!builder.execution.no_fill_word_active);
    assert!(builder.execution.has_open_native_device_row());
    assert!(builder.definition.as_ref().unwrap().field_buffer.is_empty());
}

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.to_owned(),
    }
}

#[test]
fn owner_start_break_consumes_prior_padding_through_semantic_wrappers() {
    // The existing accepted-run segmentation contract consumes ordinary
    // separators at a pass boundary. Owner and style changes do not
    // alter that projection or the retained typed link identity.
    let target = LinkTarget::External {
        uri: "https://example.org".to_owned(),
    };
    let input = [
        Inline::anchor(marker("a")),
        Inline::Strong {
            children: vec![text("A\u{a0}  ")],
        },
        Inline::anchor("authored-target"),
        Inline::anchor(marker("b")),
        Inline::Link {
            target: target.clone(),
            title: Some("label".to_owned()),
            children: vec![Inline::Emphasis {
                children: vec![text("B C")],
            }],
        },
    ];
    let output = split_native_field_passes(&input, &BTreeMap::from([(marker("b"), vec![0])]));
    assert_eq!(plain_text(&output), "A\u{a0}\nB C");
    assert!(matches!(
        output.last(),
        Some(Inline::Link { target: retained, title, .. })
            if retained == &target && title.as_deref() == Some("label")
    ));
    assert!(matches!(
        output.get(2),
        Some(Inline::Anchor { id, .. }) if id.as_str() == "authored-target"
    ));
}

#[test]
fn scalar_cursor_crosses_styles_and_resets_at_each_native_owner() {
    let input = [
        Inline::anchor(marker("unicode")),
        text("𝔸"),
        Inline::Strong {
            children: vec![
                Inline::anchor("authored-target"),
                Inline::Code {
                    value: "β  γ".to_owned(),
                },
            ],
        },
        Inline::anchor(marker("tail")),
        text("D E"),
    ];
    let output =
        split_native_field_passes(&input, &BTreeMap::from([(marker("unicode"), vec![4, 4])]));
    assert_eq!(plain_text(&output), "𝔸β\n\nγD E");
    let Inline::Strong { children } = &output[2] else {
        panic!("code's style wrapper must survive: {output:?}");
    };
    assert!(matches!(children[1], Inline::Code { .. }));
    assert!(matches!(children[4], Inline::Code { .. }));
}

#[test]
fn existing_hard_boundaries_represent_events_without_native_scalars() {
    let input = [
        Inline::anchor(marker("word")),
        text("A"),
        Inline::line_break_indented(3),
        text("B C"),
        Inline::line_break(),
        Inline::line_break(),
        text("D"),
    ];
    let output = split_native_field_passes(&input, &BTreeMap::from([(marker("word"), vec![3, 1])]));
    assert_eq!(plain_text(&output), "A\nB\nC\n\nD");
    assert!(matches!(output[2], Inline::LineBreak { indent_columns: 3 }));
}

#[test]
fn private_text_marker_does_not_advance_the_scalar_cursor() {
    let input = [text(&marker("text")), text("A B")];
    let output = split_native_field_passes(&input, &BTreeMap::from([(marker("text"), vec![2])]));
    assert_eq!(output[0], input[0]);
    assert_eq!(plain_text(&output[1..]), "A\nB");
}

#[test]
fn zero_offset_boundary_does_not_require_a_visible_owner_glyph() {
    let input = [
        Inline::anchor(marker("prefix")),
        text("A  "),
        Inline::anchor(marker("empty")),
        text(""),
        Inline::anchor(marker("suffix")),
        text("B"),
    ];
    let output = split_native_field_passes(&input, &BTreeMap::from([(marker("empty"), vec![0])]));
    assert_eq!(plain_text(&output), "A\nB");
    assert!(
        output.contains(&text("")),
        "keep its row witness: {output:?}"
    );
}

#[test]
fn many_owner_passes_keep_every_accepted_piece_in_source_order() {
    let mut input = Vec::new();
    let mut boundaries = BTreeMap::new();
    for owner in 0..1_024 {
        let owner = marker(&owner.to_string());
        input.push(Inline::anchor(owner.clone()));
        input.push(text("x  y"));
        boundaries.insert(owner, vec![3]);
    }
    let output = split_native_field_passes(&input, &boundaries);
    assert_eq!(plain_text(&output), "x\ny".repeat(1_024));
}
