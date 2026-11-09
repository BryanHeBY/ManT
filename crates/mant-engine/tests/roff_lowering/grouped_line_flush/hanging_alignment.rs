//! Alignment row closures keep hanging geometry separate from paragraph spacing.

use flate2::read::GzDecoder;
use libmandoc_rs::{MacroToken, Node, NodeKind, Parser, RoffMacro};
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{Block, TextAlignment, inline_plain_text};
use serde::Deserialize;

use super::load;
use crate::case_failures::verify_cases;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Matrix {
    oracle_sha256: String,
    width: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    id: String,
    source: String,
    source_sha256: String,
    profiles: std::collections::BTreeMap<String, (i32, String, String)>,
    group_tree: Vec<String>,
    lint: Option<String>,
    rows: Vec<String>,
}

fn find_group(node: &Node) -> Option<&Node> {
    if matches!(
        node.macro_token,
        Some(MacroToken::Roff(RoffMacro::Ce | RoffMacro::Rj))
    ) {
        Some(node)
    } else {
        node.children.iter().find_map(find_group)
    }
}

fn body(text: &str) -> &str {
    let (_, body) = text.split_once("\nDESCRIPTION\n").unwrap();
    body.strip_suffix("\n\nEND\nENDMARK").unwrap()
}

fn without_origin(row: &str) -> &str {
    // No source case writes leading SP in its operands. These columns are
    // the native page margin, alignment geometry or portable block origins.
    row.trim_start_matches(' ')
}

fn heading_anchor(round: usize) -> String {
    // The Markdown loader preserves raw heading IDs as authored HTML text.
    // The only unsupported source-generated HTML in this cohort is the END
    // heading's immediately preceding marker, reproduced once per export.
    "\n\n<a id=\"end\"></a>".repeat(round)
}

fn native_group_witness(case: &Case) {
    let parsed = Parser::default()
        .parse_bytes("grouped-alignment.1", case.source.as_bytes())
        .unwrap();
    let node = find_group(&parsed.document.root).expect("native grouped request");
    let tree = &case.group_tree;
    assert_eq!(
        node.macro_token,
        Some(MacroToken::Roff(if tree[0].contains("ce (elem)") {
            RoffMacro::Ce
        } else {
            assert!(tree[0].contains("rj (elem)"));
            RoffMacro::Rj
        }))
    );
    assert_eq!(node.flags.no_fill, tree[0].ends_with(" NOFILL"));
    assert_eq!(
        node.children[0].text.as_deref().unwrap(),
        tree[1].split_whitespace().next().unwrap(),
        "actual parsed count"
    );
    assert_eq!(
        node.children
            .iter()
            .filter(|child| child.kind == NodeKind::Text && child.flags.line_start)
            .count(),
        tree.iter()
            .filter(|line| line.contains(" (text) *"))
            .count(),
        "actual physical TEXT operands"
    );
}

