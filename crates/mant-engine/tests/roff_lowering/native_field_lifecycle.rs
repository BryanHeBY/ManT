//! Native field receipts through actual JSON and the terminal consumer.

use libmandoc_rs::{Node, NodeKind, Parser};

const HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn find(node: &Node, predicate: impl Fn(&Node) -> bool + Copy) -> Option<&Node> {
    if predicate(node) {
        Some(node)
    } else {
        node.children
            .iter()
            .find_map(|child| find(child, predicate))
    }
}

fn assert_rows(body: &str, expected: &[&str]) {
    let source = format!("{HEADER}{body}");
    if body.contains(".It Xo\n") {
        // Pristine -Ttree confirms that Xo belongs to It HEAD for these
        // tag/ohang cases; a literal diag label is not the same path.
        let report = Parser::default()
            .parse_bytes("field-lifecycle.1", source.as_bytes())
            .unwrap();
        let item = find(&report.document.root, |node| {
            node.kind == NodeKind::Block && node.macro_name.as_deref() == Some("It")
        })
        .unwrap();
        let head = item
            .children
            .iter()
            .find(|node| node.kind == NodeKind::Head)
            .unwrap();
        assert!(find(head, |node| node.macro_name.as_deref() == Some("Xo")).is_some());
    }
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("load roff");
    let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
    assert!(
        !json.contains("\\u0000mant:field-word:"),
        "private owner in JSON"
    );
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let rendered = mant_render::render_query_man(&decoded.into());
    let rows: Vec<String> = rendered
        .split_once("DESCRIPTION\n")
        .unwrap()
        .1
        .trim_end_matches('\n')
        .lines()
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    assert_eq!(rows, expected, "{source}");
}

#[test]
fn empty_field_vertical_space_has_no_synthetic_line_close() {
    // All 36 exact inputs ran on pristine CVS before these assertions.
    // term_newln() only flushes buffered cells or viscol; term_vspace()
    // then asserts one endline (term.c:475-497). Bare BACKAFTER and an
    // empty word do not occupy a cell, unlike NBRZW and a buffered glyph.
    for style in ["tag", "hang"] {
        for rows in 0usize..=2 {
            for (prefix, graph, bare_zero) in [
                ("", false, false),
                (".No \\&\n", false, false),
                (".No \\z\n", false, true),
                (".No \\zX\n", true, false),
                (".No X\n", true, false),
                (".No \"\"\n", false, false),
            ] {
                let body = format!(
                    ".Bl -{style} -width 4n\n.It Xo\n{prefix}.sp {rows}\n.No AfterWord\n.Xc\n.No BodyWord\n.El\n"
                );
                let word = if bare_zero { "fterWord" } else { "AfterWord" };
                let mut expected = Vec::new();
                if graph && rows > 0 {
                    expected.push("X".to_owned());
                }
                expected.extend(std::iter::repeat_n(
                    String::new(),
                    if graph { rows.saturating_sub(1) } else { rows },
                ));
                let row = format!(
                    "{}{word}{}",
                    if graph && rows == 0 { "X " } else { "" },
                    if style == "hang" { "BodyWord" } else { "" }
                );
                expected.push(row);
                if style == "tag" {
                    expected.push("BodyWord".to_owned());
                }
                assert_rows(
                    &body,
                    &expected.iter().map(String::as_str).collect::<Vec<_>>(),
                );
            }
        }
    }
}

#[test]
fn an_empty_head_sp_keeps_one_requested_row_before_later_words() {
    // These exact sources first ran on pristine CVS, including ASCII and
    // UTF-8. The empty native buffer skips term_newln()'s conditional close
    // (term.c:475-480); this is one executed vertical row, not two.
    for (style, expected) in [
        ("tag", vec!["", "one X", "BodyWord"]),
        ("hang", vec!["", "one X BodyWord"]),
    ] {
        let body = format!(
            ".Bl -{style} -width 4n\n.It Xo\n.sp 1\n.No one\n.No X\n.Xc\n.No BodyWord\n.El\n"
        );
        assert_rows(&body, &expected);
    }
}

#[test]
fn a_literal_body_row_close_supersedes_the_first_word_column_relation() {
    // Both exact sources first ran on pristine CVS -Tascii/-Tutf8/-Tlint.
    // NODE_LINE without NONEWLINE runs term_newln() before the BODY word
    // (mdoc_term.c:314-318). Neither NOSPACE nor the already filled HEAD
    // column can override that physical close at the final IR consumer.
    for style in ["tag", "hang"] {
        let body = format!(
            ".Bl -{style} -width 4n\n.It Xo\n.nf\n.No after space\n.Xc\n.No tail text\n.El\n"
        );
        assert_rows(&body, &["after space", "tail text"]);
    }
}

