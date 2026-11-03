//! Four existing source-bound cards allow a responsive column tail row.
//! Source identity and original IR scalars are independent of terminal paint.

use super::{Case, responsive_cells};
use mant_ir::{
    Block, Inline, ResolvedContent, TableRow, inline_plain_text,
    visit::{self, Visit},
};

struct TabCard {
    literal: &'static str,
    width: &'static str,
    control: &'static str,
    source_sha256: &'static str,
}

impl TabCard {
    fn source(&self) -> String {
        format!(
            ".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -column \"{}\" \"xxxx\"\n.It Xo\n.No \"{}\"\n{}.No AFTER\n.Xc Ta RIGHT\n.El\n.Sh NEXT\n.No END\n",
            self.width, self.literal, self.control
        )
    }

    fn owner_text(&self) -> String {
        // The exact native term_field run resolves the default .5i stop
        // before a later graph: five leading cells or four after A.
        format!(
            "{} AFTER",
            self.literal.replace(
                '\t',
                if self.literal.starts_with('A') {
                    "    "
                } else {
                    "     "
                }
            )
        )
    }
}

fn tab_card(case: &Case) -> Option<TabCard> {
    // These four complete inputs ran pristine ASCII/UTF-8(width78), HTML,
    // tree and lint again before adapting this harness. mdoc_argv.c:521-570
    // keeps the quoted TAB in one It BODY; mdoc_term.c:817-826,946-948 and
    // term.c:250-253 let the next declared field start on a separate row.
    // No general source parser or family-wide row permission lives here.
    let card = match case.id.as_str() {
        "field-pre-br-core-2100" => TabCard {
            literal: "\t",
            width: "xxxx",
            control: "",
            source_sha256: "40455a6f31f06711115e65a6692fa9bed2c1946439d848c2f9d1be9064325ab4",
        },
        "field-pre-br-widths-0156" => TabCard {
            literal: "A\tB",
            width: "xxxxxxxx",
            control: ".br\n",
            source_sha256: "b24ac98772cbafeb0d90e4016226048c178e8e849ad44b5cfd8a7cd9e3e6ec58",
        },
        "field-pre-br-widths-0157" => TabCard {
            literal: "A\tB",
            width: "xxxxxxxx",
            control: ".nf\n.fi\n",
            source_sha256: "06a62f0c76d159b8fb7f9c08462f84f68e483585e79f6b9597bce26c8a15a23f",
        },
        "field-pre-br-widths-0158" => TabCard {
            literal: "A\tB",
            width: "xxxxxxxx",
            control: ".ti\n",
            source_sha256: "34cba60e437d150c1793fdfa5e72dfdb516459464e8deab8e9d0e5d9f5c58ba4",
        },
        _ => return None,
    };
    let projection = &case.projection;
    let boundary = usize::from(card.literal.starts_with('A'));
    (case.source == card.source()
        && case.source_sha256 == card.source_sha256
        && projection["id"] == case.id
        && projection["source_sha256"] == card.source_sha256
        && projection["oracle_sha256"]
            == "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6"
        && projection["native_utf8_sha256"] == case.profiles["utf8"]["stdout_sha256"]
        && projection["native_tree_sha256"] == case.profiles["tree"]["stdout_sha256"]
        && projection["literal_tab_sites"]
            == serde_json::json!([{"row": 0, "scalar_boundary": boundary}])
        && projection["native_row_joins"] == serde_json::json!([{"row": 1, "content": "RIGHT"}]))
    .then_some(card)
}

pub(super) fn authored_tab_text(case: &Case) -> Option<&'static str> {
    tab_card(case).map(|card| card.literal)
}

fn table_rows(content: &ResolvedContent) -> Option<&[TableRow]> {
    let document = content.document.as_ref()?;
    let section = document.sections.get(1)?;
    if inline_plain_text(&section.heading.content) != "DESCRIPTION" || section.blocks.len() != 1 {
        return None;
    }
    match &section.blocks[0] {
        Block::Table { rows, source, .. }
            if rows.len() == 1
                && rows[0].cells.len() == 2
                && source
                    .as_ref()
                    .is_some_and(|source| source.line == 8 && source.column == 2) =>
        {
            Some(rows)
        }
        _ => None,
    }
}

fn paragraph_text(blocks: &[Block], line: u32, column: u32) -> Option<String> {
    match blocks {
        [
            Block::Paragraph {
                children, source, ..
            },
        ] if source
            .as_ref()
            .is_some_and(|source| source.line == line && source.column == column) =>
        {
            Some(inline_plain_text(children))
        }
        _ => None,
    }
}