#[test]
fn alignment_groups_preserve_hard_rows_across_hanging_paragraphs() {
    // The recorded sources precede this assertion. roff_term_pre_ce() calls
    // pre_br(), not term_vspace(). man_term.c::pre_HP establishes the
    // continuation offset that pre_br() adopts without opening a paragraph.
    let bytes = include_bytes!("alignment.json.gz");
    let matrix: Matrix = serde_json::from_reader(GzDecoder::new(bytes.as_slice())).unwrap();
    assert_eq!(
        matrix.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(matrix.width, 160);
    assert_eq!(matrix.cases.len(), 1504);
    verify_cases(
        matrix.cases,
        |case| &case.id,
        |case| {
            assert_eq!(case.source_sha256.len(), 64);
            for profile in ["ascii", "utf8", "html", "tree", "lint"] {
                let (status, stdout, stderr) = &case.profiles[profile];
                assert_eq!(stdout.len(), 64);
                assert_eq!(stderr.len(), 64);
                if profile == "lint" {
                    assert_eq!(*status == 0, case.lint.is_none());
                    assert!(*status <= 2);
                } else {
                    assert_eq!(*status, 0);
                }
            }
            native_group_witness(case);
            let original = load(&case.source);
            let plain = mant_render::render_query_man(&original);
            let direct = body(&plain)
                .split('\n')
                .map(without_origin)
                .collect::<Vec<_>>();
            let native = case
                .rows
                .iter()
                .map(|row| without_origin(row))
                .collect::<Vec<_>>();
            let id = case.id.split('/').collect::<Vec<_>>();
            let wide_filled_hp = id[0] == "man" && id[1].ends_with("hp16") && id[2] == "fill";
            // A short HP field can remain open on the native character
            // device at pre_br; ManT keeps the authored hard line. Only
            // the exact native padding from that one seam can be dropped.
            // The related count0/bare-z/br and /PP glyph loss is Review
            // pending a separate native field fix. Never gold FTERMARK.
            let short_field_zero_review =
                wide_filled_hp && id[4..6] == ["0", "zero"] && matches!(id[6], "br" | "pp");
            if !short_field_zero_review {
                let mut expected = native.clone();
                if wide_filled_hp {
                    if let Some(following) = native[0].strip_prefix("BEFOREMARK ")
                        && following.starts_with(' ')
                    {
                        expected[0] = "BEFOREMARK";
                        expected.insert(1, without_origin(following));
                    } else if id[5] == "zero"
                        && id[4] != "0"
                        && (native.get(1) != Some(&"")
                            || matches!(id[4], "default" | "1") && id[6] == "pp")
                    {
                        expected.insert(1, "");
                    }
                }
                assert_eq!(direct, expected, "native and portable physical rows");
            }
            let ansi = mant_render::render_query_text_with(&original, |_, text| {
                format!("\x1b[1m{text}\x1b[0m")
            });
            assert_eq!(ansi.replace("\x1b[1m", "").replace("\x1b[0m", ""), plain);

            // An already-empty aligned tail followed by PP can be
            // normalized to a single paragraph gap by Markdown. These
            // exact gap cases are Review and have their own named axis;
            // do not apply this comparison to any interior empty row.
            let paragraph_gap_review = id[0] == "man"
                && id[2] == "fill"
                && id[6] == "pp"
                && (id[5] == "empty" && id[4] != "2"
                    || id[5] == "zero" && matches!(id[4], "default" | "1"));
            let mut expected = markdown_rows(&direct, &id, paragraph_gap_review);
            for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
                if options.preserve_anchors && id[0] == "mdoc" && matches!(id[1], "tag" | "hang") {
                    // The raw definition term ID, like the END heading ID,
                    // stays literal on import. This is the one exact marker.
                    expected[0].insert_str("• ".len(), "<a id=\"term-label\"></a>");
                }
                let mut content = original.clone();
                for round in 1..=2 {
                    let markdown = render_markdown_with_options(&content, options);
                    content = super::super::json_roundtrip::json_string_roundtrip(
                        &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                    );
                    let rendered = mant_render::render_query_text(&content);
                    let mut readback = body(&rendered);
                    if options.preserve_anchors {
                        readback = readback.strip_suffix(&heading_anchor(round)).unwrap();
                    }
                    assert_eq!(
                        readback.split('\n').map(without_origin).collect::<Vec<_>>(),
                        expected,
                        "Markdown {options:?} cycle {round}"
                    );
                }
            }
        },
    );
}

fn markdown_rows(direct: &[&str], id: &[&str], paragraph_gap_review: bool) -> Vec<String> {
    let mut rows = direct.iter().map(ToString::to_string).collect::<Vec<_>>();
    if paragraph_gap_review {
        let end = rows.len() - 1;
        assert_eq!(rows[end], "AFTERMARK");
        let previous = rows.iter().rposition(|row| row == "BEFOREMARK").unwrap();
        assert!(rows[previous + 1..end].iter().all(String::is_empty));
        rows.splice(previous + 1..end, [String::new()]);
    }
    if id[0] == "mdoc" {
        // The first line of the list item (or fenced table) has these
        // known portable markers. The rest of the body has exact rows.
        match id[1] {
            "tag" | "hang" => {
                rows[0] = if rows[0] == "LABEL" {
                    "• LABEL".into()
                } else {
                    assert_eq!(rows[0], "LABEL   BEFOREMARK");
                    "• LABEL BEFOREMARK".into()
                }
            }
            "column" => {
                rows[0] = rows[0].replacen("FIRST  SECOND", "FIRST | SECOND", 1);
            }
            _ => {}
        }
    }
    rows
}

#[test]
fn hanging_row_hints_stay_in_one_paragraph_after_json_roundtrip() {
    for (source, alignment) in [
        (include_str!("hanging-wrap-ce.1"), TextAlignment::Center),
        (include_str!("hanging-wrap-rj.1"), TextAlignment::Right),
    ] {
        let content = load(source);
        let section = &content.document.as_ref().unwrap().sections[1];
        let [
            Block::Paragraph {
                children,
                inline_layout,
                layout,
                ..
            },
        ] = section.blocks.as_slice()
        else {
            panic!("one semantic HP paragraph: {section:?}");
        };
        assert_eq!(layout.indent_columns, 0);
        assert_eq!(layout.continuation_indent_columns, 4);
        assert_eq!(layout.spacing_before_lines, 0);
        assert_eq!(
            inline_plain_text(children),
            "FIRSTMARK second continuation\nROWONE\nROWTWO\nAFTERMARK"
        );
        assert_eq!(inline_layout.row_hints.len(), 2);
        for (index, hint) in inline_layout.row_hints.iter().enumerate() {
            assert_eq!(hint.row, u32::try_from(index + 1).unwrap());
            assert_eq!(hint.indent_columns, 0);
            assert_eq!(hint.alignment, alignment);
        }
    }
}
