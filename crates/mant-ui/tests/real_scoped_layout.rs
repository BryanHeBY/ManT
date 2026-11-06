//! Full real manuals paint selected source-bound rows without sentinel origins.

use mant_ir::Block;
use mant_ui::DocumentView;

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "../../mant-engine/tests/support/scoped_layout_real.rs"]
mod real;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;

#[test]
fn printf_sibling_term_and_body_cells_keep_the_original_nested_origins() {
    // Pristine complete printf source184/229: common5 + outer BODY7.
    // mdoc_term.c print_mdoc_node restores offset after each inner It.
    let content =
        json_roundtrip::json_string_roundtrip(&real::load("scoped-layout/netbsd-printf.1"));
    let rendered = DocumentView::new(&content).render(78);
    let buffer = terminal_cells::paint(&rendered, 78);
    for (line, label) in [(184, "#"), (229, "-")] {
        let item = real::definition_at(&content, line);
        let owner = item.entry.as_ref().unwrap();
        let row = rendered.anchor_row(owner.id.as_str()).unwrap();
        for column in 0..10 {
            assert_eq!(buffer[(column, row.try_into().unwrap())].symbol(), " ");
        }
        assert_eq!(buffer[(10, row.try_into().unwrap())].symbol(), label);
        assert_eq!(buffer[(22, row.try_into().unwrap())].symbol(), "A");
    }
}

#[test]
fn bpf_literal_cells_apply_the_scoped_offset_and_authored_tabs_once() {
    // The exact pristine source's literal has reading origin14 and each TAB
    // advances another8 cells (termp_bd_pre; no extra output-owner origin).
    let content = json_roundtrip::json_string_roundtrip(&real::load("scoped-layout/openbsd-bpf.4"));
    assert!(real::blocks(&content).into_iter().any(|block| {
        matches!(block, Block::Preformatted { children, .. }
            if mant_ir::inline_plain_text(children) == real::BPF_LITERAL)
    }));
    let rendered = DocumentView::new(&content).render(78);
    let buffer = terminal_cells::paint(&rendered, 78);
    let first = rendered.search("struct bpf_dltlist {");
    assert_eq!(first.len(), 1);
    for (offset, row) in [
        "struct bpf_dltlist {",
        "        u_int bfl_len;",
        "        u_int *bfl_list;",
        "};",
    ]
    .iter()
    .enumerate()
    {
        terminal_cells::assert_row(&buffer, first[0].row + offset, 17, row, "bpf source139–142");
    }
}

#[test]
fn bhyve_three_authored_header_cells_paint_on_one_exact_physical_row() {
    // Complete source587 is one It with three BODY cells. The pristine row
    // starts at common5 + reading12; It pre/post preserve field, not line.
    let content =
        json_roundtrip::json_string_roundtrip(&real::load("scoped-layout/illumos-bhyve.8"));
    let rendered = DocumentView::new(&content).render(78);
    let buffer = terminal_cells::paint(&rendered, 78);
    let row = rendered.anchor_row("backend").unwrap();
    assert_eq!(rendered.anchor_row("path"), Some(row));
    terminal_cells::assert_row(
        &buffer,
        row,
        15,
        "Backend    Path                   Description",
        "bhyve source587: original header cells",
    );
}

#[test]
fn tmux_nested_enum_and_pp_leave_exactly_one_empty_painted_row() {
    // The retained real source1416–1422 ran all five pristine profiles first:
    // It post completes the numbered row; Pp then contributes one blank row.
    let content =
        json_roundtrip::json_string_roundtrip(&real::load("source-adjacency/arch-tmux.1.gz"));
    let rendered = DocumentView::new(&content).render(78);
    let buffer = terminal_cells::paint(&rendered, 78);
    let last = rendered.search(real::TMUX_LAST);
    let after = rendered.search(real::TMUX_AFTER);
    assert_eq!(last.len(), 1);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].row, last[0].row + 2);
    terminal_cells::assert_row(&buffer, last[0].row + 1, 0, "", "tmux exact Pp row");
    terminal_cells::assert_row(
        &buffer,
        after[0].row,
        11,
        real::TMUX_AFTER,
        "tmux following owner",
    );
}
