//! Display pre handlers use native facts in both BODY and definition HEAD.

use libmandoc_rs::{DisplayKind, Node, NodeKind, Parser};

const HEADER: &str =
    ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

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
        let report = Parser::default()
            .parse_bytes("display-tabs.1", source.as_bytes())
            .unwrap();
        let item = find(&report.document.root, |node| {
            node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("It")
        })
        .unwrap();
        let head = item
            .children
            .iter()
            .find(|node| node.kind == NodeKind::Head)
            .unwrap();
        // Exact pristine -Ttree probes confirmed that these displays remain
        // within It HEAD. Ordinary BODY display coverage is insufficient.
        assert!(
            find(head, |node| {
                node.kind == NodeKind::Block
                    && matches!(node.macro_token.as_deref(), Some("D1" | "Dl" | "Bd"))
            })
            .is_some(),
            "display is not in the tested HEAD: {source}"
        );
    }
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
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
fn normalized_unfilled_and_literal_keep_distinct_tab_execution() {
    // These exact inputs ran on pristine CVS -Tascii/-Tutf8/-Ttree/-Tlint
    // before the assertions were added; filled tab warnings are expected.
    // mdoc_term.c:1458-1463 changes tabs for DISP_literal alone. Both kinds
    // preserve source rows at Bd post (1477-1484), which does not reset tabs.
    for (kind, expected_kind, width, prefix, expected) in [
        (
            "unfilled",
            DisplayKind::Unfilled,
            6,
            "",
            vec!["display", "", "X BodyWord"],
        ),
        (
            "literal",
            DisplayKind::Literal,
            6,
            "",
            vec!["display", "", "X", "BodyWord"],
        ),
        (
            "unfilled",
            DisplayKind::Unfilled,
            4,
            ".ta 2n\n",
            vec!["display", "", "X BodyWord"],
        ),
        (
            "literal",
            DisplayKind::Literal,
            4,
            ".ta 2n\n",
            vec!["display", "", "X", "BodyWord"],
        ),
    ] {
        let body = format!(
            "{prefix}.Bd -{kind}\ndisplay\n.Ed\n.Bl -tag -width {width}n\n.It \"X\t\"\n.No BodyWord\n.El\n"
        );
        let source = format!("{HEADER}{body}");
        let report = Parser::default()
            .parse_bytes("display-kind.1", source.as_bytes())
            .unwrap();
        let block = find(&report.document.root, |node| {
            node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("Bd")
        })
        .unwrap();
        assert_eq!(block.display_kind, Some(expected_kind));
        let display_body = block
            .children
            .iter()
            .find(|node| node.kind == NodeKind::Body)
            .unwrap();
        assert_eq!(display_body.display_kind, Some(expected_kind));
        assert_rows(&body, &expected);
    }
}

#[test]
fn display_tabs_execute_in_definition_heads() {
    // Each exact input ran on pristine CVS ASCII/UTF-8/tree/lint first.
    // termp_d1_pre() (1324-1334) resets only on BLOCK after term_newln;
    // termp_bd_pre() (1431-1463) switches only a literal BODY. Destination
    // changes do not replace these handlers with generic inline children.
    let cases: &[(&str, &[&str])] = &[
        (
            ".Bl -tag -width 4n\n.It Xo\n.ta 2n\n.D1 display\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["display", "X", "BodyWord"],
        ),
        (
            ".Bl -tag -width 4n\n.It Xo\n.ta 2n\n.Dl display\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["display", "X", "BodyWord"],
        ),
        (
            ".Bl -tag -width 6n\n.It Xo\n.ta 8n\n.D1 display\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["display", "X BodyWord"],
        ),
        (
            ".Bl -tag -width 6n\n.It Xo\n.ta 8n\n.Dl display\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["display", "X BodyWord"],
        ),
        (
            ".Bl -tag -width 4n\n.It Xo\n.ta 2n\n.Bd -literal -compact\ndisplay\n.Ed\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["display", "X", "BodyWord"],
        ),
        (
            ".Bl -tag -width 4n\n.It Xo\n.ta 2n\n.Bd -unfilled -compact\ndisplay\n.Ed\n.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n",
            &["display", "X BodyWord"],
        ),
    ];
    for &(body, expected) in cases {
        assert_rows(body, expected);
    }
}

#[test]
fn display_reset_runs_after_the_old_field_is_flushed() {
    // Pristine ASCII/UTF-8/tree/lint ran this exact source first. D1/Dl
    // pre calls term_newln() before default tab reset (1328-1333). X's
    // pending trailing tab therefore uses 2n, keeping display on its row;
    // applying the default 5n first would end that row prematurely.
    assert_rows(
        ".Bl -tag -width 4n\n.It Xo\n.ta 2n\n.No \"X\t\"\n.Dl display\n.Xc\n.No BodyWord\n.El\n",
        &["X display", "BodyWord"],
    );
}

#[test]
fn crossed_display_post_executes_at_the_original_body_marker() {
    // Exact pristine ASCII/UTF-8/tree/lint probes preceded these assertions.
    // Native reports the deliberate bad nesting and filled-tab warnings.
    // print_mdoc_node() restores the original BODY's font before Bd post,
    // marks it ENDED, and skips post at normal return (mdoc_term.c:409-429).
    // The marker's term_newln(), not the enclosing Rust helper return,
    // closes display's current field (termp_bd_post():1474-1486).
    for (kind, expected) in [
        ("literal", vec!["[ display", "X ]", "BodyWord"]),
        ("unfilled", vec!["[ display", "X ] BodyWord"]),
    ] {
        assert_rows(
            &format!(
                ".Bl -tag -width 4n\n.It Xo\n.ta 2n\n.Bd -{kind} -compact\n.Bo\ndisplay\n.Ed\n.No \"X\t\"\n.Bc\n.Xc\n.No BodyWord\n.El\n"
            ),
            &expected,
        );
    }
}