#[test]
fn pending_tab_reconfiguration_preserves_only_the_current_pass_boundaries() {
    // Exact inputs ran on pristine CVS before these assertions. .ta does
    // not flush (roff_term.c:217-223); term_fill() decides automatic width
    // passes at the eventual flush. The author's in-word \p still executes.
    for (style, rows) in [
        ("tag", vec!["A B", "C D", "BodyWord"]),
        ("hang", vec!["A B", "C D BodyWord"]),
    ] {
        let body = format!(
            ".Bl -{style} -width 4n\n.It Xo\n.br\n.No \"A \tB\\p C D\"\n.ta T 2n\n.Xc\n.No BodyWord\n.El\n"
        );
        assert_rows(&body, &rows);
    }
}

#[test]
fn fill_mode_requests_retire_rejected_glyphs_before_changing_field_flags() {
    // Each exact source ran on pristine CVS -Tascii/-Tutf8/-Ttree/-Tlint.
    // fi/nf share pre_br(), whose term_newln() precedes the BRIND changes
    // (roff_term.c:45-58,69-78). First-pass rejection must clear BACKBEFORE
    // and the pending Y before the new source word executes (term.c:233-237).
    for style in ["tag", "hang"] {
        let body = format!(
            ".Bl -{style} -width 4n\n.It Xo\n.No \\p\n.No \\zY\n.nf\n.No Z\n.Xc\n.No BodyWord\n.El\n"
        );
        assert_rows(&body, &["Z", "BodyWord"]);
    }
}

#[test]
fn the_final_body_gap_uses_the_current_tab_configuration() {
    // All four exact inputs ran on pristine CVS -Tascii/-Tutf8/-Ttree/-Tlint
    // before these assertions. roff_term_pre_ta() changes pending tabs,
    // without flushing; term_field()/term_flushln() determine the final
    // physical column (roff_term.c:217-223, term.c:156-229). That result,
    // rather than a historical projected width, decides the BODY gap.
    for width in [6, 8] {
        for words in [".No \"A\tB C D\"\n", ".No \"A\tB\"\n.No C\n.No D\n"] {
            let body = format!(
                ".Bl -hang -width {width}n\n.It Xo\n.br\n.ta 8n\n{words}.ta 2n\n.Xc\n.No BodyWord\n.El\n"
            );
            assert_rows(&body, &["A B C D BodyWord"]);
        }
    }
}

#[test]
fn responsive_hang_margin_retains_the_explicit_marker_rejection() {
    // Exact inputs first ran on pristine CVS -Tascii/-Tutf8/-Ttree/-Tlint.
    // The frozen matrix retains A for the width-only HANG .mc case. These
    // counterparts still execute authored ESCAPE_BREAK through term_fill()
    // (term.c:294-306), retire its rejected suffix, and preserve Bob after
    // the author's genuine term_newln() (mdoc_term.c:1084-1085).
    for word in [r"\p A", r"\p\& A"] {
        let body = format!(
            ".Bl -hang -width 6n\n.It Xo\n.No LONGTEXT\n.mc\n.No \"{word}\"\n.An -split\n.An Bob\n.Xc\n.No BODY\n.El\n"
        );
        assert_rows(&body, &["LONGTEXT Bob BODY"]);
    }
}

