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

fn find_group(node: &Node) -> Option<(&Node, Option<&Node>)> {
    node.children.iter().enumerate().find_map(|(index, child)| {
        if matches!(
            child.macro_token,
            Some(MacroToken::Roff(RoffMacro::Ce | RoffMacro::Rj))
        ) {
            Some((child, node.children.get(index + 1)))
        } else {
            find_group(child)
        }
    })
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

const MAN_HEADER: &str = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH DESCRIPTION\n";
const MAN_END: &str = ".SH END\nENDMARK\n";

fn assert_man_rows(source: &str, expected: &[&str]) {
    let original = load(source);
    let plain = mant_render::render_query_man(&original);
    let rows = plain
        .split_once("\nDESCRIPTION\n")
        .unwrap()
        .1
        .split('\n')
        .map(without_origin)
        .collect::<Vec<_>>();
    assert_eq!(rows, expected, "{source}");
}

#[test]
fn invisible_flushes_close_the_existing_hanging_row_only_once() {
    // Pristine ASCII, UTF-8, tree and lint ran for every source. The HP
    // tail test in term.c:250-253 ends BEFOREMARK at 275u but keeps that
    // device row open at 276u. An embedded pre_br may then close the
    // existing row; only a subsequent empty flush adds a blank.
    for request in ["ce", "rj"] {
        for width in ["275u", "276u", "277u", "16"] {
            for word in ["\\&", "\\z", "\\&\\c"] {
                for (control, spacing) in [("", 0), (".sp 0\n", 0), (".br\n", 0), (".sp 1\n", 1)] {
                    let blanks = if width == "275u" {
                        1 + usize::from(!control.is_empty() && word != "\\z")
                    } else {
                        usize::from(!control.is_empty())
                    } + spacing;
                    let source = format!(
                        "{MAN_HEADER}.HP {width}\nBEFOREMARK\n.{request} 1\n{word}\n{control}AFTERMARK\n{MAN_END}"
                    );
                    let mut rows = vec!["BEFOREMARK"];
                    rows.extend(std::iter::repeat_n("", blanks));
                    rows.extend(["AFTERMARK", "", "END", "ENDMARK"]);
                    assert_man_rows(&source, &rows);
                }
            }
        }
    }

    // A break marker can be rejected without printing a fresh row.
    // In these exact fixed-CVS cases, the pre_br/group/paragraph boundary
    // closes the hanging prefix and only explicit spacing adds one blank.
    for (body, blanks) in [
        (".HP 276u\nBEFOREMARK\n.rj -1\n\\p\n.sp 0\n", 0),
        (".HP 277u\nBEFOREMARK\n.rj -1\n\\p\n.sp 1\n", 1),
        (".HP 24n\nBEFOREMARK\n.rj 2\n\\p\n.PP\n", 1),
        (".HP 16\nBEFOREMARK\n.rj -1\n\\z\n\\z\n.PP\n", 1),
    ] {
        let mut rows = vec!["BEFOREMARK"];
        rows.extend(std::iter::repeat_n("", blanks));
        rows.extend(["AFTERMARK", "", "END", "ENDMARK"]);
        assert_man_rows(&format!("{MAN_HEADER}{body}AFTERMARK\n{MAN_END}"), &rows);
    }
}

#[test]
fn man_structural_boundaries_close_an_invisible_tail_before_spacing() {
    // Exact sources were verified on pristine ASCII/UTF-8/tree/lint.
    // Zero/negative count leaves these words outside roff_term_pre_ce().
    // print_bvspace() first closes their occupied row, then adds PD.
    for request in ["ce", "rj"] {
        for count in ["-1", "0"] {
            for words in [
                "\\z\n\\z\n",
                "\\&\n\\&\n",
                "\\&\\c\n\\&\n",
                ".de AA\n\\&\n\\&\n..\n.AA\n",
            ] {
                for (boundary, body) in [
                    (".PP\n", "AFTERMARK"),
                    (".P\n", "AFTERMARK"),
                    (".LP\n", "AFTERMARK"),
                    (".HP 9\n", "AFTERMARK"),
                    (".IP TAG 9\n", "TAG      AFTERMARK"),
                    (".TP 9\nTAG\n", "TAG      AFTERMARK"),
                    (".SY command\n", "command AFTERMARK"),
                ] {
                    let suffix = if boundary.starts_with(".SY") {
                        ".YS\n"
                    } else {
                        ""
                    };
                    let source = format!(
                        "{MAN_HEADER}BEFOREMARK\n.{request} {count}\n{words}{boundary}AFTERMARK\n{suffix}{MAN_END}"
                    );
                    assert_man_rows(&source, &["BEFOREMARK", "", "", body, "", "END", "ENDMARK"]);
                }
            }
        }
    }

    // post_SH() closes the BODY at the next section or EOF. In the middle
    // of a BODY, pre_SS() only closes an invisible predecessor when PD
    // executes term_vspace(); PD 0 carries that row into the heading.
    for (tail, rows) in [
        (
            ".SS AFTER\nAFTERMARK\n",
            &["", "", "AFTER", "AFTERMARK"][..],
        ),
        (".SH AFTER\nAFTERMARK\n", &["", "", "AFTER", "AFTERMARK"]),
        ("", &["", ""]),
    ] {
        for prior in [".ce 0\n\\&\n", ".rj 0\n\\p\n"] {
            let expected = ["BEFOREMARK"]
                .into_iter()
                .chain(rows.iter().copied())
                .collect::<Vec<_>>();
            assert_man_rows(&format!("{MAN_HEADER}BEFOREMARK\n{prior}{tail}"), &expected);
        }
    }
    for (tail, rows) in [
        (".SS AFTER\nAFTERMARK\n", &["AFTER", "AFTERMARK"][..]),
        (".SH AFTER\nAFTERMARK\n", &["", "AFTER", "AFTERMARK"]),
        ("", &["", ""]),
    ] {
        let expected = ["BEFOREMARK"]
            .into_iter()
            .chain(rows.iter().copied())
            .collect::<Vec<_>>();
        assert_man_rows(
            &format!("{MAN_HEADER}BEFOREMARK\n.PD 0\n.ce 0\n\\&\n{tail}"),
            &expected,
        );
    }
}

fn heading_anchor(round: usize) -> String {
    // The Markdown loader preserves raw heading IDs as authored HTML text.
    // The only unsupported source-generated HTML in this cohort is the END
    // heading's immediately preceding marker, reproduced once per export.
    "\n\n<a id=\"end\"></a>".repeat(round)
}

fn native_group_witness(case: &Case, paragraph_gap_policy: bool) {
    let parsed = Parser::default()
        .parse_bytes("grouped-alignment.1", case.source.as_bytes())
        .unwrap();
    let (node, following) = find_group(&parsed.document.root).expect("native grouped request");
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
    if paragraph_gap_policy {
        assert_eq!(node.children.len(), 1, "count-only group has no hard row");
        assert_eq!(
            following.and_then(|node| node.macro_token.as_ref()),
            Some(&MacroToken::Roff(RoffMacro::Sp)),
            "source blank generated a spacing request outside the group"
        );
    }
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
            let id = case.id.split('/').collect::<Vec<_>>();
            // roff.c::roff_onearg suppresses collection at count zero.
            // The man.c::man_ptext blank becomes a sibling sp, so only
            // these explicit positive spacing contributions can collapse.
            let paragraph_gap_policy = id[0] == "man"
                && id[2] == "fill"
                && id[4] == "0"
                && id[5] == "empty"
                && id[6] == "pp";
            native_group_witness(case, paragraph_gap_policy);
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
            let wide_filled_hp = id[0] == "man" && id[1].ends_with("hp16") && id[2] == "fill";
            // A short HP field can remain open on the native character
            // device at pre_br; ManT keeps the authored hard line. Only
            // the exact native padding from that one seam can be dropped.
            // A graphless group flush can close that occupied device row.
            // The row already has a portable end and contributes no new
            // empty row (term.c:475-481,233-253).
            let mut expected = native.clone();
            if wide_filled_hp
                && let Some(following) = native[0].strip_prefix("BEFOREMARK ")
                && following.starts_with(' ')
            {
                expected[0] = "BEFOREMARK";
                expected.insert(1, without_origin(following));
            }
            assert_eq!(direct, expected, "native and portable physical rows");
            let ansi = mant_render::render_query_text_with(&original, |_, text| {
                format!("\x1b[1m{text}\x1b[0m")
            });
            assert_eq!(ansi.replace("\x1b[1m", "").replace("\x1b[0m", ""), plain);

            let mut expected = markdown_rows(&expected, &id, paragraph_gap_policy);
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

fn markdown_rows(direct: &[&str], id: &[&str], paragraph_gap_policy: bool) -> Vec<String> {
    let mut rows = direct.iter().map(ToString::to_string).collect::<Vec<_>>();
    if paragraph_gap_policy {
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

const MAN: &str = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH DESCRIPTION\n";
const END: &str = ".SH END\nENDMARK\n";

#[test]
fn occupied_hanging_device_row_controls_a_later_conditional_flush() {
    // The pinned CVS ASCII/UTF-8 rows for these exact sources bracket
    // term_flushln()'s final vbr + trailspace > vfield + half-EN test.
    // At 276u the original HP field still occupies the device. Its
    // later .br flushes BACKAFTER even though the reading line already
    // ended. The direct successor instead consumes that pending escape.
    for (prefix, suffix) in [("", ""), (".RS 3\n.RS 2\n", ".RE\n.RE\n")] {
        for (width, occupied) in [("275u", false), ("276u", true), ("277u", true)] {
            for alignment in ["ce", "rj"] {
                for count in ["0", "-1"] {
                    for (request, gap) in [
                        ("", false),
                        (".br\n", false),
                        (".br\n.br\n", false),
                        (".PP\n", true),
                    ] {
                        let source = format!(
                            "{MAN}{prefix}.HP {width}\nBEFOREMARK\n.{alignment} {count}\n\\z\n{request}AFTERMARK\n{suffix}{END}"
                        );
                        let last = if occupied && !request.is_empty() {
                            "AFTERMARK"
                        } else {
                            "FTERMARK"
                        };
                        let expected = if gap {
                            vec!["BEFOREMARK", "", last]
                        } else {
                            vec!["BEFOREMARK", last]
                        };
                        let original = load(&source);
                        let rendered = mant_render::render_query_man(&original);
                        assert_eq!(
                            body(&rendered)
                                .split('\n')
                                .map(without_origin)
                                .collect::<Vec<_>>(),
                            expected,
                            "{source}"
                        );
                        for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
                            let mut content = original.clone();
                            for round in 1..=2 {
                                let markdown = render_markdown_with_options(&content, options);
                                content = super::json_string_roundtrip(
                                    &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                                );
                                let rendered = mant_render::render_query_text(&content);
                                let mut readback = body(&rendered);
                                if options.preserve_anchors {
                                    readback =
                                        readback.strip_suffix(&heading_anchor(round)).unwrap();
                                }
                                assert_eq!(
                                    readback.split('\n').map(without_origin).collect::<Vec<_>>(),
                                    expected,
                                    "{options:?} cycle {round}: {source}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn occupied_hanging_device_row_keeps_the_following_section_owner() {
    // man_term.c::pre_SH calls term_newln before its heading word. The
    // frozen wide-HP source has NEXT, with the bare escape already flushed.
    let source = format!("{MAN}.HP 16n\nBEFOREMARK\n.ce 0\n\\z\n.SH NEXT\nAFTERMARK\n{END}");
    let content = load(&source);
    let document = content.document.as_ref().unwrap();
    let next = &document.sections[2];
    assert_eq!(next.id.as_str(), "next");
    assert_eq!(next.heading.plain_text(), "NEXT");
    assert_eq!(next.source.unwrap().line, 9);
    let index = mant_ir::DocumentIndex::build(document);
    assert_eq!(index.fragment_target("next"), Some(&next.id));
    let outline = mant_query::build_outline(&content).unwrap();
    let node = outline
        .nodes
        .iter()
        .find(|node| node.id() == "next")
        .unwrap();
    assert_eq!(node.title(), "NEXT");
    let search = mant_query::search_query(
        &content,
        &mant_protocol::SearchQuery {
            pattern: "NEXT".into(),
            syntax: mant_protocol::SearchSyntax::Literal,
            case: mant_protocol::SearchCase::Sensitive,
            scope: mant_protocol::SearchScope::Visible,
            word: true,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .unwrap();
    let [hit] = search.matches.as_slice() else {
        panic!("exactly one heading hit");
    };
    assert_eq!(hit.outline.node.id(), "next");
    assert_eq!(hit.outline.node.title(), "NEXT");
    assert_eq!(
        body(&mant_render::render_query_man(&content)),
        "BEFOREMARK\n\nNEXT\nAFTERMARK"
    );
}

const WORD_MAN: &str =
    ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- word audit\n.SH DESCRIPTION\n";
const WORD_END: &str = ".SH ENDTEST\nFINISH\n";

fn hanging_word_rows(source: &str) -> String {
    let rendered = mant_render::render_query_man(&load(source));
    let (_, body) = rendered.split_once("\nDESCRIPTION\n").unwrap();
    body.strip_suffix("\n\nENDTEST\nFINISH").unwrap().to_owned()
}

#[test]
fn hanging_word_passes_resolve_origins_and_close_the_rejected_tail_once() {
    // Pinned CVS term.c::term_flushln() moves BRIND's next accepted pass
    // to rmargin after the loop endline. The later nbr == 0 tail can end
    // a distinct empty row, even if this flush previously printed text.
    for width in [4, 16] {
        let indent = " ".repeat(width);
        for request in ["", "\n.br"] {
            for (first, next, expected) in [
                (
                    r#".B "ALPHA\zX""#,
                    r#".I "\p BETA""#,
                    format!("ALPHAX BETA\n{indent}OMEGA"),
                ),
                (
                    r#".B "ALPHA\zX""#,
                    r#".I "\p\: \p\& \p BETA""#,
                    if request.is_empty() {
                        "ALPHAX\n".to_owned()
                    } else {
                        format!("ALPHAX\n\n{indent}OMEGA")
                    },
                ),
                (
                    r#".B "ALPHA\zX\z""#,
                    r#".I "\&\p\: \p BETA""#,
                    format!("ALPHAX\n{indent} ETA\n{indent}OMEGA"),
                ),
            ] {
                let source =
                    format!("{WORD_MAN}.HP {width}n\n{first}\n{next}{request}\nOMEGA\n{WORD_END}");
                // A wide short field can keep BETA and OMEGA on the
                // native line at br. The explicit reading boundary keeps
                // OMEGA on the hanging row without adding native padding.
                assert_eq!(hanging_word_rows(&source), expected, "{source}");
            }
        }
    }
}

#[test]
fn alignment_after_an_accepted_hanging_word_pass_rebases_each_origin_once() {
    // pre_br() can enter the group after an earlier term_flushln() pass
    // has already committed a continuation row. Earlier and later rows
    // of the same reading paragraph share one coordinate base.
    for width in [4, 16] {
        let indent = " ".repeat(width);
        let source = format!(
            "{WORD_MAN}.HP {width}n\n.B \"ALPHA\\p BETA\"\n.ce 0\n\\z\n.br\nOMEGA\n{WORD_END}"
        );
        assert_eq!(
            hanging_word_rows(&source),
            format!("ALPHA\n{indent}BETA\n{indent}MEGA"),
            "{source}"
        );
    }
}
