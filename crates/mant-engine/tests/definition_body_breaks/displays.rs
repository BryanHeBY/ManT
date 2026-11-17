//! Executed native display row closes are distinct from vertical spacing.
use mant_ir::{Block, DefinitionItem, HeadBodyRelation, ResolvedContent, inline_plain_text};
use mant_loader::{load_markdown_text, load_roff_bytes};
use mant_render::render_query_text;

#[path = "../support/json_roundtrip.rs"]
mod json_roundtrip;

const HEADER: &str = ".Dd November 17, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd behavior probe\n.Sh EXIT STATUS\n";
const PAYLOAD: &str = "Execution completed.";

fn exit_status_items(content: &ResolvedContent) -> &[DefinitionItem] {
    let section = content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "EXIT STATUS")
        .unwrap();
    let [Block::DefinitionList { items, .. }] = section.blocks.as_slice() else {
        panic!("expected an exit-status definition list");
    };
    items
}

#[test]
fn display_pre_closes_short_labels_without_inventing_compact_spacing() {
    // CVS mdoc_term.c::termp_bd_pre() calls print_bvspace(); the latter
    // ends the active row before checking compact. Expectations below were
    // taken from the exact inputs on the fixed oracle (without page margin).
    for (style, body_base) in [
        ("inset", 0),
        ("diag", 0),
        ("tag -width 6n", 8),
        ("hang -width 6n", 8),
    ] {
        for kind in ["filled", "ragged"] {
            for compact in ["", " -compact"] {
                for (offset, extra_indent) in [("", 0), (" -offset indent", 6)] {
                    let source = format!(
                        "{HEADER}.Bl -{style}\n.It 0\n.Bd -{kind}{compact}{offset}\n{PAYLOAD}\n.Ed\n.El\n"
                    );
                    let content = json_roundtrip::json_string_roundtrip(
                        &load_roff_bytes(source.as_bytes()).unwrap(),
                    );
                    let [item] = exit_status_items(&content) else {
                        panic!("{source}");
                    };
                    assert_eq!(
                        item.head_body_relation,
                        HeadBodyRelation::Separate,
                        "{source}"
                    );
                    assert_eq!(inline_plain_text(&item.terms[0]), "0", "{source}");
                    let text = render_query_text(&content);
                    let body_indent = " ".repeat(body_base + extra_indent);
                    let gap = if compact.is_empty() { "\n" } else { "" };
                    assert_eq!(
                        text.split_once("EXIT STATUS\n").unwrap().1.trim_end(),
                        format!("0\n{gap}{body_indent}{PAYLOAD}"),
                        "{source}"
                    );
                }
            }
        }
    }
}

#[test]
fn compact_display_and_open_run_in_use_their_executed_rows() {
    for (head, body, expected) in [
        // A regular word follows the generated inset separator on the same row.
        (
            ".It 0\n",
            "Execution completed.\n",
            "0 Execution completed.",
        ),
        (
            ".It 0\n",
            ".D1 Execution completed.\n",
            "0\n      Execution completed.",
        ),
        (
            ".It 0\n",
            ".Bd -literal -compact\nExecution completed.\n.Ed\n",
            "0\nExecution completed.",
        ),
        // The empty display still executes its pre before the following word.
        (
            ".It 0\n",
            ".Bd -filled -compact\n.Ed\nExecution completed.\n",
            "0\nExecution completed.",
        ),
        // With no HEAD, the same pre does not produce any empty output row.
        (
            ".It\n",
            ".Bd -filled -offset indent -compact\nExecution completed.\n.Ed\n",
            "      Execution completed.",
        ),
        // term_newln() in Bd pre does not depend on the child NODE_LINE gate.
        (
            ".It Sy 0\\c\n",
            ".Bd -filled -offset indent -compact\nExecution completed.\n.Ed\n",
            "0\n      Execution completed.",
        ),
    ] {
        let source = format!("{HEADER}.Bl -inset\n{head}{body}.El\n");
        let content = load_roff_bytes(source.as_bytes()).unwrap();
        let text = render_query_text(&content);
        assert_eq!(
            text.split_once("EXIT STATUS\n").unwrap().1.trim_end(),
            expected,
            "{source}"
        );
    }
}

#[test]
fn compact_display_closes_the_head_before_later_invisible_body_rows() {
    // The recorded 80 CVS cases distinguish an empty buffer from NBRZW.
    // term_newln() ends the old HEAD before the display's own buffer or
    // a later br/sp enters term_flushln() (term.c:340-350,475-497).
    for style in ["inset", "diag", "tag -width 6n", "hang -width 6n"] {
        for (display, display_rows) in [("", 0), ("\\&\n", 1), (".No \"\"\n", 0), (".No \\&\n", 1)]
        {
            for (body, body_rows) in [
                ("", 0),
                ("\\&\n.br\n", 1),
                (".No \"\"\n.br\n", 0),
                (".No \\&\n.br\n", 1),
                (".No \\&\n.sp 1\n", 2),
            ] {
                let source = format!(
                    "{HEADER}.Bl -{style}\n.It 0\n.Bd -filled -compact\n{display}.Ed\n{body}PAYLOAD\n.El\n"
                );
                let content = load_roff_bytes(source.as_bytes()).unwrap();
                let text = render_query_text(&content);
                let rows = text
                    .split_once("EXIT STATUS\n")
                    .unwrap()
                    .1
                    .lines()
                    .map(str::trim)
                    .collect::<Vec<_>>();
                let mut expected = vec!["0"];
                expected.extend(std::iter::repeat_n("", display_rows + body_rows));
                expected.push("PAYLOAD");
                assert_eq!(rows, expected, "{source}");
            }
        }
    }
}

#[test]
fn real_ctfdiff_keeps_each_exit_code_and_display_in_one_owner() {
    let source = include_bytes!(
        "../../../../tests/fixtures/roff/real/definition-displays/illumos-ctfdiff.1"
    );
    let content = json_roundtrip::json_string_roundtrip(&load_roff_bytes(source).unwrap());
    let expected = [
        (
            "0",
            "Execution completed successfully, no differences were detected between file1 and file2.",
        ),
        (
            "1",
            "Execution completed successfully, but differences were detected between file1 and file2.",
        ),
        ("2", "Invalid command line options were specified."),
        ("3", "A fatal error occurred."),
    ];
    let items = exit_status_items(&content);
    assert_eq!(items.len(), expected.len());
    for (item, (label, description)) in items.iter().zip(expected) {
        assert_eq!(item.head_body_relation, HeadBodyRelation::Separate);
        assert_eq!(inline_plain_text(&item.terms[0]), label);
        let block = item.description.iter().find_map(|block| match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                Some(inline_plain_text(children))
            }
            _ => None,
        });
        assert_eq!(block.as_deref(), Some(description));
    }

    let text = render_query_text(&content);
    let markdown = mant_codec::encode::render_markdown(&content);
    let read_back = render_query_text(&load_markdown_text(&markdown, None).unwrap());
    let lines: Vec<_> = text.lines().collect();
    let read_lines: Vec<_> = read_back.lines().collect();
    for (label, description) in expected {
        assert_eq!(text.matches(description).count(), 1);
        assert_eq!(read_back.matches(description).count(), 1);
        let row = lines
            .iter()
            .position(|line| line.trim() == description)
            .unwrap();
        assert_eq!(lines[row - 1], label);
        assert_eq!(lines[row], format!("      {description}"));
        let row = read_lines
            .iter()
            .position(|line| line.trim() == description)
            .unwrap();
        assert!(
            read_lines[row - 1]
                .trim_end()
                .ends_with(&format!(" {label}"))
        );
    }
}