#[test]
fn rejected_pass_never_sweeps_its_tail() {
    // Every exact input below ran with pristine CVS -Tascii/-Tutf8/-Tlint
    // before its expectation was written. term.c:143-146 exits on nbr==0 before the 177-196 BRTRSP sweep.
    // Spaces in that rejected pass cannot manufacture a device overrun.
    let cases: &[(&str, &[&str])] = &[
        (
            ".Bl -tag -width 2n\n.It Xo\n.No \"\\pY\"\n.Xc\n.No BodyWord\n.El\n",
            &["Y BodyWord"],
        ),
        (
            ".Bl -tag -width 2n\n.It Xo\n.No \"\\p Y\"\n.Xc\n.No BodyWord\n.El\n",
            &["BodyWord"],
        ),
        (
            ".Bl -tag -width 2n\n.It Xo\n.No \"\\p    Y\"\n.Xc\n.No BodyWord\n.El\n",
            &["BodyWord"],
        ),
        (
            ".Bl -tag -width 2n\n.It Xo\n.No \"\\p        Y\"\n.Xc\n.No BodyWord\n.El\n",
            &["BodyWord"],
        ),
        (
            ".Bl -tag -width 4n\n.It Xo\n.No \"\\pY\"\n.Xc\n.No BodyWord\n.El\n",
            &["Y BodyWord"],
        ),
        (
            ".Bl -tag -width 4n\n.It Xo\n.No \"\\p Y\"\n.Xc\n.No BodyWord\n.El\n",
            &["BodyWord"],
        ),
        (
            ".Bl -tag -width 4n\n.It Xo\n.No \"\\p    Y\"\n.Xc\n.No BodyWord\n.El\n",
            &["BodyWord"],
        ),
        (
            ".Bl -tag -width 4n\n.It Xo\n.No \"\\p        Y\"\n.Xc\n.No BodyWord\n.El\n",
            &["BodyWord"],
        ),
        (
            ".Bl -tag -width 6n\n.It Xo\n.No \"\\pY\"\n.Xc\n.No BodyWord\n.El\n",
            &["Y BodyWord"],
        ),
        (
            ".Bl -tag -width 6n\n.It Xo\n.No \"\\p Y\"\n.Xc\n.No BodyWord\n.El\n",
            &["BodyWord"],
        ),
        (
            ".Bl -tag -width 6n\n.It Xo\n.No \"\\p    Y\"\n.Xc\n.No BodyWord\n.El\n",
            &["BodyWord"],
        ),
        (
            ".Bl -tag -width 6n\n.It Xo\n.No \"\\p        Y\"\n.Xc\n.No BodyWord\n.El\n",
            &["BodyWord"],
        ),
    ];
    for &(body, expected) in cases {
        assert_rows(body, expected);
    }
}