fn literal_tab_count(content: &ResolvedContent) -> usize {
    struct Tabs(usize);
    impl<'ir> Visit<'ir> for Tabs {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            match inline {
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    self.0 += value.matches('\t').count();
                }
                _ => {}
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut tabs = Tabs(0);
    if let Some(document) = &content.document {
        tabs.visit_document(document);
    }
    tabs.0
}

pub(super) fn resolved_tab_owner_matches(case: &Case, content: &ResolvedContent) -> bool {
    if case.projection.get("literal_tab_sites").is_none() {
        return true;
    }
    let Some(card) = tab_card(case) else {
        return false;
    };
    let Some(rows) = table_rows(content) else {
        return false;
    };
    let first = paragraph_text(&rows[0].cells[0].blocks, 10, 2);
    let right_line = 12 + u32::try_from(card.control.lines().count()).unwrap();
    let second = paragraph_text(&rows[0].cells[1].blocks, right_line, 8);
    // assert_owned_ast independently pins the authored TAB and its original
    // source scalar. Post-execution IR owns the actual resolved spaces in
    // that same cell, all word seams, and no invented hard break. Native
    // term.c:394-427 consumes TAB at the real field print, not at word append.
    first.as_deref() == Some(card.owner_text().as_str())
        && second.as_deref() == Some("RIGHT")
        && literal_tab_count(content) == 0
}

fn responsive_rows(rows: &[String]) -> Vec<String> {
    rows.iter().map(|row| responsive_cells(row)).collect()
}

pub(super) fn rows_match(case: &Case, actual: &[String]) -> bool {
    let expected = responsive_rows(&case.expected_rows);
    let joins = case.projection["native_row_joins"].as_array();
    if joins.is_none_or(Vec::is_empty) {
        return responsive_rows(actual) == expected;
    }
    if tab_card(case).is_none() {
        return false;
    }
    let Some(native) = &case.native_region else {
        return false;
    };
    let raw = responsive_rows(&native.rows);
    if native.status != "asserted" || raw.len() != 2 || raw[1] != "RIGHT" {
        return false;
    }
    // Reuse the existing responsive_cells projection. The original card's
    // only optional join must reproduce the frozen gold exactly. Compare
    // either complete row vector, never collapse arbitrary product rows.
    let joined = vec![format!("{} RIGHT", raw[0])];
    if joined != expected {
        return false;
    }
    let actual = responsive_rows(actual);
    actual == raw || actual == expected
}

fn first_cell_children(content: &mut ResolvedContent) -> &mut Vec<Inline> {
    let document = content.document.as_mut().unwrap();
    let Block::Table { rows, .. } = &mut document.sections[1].blocks[0] else {
        panic!("original table owner");
    };
    let Block::Paragraph { children, .. } = &mut rows[0].cells[0].blocks[0] else {
        panic!("original paragraph root");
    };
    children
}

#[test]
fn finite_column_tail_cards_preserve_rows_seams_and_resolved_tab_owners() {
    let (_, cases) = super::fixture();
    let mut checked = 0;
    for mut case in cases {
        let Some(card) = tab_card(&case) else {
            continue;
        };
        checked += 1;
        let native = &case.native_region.as_ref().unwrap().rows;
        assert!(rows_match(&case, native), "{}: native raw rows", case.id);
        assert!(
            rows_match(&case, &case.expected_rows),
            "{}: optional tail join",
            case.id
        );
        for row in 0..native.len() {
            let mut deleted = native.clone();
            deleted.remove(row);
            assert!(
                !rows_match(&case, &deleted),
                "{}: deleted row {row}",
                case.id
            );
            let mut added = native.clone();
            added.insert(row, String::new());
            assert!(
                !rows_match(&case, &added),
                "{}: added blank row {row}",
                case.id
            );
        }
        let first = responsive_cells(&native[0]);
        let glued = vec![format!("{first}RIGHT")];
        assert!(
            !rows_match(&case, &glued),
            "{}: lost tail word seam",
            case.id
        );
        let extra_break = vec![first.replace("AFTER", "AF"), "TER".into(), "RIGHT".into()];
        assert!(
            !rows_match(&case, &extra_break),
            "{}: invented hard row",
            case.id
        );

        let content = super::product_content(&case);
        assert!(resolved_tab_owner_matches(&case, &content), "{}", case.id);
        let original = card.owner_text();
        for mutated in [
            original.replacen(' ', "", 1),
            original.replacen(' ', "\t", 1),
            format!("{original} "),
            format!("\t{original}"),
            original.replace(" AFTER", "AFTER"),
        ] {
            let mut changed = content.clone();
            *first_cell_children(&mut changed) = vec![Inline::Text { value: mutated }];
            assert!(
                !resolved_tab_owner_matches(&case, &changed),
                "{}: altered owner",
                case.id
            );
        }
        let mut changed = content.clone();
        first_cell_children(&mut changed).push(Inline::line_break());
        assert!(
            !resolved_tab_owner_matches(&case, &changed),
            "{}: inserted hard break",
            case.id
        );
        let mut moved = content.clone();
        let document = moved.document.as_mut().unwrap();
        let Block::Table { rows, .. } = &mut document.sections[1].blocks[0] else {
            unreachable!()
        };
        rows[0].cells.swap(0, 1);
        assert!(
            !resolved_tab_owner_matches(&case, &moved),
            "{}: moved TAB owner",
            case.id
        );
        let mut copied = content.clone();
        let document = copied.document.as_mut().unwrap();
        let Block::Paragraph { children, .. } = &mut document.sections[2].blocks[0] else {
            unreachable!()
        };
        children.push(Inline::Text { value: "\t".into() });
        assert!(
            !resolved_tab_owner_matches(&case, &copied),
            "{}: copied TAB to another owner",
            case.id
        );

        case.source.push('\n');
        assert!(
            !rows_match(&case, &case.expected_rows),
            "{}: source binding changed",
            case.id
        );
    }
    assert_eq!(checked, 4);
}

#[test]
fn unregistered_rows_never_inherit_the_column_tail_permission() {
    let (_, cases) = super::fixture();
    let case = &cases[0];
    assert_eq!(case.id, "field-pre-br-core-0000");
    assert_eq!(case.expected_rows.len(), 2);
    assert!(rows_match(case, &case.expected_rows));
    let mut deleted = case.expected_rows.clone();
    deleted.remove(0);
    assert!(!rows_match(case, &deleted));
    let mut added = case.expected_rows.clone();
    added.insert(1, String::new());
    assert!(!rows_match(case, &added));
    assert!(!rows_match(case, &[case.expected_rows.join(" ")]));
}
