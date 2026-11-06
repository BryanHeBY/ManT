//! Complete original manuals exercise selected native source-event boundaries.

use mant_ir::{Block, TableRowKind};

#[path = "support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/field_boundary_real.rs"]
mod real;

const CHEM_PROSE: &str = "Normally the bond begins at the last object placed;";
const KSH_COMMAND: &str = "$ [[ -r foo && $(< foo) = b*r ]]";

fn read_markdown(content: &mant_ir::ResolvedContent) -> mant_ir::ResolvedContent {
    json_roundtrip::json_string_roundtrip(
        &mant_loader::load_markdown_text(&mant_codec::encode::render_markdown(content), None)
            .unwrap(),
    )
}

#[test]
fn chem_synopsis_exit_keeps_later_ordinary_prose_in_one_owner() {
    // The complete original gzip ran all five pristine profiles first.
    // post_SY BODY consumes its row and restores offset, not a finite HEAD
    // margin for subsequent PP/RS ordinary words (man_term.c:878-886).
    let mut content =
        json_roundtrip::json_string_roundtrip(&real::load("field-boundaries/debian-chem.1.gz"));
    for round in 0..=2 {
        let owners = real::blocks(&content)
            .into_iter()
            .filter(|block| {
                matches!(block, Block::Paragraph { children, .. }
                    if mant_ir::inline_plain_text(children).contains(CHEM_PROSE))
            })
            .count();
        assert_eq!(owners, 1, "round{round}: complete filled prose owner");
        assert!(
            mant_render::render_query_man(&content)
                .lines()
                .any(|row| row.contains(CHEM_PROSE))
        );
        if round < 2 {
            content = read_markdown(&content);
        }
    }
}

#[test]
fn scmdctl_motor_table_keeps_all_data_and_rule_spans_in_one_owner() {
    // Native tree: TS163/TE191 owns 16 spans (ten data, six layout rules),
    // independent of the earlier device table at source141. First-span
    // term_newln consumes the outer It input before tbl's own columns.
    let content =
        json_roundtrip::json_string_roundtrip(&real::load("field-boundaries/netbsd-scmdctl.1"));
    let owners = real::blocks(&content)
        .into_iter()
        .filter(|block| {
            matches!(block, Block::Table { source: Some(source), .. }
                if source.line > 163 && source.line < 191)
        })
        .collect::<Vec<_>>();
    assert_eq!(owners.len(), 1, "the original motor TS/TE is one owner");
    let Block::Table { rows, .. } = owners[0] else {
        unreachable!()
    };
    assert_eq!(rows.len(), 16);
    assert_eq!(
        rows.iter()
            .filter(|row| matches!(row.kind, TableRowKind::Data))
            .count(),
        10
    );
    let labels = rows
        .iter()
        .filter(|row| matches!(row.kind, TableRowKind::Data))
        .map(|row| match &row.cells[0].blocks[0] {
            Block::Paragraph { children, .. } => mant_ir::inline_plain_text(children),
            _ => panic!("native cell paragraph"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        [
            "Subcommand",
            "get",
            "set",
            "",
            "",
            "invert",
            "",
            "bridge",
            "enable",
            "disable"
        ]
    );
    let mut current = content;
    for _ in 0..2 {
        current = read_markdown(&current);
        let motor = real::blocks(&current)
            .into_iter()
            .filter(|block| {
                matches!(block, Block::Preformatted { children, .. }
                    if mant_ir::inline_plain_text(children).contains("Subcommand")
                    && mant_ir::inline_plain_text(children).contains("disable"))
            })
            .count();
        assert_eq!(motor, 1, "one portable fence for the whole original table");
    }
}

#[test]
fn mandoc_nested_enum_display_keeps_its_local_source_offset() {
    // Pristine DISPLAY at24, subsequent plain BODY at18: local +6.
    // This fixed section uses the selected five-column reading page origin.
    let content =
        json_roundtrip::json_string_roundtrip(&real::load("field-boundaries/openbsd-mandoc.1"));
    let rendered = mant_render::render_query_man(&content);
    let display = rendered
        .lines()
        .find(|row| row.contains("coding:"))
        .unwrap();
    assert_eq!(display.find(".\\\" -*-"), Some(19));
    let after = rendered
        .lines()
        .find(|row| row.contains("then input is interpreted according to"))
        .unwrap();
    assert_eq!(after.find("then input"), Some(13));
    let mut current = content;
    for _ in 0..2 {
        current = read_markdown(&current);
        assert_eq!(
            real::blocks(&current)
                .into_iter()
                .filter(|block| {
                    matches!(block, Block::Preformatted { children, .. }
                        if mant_ir::inline_plain_text(children).contains("coding:"))
                })
                .count(),
            1
        );
    }
}

#[test]
fn ksh_nested_bullet_literal_keeps_its_parent_relative_offset() {
    // The complete source's native literal origin is29, its plain BODY23.
    // Compare that relative source fact; ksh has independent section layout,
    // so a whole-page native-minus-five transformation would be incorrect.
    let content =
        json_roundtrip::json_string_roundtrip(&real::load("field-boundaries/openbsd-ksh.1"));
    let rendered = mant_render::render_query_man(&content);
    let rows = rendered.lines().collect::<Vec<_>>();
    let display = rows
        .iter()
        .position(|row| row.contains(KSH_COMMAND))
        .unwrap();
    let before = rows[..display]
        .iter()
        .rfind(|row| row.contains("Parameter, command"))
        .unwrap();
    // Locate the actual BODY glyph after its structural bullet, rather
    // than that marker or the native device's optional soft-wrap point.
    let body_origin =
        mant_ir::geometry::text_width(&before[..before.find("Parameter, command").unwrap()]);
    assert_eq!(rows[display].find(KSH_COMMAND), Some(body_origin + 6));
    let mut current = content;
    for _ in 0..2 {
        current = read_markdown(&current);
        assert_eq!(
            real::blocks(&current)
                .into_iter()
                .filter(|block| {
                    matches!(block, Block::Preformatted { children, .. }
                        if mant_ir::inline_plain_text(children).lines().any(|row| row == KSH_COMMAND))
                })
                .count(),
            1
        );
    }
}