#[test]
fn tab_settings_and_source_references_control_the_final_field() {
    // Every exact input below ran with pristine CVS -Tascii/-Tutf8/-Tlint
    // before its expectation was written. term_tab.c::term_tab_set/term_tab_next and term.c:327-338,177-196.
    // These filled-tab inputs have the expected mandoc tab warning; the
    // row relationship is still defined. Stops include absolute, periodic,
    // relative, and fractional distances in both simple and extended HEAD.
    let cases: &[(&str, &[&str])] = &[
        (
            ".Bl -tag -width 4n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["X", "BodyWord"],
        ),
        (
            ".Bl -tag -width 4n\n.It Xo\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["X", "BodyWord"],
        ),
        (
            ".ta 2n\n.Bl -tag -width 4n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta 2n\n.Bl -tag -width 4n\n.It Xo\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta 3n\n.Bl -tag -width 4n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta 3n\n.Bl -tag -width 4n\n.It Xo\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta 4n\n.Bl -tag -width 4n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta 4n\n.Bl -tag -width 4n\n.It Xo\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta 8n\n.Bl -tag -width 4n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["X", "BodyWord"],
        ),
        (
            ".ta 8n\n.Bl -tag -width 4n\n.It Xo\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["X", "BodyWord"],
        ),
        (
            ".ta T 3n\n.Bl -tag -width 4n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta T 3n\n.Bl -tag -width 4n\n.It Xo\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta 2n +3n\n.Bl -tag -width 4n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta 2n +3n\n.Bl -tag -width 4n\n.It Xo\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta 2n 4n T 3n +2n\n.Bl -tag -width 4n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta 2n 4n T 3n +2n\n.Bl -tag -width 4n\n.It Xo\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta 2.5n\n.Bl -tag -width 4n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".ta 2.5n\n.Bl -tag -width 4n\n.It Xo\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
    ];
    for &(body, expected) in cases {
        assert_rows(body, expected);
    }
}

#[test]
fn accepted_invisible_head_has_one_physical_row() {
    // Every exact input below ran with pristine CVS -Tascii/-Tutf8/-Tlint
    // before its expectation was written. mdoc_term.c::termp_it_post calls term_newln for ohang HEAD.
    // ASCII_NBRZW is an accepted zero-width graph (term.c:340-349),
    // while bare BACKAFTER or an empty word alone owns no emitted row.
    let cases: &[(&str, &[&str])] = &[
        (
            ".Bl -ohang\n.It Xo\n.No \\&\n.Xc\n.No BodyWord\n.El\n",
            &["", "BodyWord"],
        ),
        (
            ".Bl -ohang\n.It Xo\n.No \\p\\&\n.Xc\n.No BodyWord\n.El\n",
            &["", "BodyWord"],
        ),
        (
            ".Bl -ohang\n.It Xo\n.No \\z\n.Xc\n.No BodyWord\n.El\n",
            &["odyWord"],
        ),
        (
            ".Bl -ohang\n.It Xo\n.No \"\"\n.Xc\n.No BodyWord\n.El\n",
            &["BodyWord"],
        ),
    ];
    for &(body, expected) in cases {
        assert_rows(body, expected);
    }
}

#[test]
fn literal_tabs_persist_until_a_real_reset_handler() {
    // Every exact input below ran with pristine CVS -Tascii/-Tutf8/-Tlint
    // before its expectation was written. termp_bd_pre installs 8n; Bd post does not restore prior tabs.
    // termp_d1_pre and Sh BODY explicitly reset the defaults.
    let cases: &[(&str, &[&str])] = &[
        (
            ".Bl -tag -width 6n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["X BodyWord"],
        ),
        (
            ".Bd -literal\ndisplay\n.Ed\n.Bl -tag -width 6n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["display", "", "X", "BodyWord"],
        ),
        (
            ".Bd -literal\ndisplay\n.Ed\n.Dl display2\n.Bl -tag -width 6n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["display", "display2", "", "X BodyWord"],
        ),
        (
            ".Bd -literal\ndisplay\n.Ed\n.Sh NEXT\n.Bl -tag -width 6n\n.It \"X\t\"\n.No BodyWord\n.El\n",
            &["display", "", "NEXT", "X BodyWord"],
        ),
    ];
    for &(body, expected) in cases {
        assert_rows(body, expected);
    }
}

#[test]
fn source_tab_changes_flush_old_fields_before_applying_new_settings() {
    // Exact pristine CVS ASCII/UTF-8/tree/lint probes preceded this assertion.
    // print_mdoc_node() enters NODE_LINE before roff_term_pre_ta(), and the
    // preceding TAG field's BRTRSP sweep includes its old-default tail tab.
    assert_rows(
        ".nf\n.Bl -tag -width 4n\n.It Xo\n.No \"X\t\"\n.ta 2n\n.No Y\n.Xc\n.No BodyWord\n.El\n",
        &["", "X", "Y", "BodyWord"],
    );
}

#[test]
fn filled_tab_configuration_reinterprets_the_unprinted_field() {
    // Each exact input ran on pristine ASCII/UTF-8/tree/lint first (filled
    // tabs warn). roff_term.c:216-221 changes stops without term_newln();
    // term_fill()327-338 sees that configuration at the actual HEAD flush.
    // A word-time scan prefix cannot freeze the previous Tab width.
    for style in ["tag", "hang"] {
        for stop in ["T 2n", "T 8n"] {
            let body = format!(
                ".Bl -{style} -width 4n\n.It Xo\n.br\n.No \"A \tB\\p C\"\n.ta {stop}\n.Xc\n.No BODY\n.El\n"
            );
            let expected = match (style, stop) {
                ("tag", "T 2n") => vec!["A B", "C", "BODY"],
                ("tag", _) => vec!["A", "B", "C", "BODY"],
                (_, "T 2n") => vec!["A B", "C BODY"],
                _ => vec!["A", "B", "C BODY"],
            };
            assert_rows(&body, &expected);
        }
    }
}

#[test]
fn fractional_tab_advances_update_each_printed_device_segment() {
    // Every complete source ran on pristine CVS ASCII/UTF-8/tree/lint first.
    // term_field() advances each blank segment independently;
    // term_ascii.c::ascii_advance() rounds at each call, not after the whole
    // field. These printed columns decide the next .mc's vfield and whether
    // Q belongs to an accepted row. Filled-tab warnings are expected.
    let cases: &[(&str, &[&str])] = &[
        (
            ".ta T 2.75n\n.Bl -tag -width 12n\n.It Xo\n.No \"A\tB\tC\"\n.mc\n.No \"D\tE\"\n.mc\n.No Q\n.Xc\n.No BODY\n.El\n",
            &["A B C D E", "Q", "BODY"],
        ),
        (
            ".ta T 2.75n\n.Bl -tag -width 13n\n.It Xo\n.No \"A\tB\tC\"\n.mc\n.No \"D\tE\"\n.mc\n.No Q\n.Xc\n.No BODY\n.El\n",
            &["A B C D E", "BODY"],
        ),
        (
            ".ta T 2.5n\n.Bl -tag -width 11n\n.It Xo\n.No \"A\tB\tC\"\n.mc\n.No \"D\tE\"\n.mc\n.No Q\n.Xc\n.No BODY\n.El\n",
            &["A B C D E", "BODY"],
        ),
    ];
    for &(body, expected) in cases {
        assert_rows(body, expected);
    }
}
