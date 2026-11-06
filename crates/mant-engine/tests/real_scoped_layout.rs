//! Real manuals protect selected origins, physical headers and Pp distance.

use mant_ir::Block;

#[path = "support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/scoped_layout_real.rs"]
mod real;

#[test]
fn netbsd_printf_sibling_heads_retain_the_same_finite_origin() {
    // Exact complete source ran all five pinned profiles before these facts.
    // print_mdoc_node restores offset at each node return; It uses width5+2
    // for the outer BODY and the same source origin for both inner HEADs.
    let content =
        json_roundtrip::json_string_roundtrip(&real::load("scoped-layout/netbsd-printf.1"));
    let rendered = mant_render::render_query_man(&content);
    for (line, label, phrase) in [
        (184, "#", "A ‘#’ character specifying"),
        (229, "-", "A minus sign which specifies"),
    ] {
        let item = real::definition_at(&content, line);
        assert_eq!(mant_ir::inline_plain_text(&item.terms[0]), label);
        assert_eq!(item.terms[0].inline_layout.row_layout(0).indent_columns, 0);
        let row = rendered.lines().find(|row| row.contains(phrase)).unwrap();
        assert!(
            row.starts_with(&format!("       {label}           {phrase}")),
            "{row:?}"
        );
    }
    let mut current = content;
    for _ in 0..2 {
        current = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_markdown_text(&mant_codec::encode::render_markdown(&current), None)
                .unwrap(),
        );
        let expected =
            "- A minus sign which specifies left adjustment of the output in the indicated field;";
        let owners = real::blocks(&current)
            .into_iter()
            .filter(|block| {
                matches!(block, Block::Paragraph { children, .. }
                if mant_ir::inline_plain_text(children) == expected)
            })
            .count();
        assert_eq!(
            owners, 1,
            "the portable separated word seam keeps one original BODY owner"
        );
    }
}

#[test]
fn openbsd_bpf_literal_offset_and_executed_tabs_are_projected_once() {
    // termp_bd_pre adds its six-column local offset to the original BODY's
    // eight columns. The pristine literal starts at absolute19/common5=14.
    let content = json_roundtrip::json_string_roundtrip(&real::load("scoped-layout/openbsd-bpf.4"));
    let owner = real::definition_at(&content, 135);
    let Block::Preformatted { children, .. } = &owner.description[1] else {
        panic!("original literal owner");
    };
    assert_eq!(mant_ir::inline_plain_text(children), real::BPF_LITERAL);
    let rendered = mant_render::render_query_man(&content);
    let rows = rendered.lines().collect::<Vec<_>>();
    let first = rows
        .iter()
        .position(|row| row.ends_with("struct bpf_dltlist {"))
        .unwrap();
    assert_eq!(
        &rows[first..first + 4],
        [
            "              struct bpf_dltlist {",
            "                      u_int bfl_len;",
            "                      u_int *bfl_list;",
            "              };",
        ]
    );
    let mut current = content;
    for _ in 0..2 {
        current =
            mant_loader::load_markdown_text(&mant_codec::encode::render_markdown(&current), None)
                .unwrap();
        let code = real::blocks(&current)
            .into_iter()
            .filter_map(|block| match block {
                Block::Preformatted { children, .. } => Some(mant_ir::inline_plain_text(children)),
                _ => None,
            })
            .find(|text| text.starts_with("struct bpf_dltlist {"))
            .unwrap();
        assert_eq!(
            code,
            real::BPF_LITERAL,
            "executed literal content, not native origin padding"
        );
    }
}

#[test]
fn illumos_bhyve_all_authored_header_cells_share_the_original_row() {
    // termp_it_pre reinstalls NOBREAK before each non-final column BODY;
    // termp_it_post flushes its field rather than introducing a hard row.
    let content =
        json_roundtrip::json_string_roundtrip(&real::load("scoped-layout/illumos-bhyve.8"));
    let table = real::blocks(&content).into_iter().find(|block| match block {
        Block::Table { rows, .. } => rows[0].cells[0].blocks.first().is_some_and(|block| {
            matches!(block, Block::Paragraph { children, .. } if mant_ir::inline_plain_text(children) == "Backend")
        }),
        _ => false,
    }).unwrap();
    let Block::Table { rows, .. } = table else {
        unreachable!()
    };
    assert_eq!(rows[0].cells.len(), 3);
    assert!(rows[0].cells.iter().all(|cell| !cell.break_after));
    let rendered = mant_render::render_query_man(&content);
    assert!(
        rendered
            .lines()
            .any(|row| row == "            Backend    Path                   Description")
    );
    let mut current = content;
    for _ in 0..2 {
        current =
            mant_loader::load_markdown_text(&mant_codec::encode::render_markdown(&current), None)
                .unwrap();
        assert!(real::blocks(&current).into_iter().any(|block| {
            matches!(block, Block::Preformatted { children, .. }
                if mant_ir::inline_plain_text(children).lines().next() == Some("Backend | Path | Description"))
        }));
    }
}

#[test]
fn tmux_nested_enum_post_and_following_pp_emit_one_empty_row() {
    // Complete tmux source1416–1422 was checked in the five pristine profiles:
    // It post closes LAST; Pp contributes exactly one additional empty row.
    let mut content =
        json_roundtrip::json_string_roundtrip(&real::load("source-adjacency/arch-tmux.1.gz"));
    for round in 0..=2 {
        let rendered = mant_render::render_query_man(&content);
        let rows = rendered.lines().collect::<Vec<_>>();
        let last = rows
            .iter()
            .position(|row| row.ends_with(real::TMUX_LAST))
            .unwrap();
        let after = rows
            .iter()
            .position(|row| row.ends_with(real::TMUX_AFTER))
            .unwrap();
        assert_eq!(after, last + 2, "round{round}: one Pp empty row");
        assert_eq!(rows[last + 1], "");
        if round == 0 {
            assert_eq!(rows[after], format!("        {}", real::TMUX_AFTER));
        }
        if round < 2 {
            content = mant_loader::load_markdown_text(
                &mant_codec::encode::render_markdown(&content),
                None,
            )
            .unwrap();
        }
    }
}