#[test]
fn display_vertical_space_uses_the_original_head_execution_context() {
    // Each exact source ran through pristine CVS ASCII/UTF-8/tree/lint
    // before these assertions. All ten inputs have clean lint output.
    // print_bvspace() (mdoc_term.c:583-628) stops at a non-item It ancestor
    // even for its first HEAD child, but stops without vspace at Sh/Ss.
    // Compact suppresses its term_vspace(); negative .sp debt consumes it.
    // This is not roff .sp: it must preserve BRIND/NOBREAK and end HANG's
    // physical row without applying roff_term_pre_br()'s flag changes.
    let cases: &[(&str, &[&str])] = &[
        (".Bd -literal\n.No Beta\n.Ed\n", &["Beta"]),
        (
            ".No Alpha\n.Bd -literal\n.No Beta\n.Ed\n",
            &["Alpha", "", "Beta"],
        ),
        (
            ".Bl -inset\n.It Xo\n.Bd -literal\n.No Beta\n.Ed\n.Xc\n.No BodyWord\n.El\n",
            &["", "Beta", "BodyWord"],
        ),
        (
            ".Bl -inset\n.It Xo\n.No Alpha\n.Bd -literal\n.No Beta\n.Ed\n.Xc\n.No BodyWord\n.El\n",
            &["Alpha", "", "Beta", "BodyWord"],
        ),
        (
            ".Bl -inset\n.It Xo\n.No Alpha\n.Bd -literal -compact\n.No Beta\n.Ed\n.Xc\n.No BodyWord\n.El\n",
            &["Alpha", "Beta", "BodyWord"],
        ),
        (
            ".Bl -tag -width 4n\n.It Xo\n.Bd -literal\n.No Beta\n.Ed\n.Xc\n.No BodyWord\n.El\n",
            &["", "Beta", "BodyWord"],
        ),
        (
            ".Bl -tag -width 4n\n.It Xo\n.No Alpha\n.Bd -literal\n.No Beta\n.Ed\n.Xc\n.No BodyWord\n.El\n",
            &["Alpha", "", "Beta", "BodyWord"],
        ),
        (
            ".Bl -hang -width 4n\n.It Xo\n.No Alpha\n.Bd -literal\n.No Beta\n.Ed\n.Xc\n.No BodyWord\n.El\n",
            &["Alpha", "Beta BodyWord"],
        ),
        (
            ".Bl -inset\n.It Xo\n.No Alpha\n.sp -1\n.Bd -literal\n.No Beta\n.Ed\n.Xc\n.No BodyWord\n.El\n",
            &["Alpha", "Beta", "BodyWord"],
        ),
        (
            ".Bl -inset\n.It Xo\n.No Alpha\\c\n.Bd -literal\n.No Beta\n.Ed\n.Xc\n.No BodyWord\n.El\n",
            &["Alpha", "", "Beta", "BodyWord"],
        ),
    ];
    for &(body, expected) in cases {
        assert_rows(body, expected);
    }
}

#[test]
fn macro_pre_preserves_vertical_debt_until_an_actual_formatter_word() {
    // Every exact input ran on pristine ASCII/UTF-8/tree/lint first.
    // mdoc_term.c::print_mdoc_node() executes macro pre before children;
    // term.c:573-589 clears skipvsp only inside term_word(). The display
    // pre and transparent Xo/Bf/Bk pre therefore cannot consume the debt
    // merely because their descendants include a later visible operand.
    for display in [
        ".D1 Beta\n",
        ".Dl Beta\n",
        ".Bd -literal\n.No Beta\n.Ed\n",
        ".Xo\n.Bd -literal\n.No Beta\n.Ed\n.Xc\n",
        ".Bf -emphasis\n.Bd -literal\n.No Beta\n.Ed\n.Ef\n",
        ".Bk -words\n.Bd -literal\n.No Beta\n.Ed\n.Ek\n",
    ] {
        assert_rows(
            &format!(".Bl -inset\n.It Xo\n.No Alpha\n.sp -1\n{display}.Xc\n.No BodyWord\n.El\n"),
            &["Alpha", "Beta", "BodyWord"],
        );
    }
    // These state-only macros execute no word. No's explicit empty and
    // font-only operands do execute term_word() and clear debt; \& also
    // occupies a native row, so Bd's newline preserves that additional row.
    for (middle, expected) in [
        (".ft B\n", vec!["Alpha", "Beta", "BodyWord"]),
        (".Sm off\n", vec!["Alpha", "Beta", "BodyWord"]),
        (".An -split\n", vec!["Alpha", "Beta", "BodyWord"]),
        (".ta 2n\n", vec!["Alpha", "Beta", "BodyWord"]),
        (".Ns\n", vec!["Alpha", "Beta", "BodyWord"]),
        (".No \"\"\n", vec!["Alpha", "", "Beta", "BodyWord"]),
        (".No \\fB\n", vec!["Alpha", "", "Beta", "BodyWord"]),
        (".No \\&\n", vec!["Alpha", "", "", "Beta", "BodyWord"]),
    ] {
        assert_rows(
            &format!(
                ".Bl -inset\n.It Xo\n.No Alpha\n.sp -1\n{middle}.Bd -literal\n.No Beta\n.Ed\n.Xc\n.No BodyWord\n.El\n"
            ),
            &expected,
        );
    }
}
