use std::path::Path;

use mant_ir::Block;

use super::{parse_manual_bytes, projected_document_text};

const HEADER: &str = concat!(
    ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n",
    ".Nm probe\n.Nd test page\n.Sh DESCRIPTION\n",
);
const FOOTER: &str = ".Sh END PROBE\nDone.\n";

#[test]
fn obsolete_debug_control_omits_only_its_own_operands() {
    // The fixed ASCII/UTF-8 renderers and HTML use mdoc's skip pre-handler
    // for Db. mdoc_validate.c::post_obsolete still records each authored use.
    for (body, wanted, warning) in [
        ("BEFORE\n.Db GHOSTTOKEN\nAFTER", "BEFORE AFTER", "Db"),
        (
            ".Bd -literal\nBEFORE\n.Db GHOSTTOKEN\nAFTER\n.Ed",
            "BEFORE\nAFTER",
            "Db",
        ),
        (
            ".Bl -tag -width Ds\n.It Xo\n.No BEFORE\n.Db GHOSTTOKEN\n.No AFTER\n.Xc\nBODY\n.El",
            "BEFORE AFTER",
            "Db",
        ),
        (
            ".Bl -tag -width Ds\n.It K\nBEFORE\n.Db GHOSTTOKEN\nAFTER\n.El",
            "BEFORE AFTER",
            "Db",
        ),
        ("BEFORE\n.Es GHOSTTOKEN end\nAFTER", "BEFORE AFTER", "Es"),
        (
            "BEFORE\n.No GHOSTTOKEN\nAFTER",
            "BEFORE GHOSTTOKEN AFTER",
            "",
        ),
    ] {
        let source = format!("{HEADER}{body}\n{FOOTER}");
        let document = parse_manual_bytes(Path::new("probe.1"), source.as_bytes()).unwrap();
        let text = projected_document_text(&document);
        assert!(text.contains(wanted), "{source}\n{text}");
        assert_eq!(text.contains("GHOSTTOKEN"), warning.is_empty(), "{source}");
        if !warning.is_empty() {
            assert!(document.diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .message
                    .ends_with(&format!("obsolete macro: {warning}"))
            }));
        }
    }
}

#[test]
fn obsolete_debug_control_does_not_supply_definition_names() {
    for (head, expected) in [
        (".It Xo\n.Db --fake\n.Fl a\n.Xc", &["-a"][..]),
        (".It Xo\n.Fl a\n.Db --fake\n.Xc", &["-a"][..]),
        (
            ".It Xo\n.Fl a ,\n.Db --fake ,\n.Fl Fl all\n.Xc",
            &["-a", "--all"][..],
        ),
        (".It Fl Fl fake", &["--fake"][..]),
    ] {
        let source = format!("{HEADER}.Bl -tag -width Ds\n{head}\nBODY\n.El\n{FOOTER}");
        let document = parse_manual_bytes(Path::new("probe.1"), source.as_bytes()).unwrap();
        let [Block::DefinitionList { items, .. }] = document.sections[1].blocks.as_slice() else {
            panic!("expected one definition list: {source}");
        };
        assert_eq!(items[0].entry.as_ref().unwrap().names, expected, "{source}");
        assert_eq!(
            projected_document_text(&document).contains("--fake"),
            expected.contains(&"--fake"),
            "{source}"
        );
    }
}

#[test]
fn tbl_retains_debug_operands_without_running_the_mdoc_skip_handler() {
    // In fixed roff.c::roff_req_or_macro, an mdoc request inside tbl is
    // diagnosed as a table macro, then its operands go to tbl_read().
    // This covers both one-line data and continued cells with other input.
    for (cell, expected) in [
        (".Db GHOSTTOKEN", "GHOSTTOKEN"),
        (".Db GHOSTTOKEN\nAFTER", "GHOSTTOKEN AFTER"),
        (".Db GHOSTTOKEN\n.Em AFTER", "GHOSTTOKEN AFTER"),
        (
            ".Em BEFORE\n.Db GHOSTTOKEN\nAFTER",
            "BEFORE GHOSTTOKEN AFTER",
        ),
    ] {
        let source = format!("{HEADER}.TS\nl.\nT{{\n{cell}\nT}}\n.TE\n{FOOTER}");
        let document = parse_manual_bytes(Path::new("probe.1"), source.as_bytes()).unwrap();
        let [Block::Table { rows, .. }] = document.sections[1].blocks.as_slice() else {
            panic!("expected one table: {source}");
        };
        let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
            panic!("expected one paragraph in the table: {source}");
        };
        assert_eq!(mant_ir::inline_plain_text(children), expected, "{source}");
    }
}
