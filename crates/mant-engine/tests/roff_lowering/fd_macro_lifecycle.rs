//! Fd post executes native `term_newln` once, independently of its IR sink.

use libmandoc_rs::{Node, Parser};
use mant_ir::{
    Inline,
    visit::{self, Visit},
};

const HEADER: &str = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n";

fn description_rows(source: &str) -> Vec<String> {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("load Fd source");
    let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
    // Use actual text decoding; from_value does not exercise JSON depth.
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let rendered = mant_render::render_query_man(&decoded.into());
    let body = rendered
        .split_once("DESCRIPTION\n")
        .unwrap()
        .1
        .split_once("\nNEXT\n")
        .unwrap()
        .0
        .trim_matches('\n');
    body.lines()
        .map(|line| line.trim_start().replace('\u{a0}', " "))
        .collect()
}

fn assert_description_rows(source: &str, expected: &[&str]) {
    assert_eq!(description_rows(source), expected, "{source}");
}

#[test]
fn fd_post_commits_the_row_origin_before_the_outer_scope_returns() {
    // Four exact pristine UTF-8/ASCII/tree/lint runs first. Fd post flushes
    // before print_mdoc_node restores offset (mdoc_term.c:419,437-439), so
    // its jump and minbl have committed. Cd lacks that post; HEAD's later
    // flush happens after Xo returns. Mode-only An executes no new word.
    let mut failures = Vec::new();
    for (suffix, expected) in [("", "X     BBODY"), (".An -split\n.No C\n", "X     BCBODY")] {
        let source = format!(
            "{HEADER}.Bl -hang -width 4n\n.It Xo\n.No X\n.br\n.Fd B\n{suffix}.Xc\n.No BODY\n.El\n.Sh NEXT\n.No END\n"
        );
        let actual = description_rows(&source);
        if actual != [expected] {
            failures.push(format!(
                "{source}\nactual {actual:?}; expected {expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn cd_without_a_post_keeps_its_later_word_boundaries_in_the_same_row() {
    // Both exact sources ran pristine CVS before these assertions (same
    // evidence as the Fd counterexamples above). Cd's delayed field origin
    // falls within the frozen G-IND responsive geometry: the oracle rows
    // are "X B   BODY" and "X B C BODY". This dedicated control allows
    // only the number of padding spaces to differ; it still pins one physical
    // row, every glyph, their order and every native word boundary. In the
    // Fd counterparts above the post already consumed the BODY separator,
    // so BBODY/CBODY join; importing that post into Cd fails here.
    for (suffix, words) in [
        ("", &["X", "B", "BODY"][..]),
        (".An -split\n.No C\n", &["X", "B", "C", "BODY"][..]),
    ] {
        let source = format!(
            "{HEADER}.Bl -hang -width 4n\n.It Xo\n.No X\n.br\n.Cd B\n{suffix}.Xc\n.No BODY\n.El\n.Sh NEXT\n.No END\n"
        );
        let rows = description_rows(&source);
        assert_eq!(rows.len(), 1, "{source}\n{rows:?}");
        let mut remaining = rows[0].as_str();
        for (index, word) in words.iter().enumerate() {
            if index > 0 {
                let after_padding = remaining.trim_start_matches(' ');
                assert!(
                    after_padding.len() < remaining.len(),
                    "lost word boundary: {rows:?}"
                );
                remaining = after_padding;
            }
            remaining = remaining
                .strip_prefix(word)
                .unwrap_or_else(|| panic!("lost or reordered {word:?}: {source}\n{rows:?}"));
        }
        assert!(remaining.is_empty(), "extra glyphs: {source}\n{rows:?}");
    }
}

#[test]
fn fd_post_uses_the_same_empty_and_pending_word_rules_in_each_sink() {
    // All 36 exact inputs ran first with pristine CVS -Tutf8/-Tascii/
    // -Ttree/-Tlint, recorded in target/audits/nf05-expanded-before-tests.json.
    // print_mdoc_node() pops the font, then termp_fd_post() calls term_newln
    // (mdoc_term.c:409-421,1255-1265). An empty native buffer closes nothing;
    // bare BACKAFTER survives, unlike buffered NBRZW/\p/\zX (term.c:475-480).
    let mut failures = Vec::new();
    for (operand, filled, no_fill) in [
        (
            "",
            &["BEFORE AFTER TAIL"][..],
            &["BEFORE", "AFTER", "TAIL"][..],
        ),
        (
            "\"\"",
            &["BEFORE", "AFTER TAIL"][..],
            &["BEFORE", "AFTER", "TAIL"][..],
        ),
        (
            r#""\&""#,
            &["BEFORE", "AFTER TAIL"][..],
            &["BEFORE", "", "AFTER", "TAIL"][..],
        ),
        (
            r#""\z""#,
            &["BEFORE", "AFTER TAIL"][..],
            &["BEFORE", "FTER", "TAIL"][..],
        ),
        (
            r#""\zX""#,
            &["BEFORE X", "AFTER TAIL"][..],
            &["BEFORE", "X", "AFTER", "TAIL"][..],
        ),
        (
            r#""\p""#,
            &["BEFORE", "AFTER TAIL"][..],
            &["BEFORE", "", "AFTER", "TAIL"][..],
        ),
        (
            r#""\p\p""#,
            &["BEFORE", "AFTER TAIL"][..],
            &["BEFORE", "", "AFTER", "TAIL"][..],
        ),
        (
            r#""A\p""#,
            &["BEFORE A", "AFTER TAIL"][..],
            &["BEFORE", "A", "AFTER", "TAIL"][..],
        ),
        (
            r#""A\p\p""#,
            &["BEFORE A", "AFTER TAIL"][..],
            &["BEFORE", "A", "AFTER", "TAIL"][..],
        ),
        (
            r#""A\c""#,
            &["BEFORE A", "AFTER TAIL"][..],
            &["BEFORE", "A", "AFTER", "TAIL"][..],
        ),
        (
            r#""\zX\c""#,
            &["BEFORE X", "AFTER TAIL"][..],
            &["BEFORE", "X", "AFTER", "TAIL"][..],
        ),
        (
            r#""\fIA\c\fP""#,
            &["BEFORE A", "AFTER TAIL"][..],
            &["BEFORE", "A", "AFTER", "TAIL"][..],
        ),
    ] {
        for (start, end, expected) in [
            ("", "", filled),
            (".nf\n", ".fi\n", no_fill),
            (".Bd -literal -compact\n", ".Ed\n", no_fill),
        ] {
            let source = format!(
                "{HEADER}.No BEFORE\n{start}.Fd {operand}\n.No AFTER\n{end}.No TAIL\n.Sh NEXT\n.No END\n"
            );
            let actual = description_rows(&source);
            if actual != expected {
                failures.push(format!(
                    "{source}\nactual {actual:?}; expected {expected:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

fn contains_macro(node: &Node, name: &str) -> bool {
    node.macro_name.as_deref() == Some(name)
        || node.children.iter().any(|node| contains_macro(node, name))
}

#[test]
fn tbl_raw_text_is_not_a_second_mdoc_macro_execution_entry() {
    // Exact source ran pristine UTF-8/tree/lint before this assertion.
    // tbl_data.c::tbl_cdata concatenates T{ text; tbl_term.c::tbl_word executes
    // one formatter word. There is no Fd AST node or termp_fd_post in this cell.
    let source = format!(
        "{HEADER}.TS\nl.\nT{{\n.Fd \"A\\c\"\n.No B\nT}}\n.TE\n.No BODY\n.Sh NEXT\n.No END\n"
    );
    let parsed = Parser::default()
        .parse_bytes("fd-tbl.1", source.as_bytes())
        .unwrap();
    assert!(!contains_macro(&parsed.document.root, "Fd"));
    assert_description_rows(&source, &["\"A\" B", "BODY"]);
}

#[test]
fn fd_font_scope_does_not_leak_to_following_body() {
    struct FontRuns {
        strong: Vec<String>,
    }
    impl<'ir> Visit<'ir> for FontRuns {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Strong { children } = inline {
                self.strong.push(mant_ir::inline_plain_text(children));
            }
            visit::walk_inline(self, inline);
        }
    }
    // Original MP01 exact source ran pristine before this assertion:
    // mdoc_term.c::print_mdoc_node restores font depth before Fd post.
    let source = include_str!("review25_matrix/cases/mp01.1");
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let mut runs = FontRuns { strong: Vec::new() };
    visit::walk_document(&mut runs, query.document.as_ref().unwrap());
    assert!(
        runs.strong.iter().any(|value| value == "XSHARED"),
        "{:?}",
        runs.strong
    );
    assert!(
        !runs.strong.iter().any(|value| value.contains('B')),
        "{:?}",
        runs.strong
    );
}
