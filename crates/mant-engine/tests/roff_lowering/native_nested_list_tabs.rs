//! Column-list pre/post executes even inside a definition HEAD output owner.

use libmandoc_rs::{DefinitionListStyle, Node, NodeKind, NormalizedListKind, Parser};

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

fn assert_head_list_rows(
    before: &str,
    inner: &str,
    kind: NormalizedListKind,
    style: Option<DefinitionListStyle>,
    expected: &[&str],
) {
    let source = format!(
        "{HEADER}.Bl -tag -width 4n\n.It Xo\n.ta 2n\n{before}{inner}.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n"
    );
    let report = Parser::default()
        .parse_bytes("head-column.1", source.as_bytes())
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
    assert!(
        find(head, |node| {
            node.kind == NodeKind::Block
                && node.list_kind == Some(kind)
                && node.definition_list_style == style
        })
        .is_some(),
        "the inner list escaped the tested HEAD: {source}"
    );

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
        // Only horizontal device geometry is responsive. Do not trim,
        // filter, or coalesce empty rows: each native post owns its row.
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    assert_eq!(rows, expected, "{source}");
}

#[test]
fn requests_before_a_nested_item_execute_at_their_actual_node_entry() {
    // The nine exact sources ran on pristine CVS -Tascii/-Tutf8/-Ttree
    // before these assertions. Bl BODY requests precede It entry, whose
    // print_bvspace() follows them (mdoc_term.c:583-628); an empty-buffer
    // sp has only its requested endline (term.c:475-497), while nf changes
    // the actual later NODE_LINE entries (mdoc_term.c:314-325).
    for (option, style) in [
        ("tag -width 4n", DefinitionListStyle::Tag),
        ("hang -width 4n", DefinitionListStyle::Hang),
        ("inset", DefinitionListStyle::Inset),
    ] {
        for (control, rows) in [
            (".br\n", vec!["", "one two", "X", "BodyWord"]),
            (".nf\n", vec!["", "one", "two", "X", "BodyWord"]),
            (".sp 1\n", vec!["", "", "one two", "X", "BodyWord"]),
        ] {
            let inner = format!(".Bl -{option}\n{control}.It one\n.No two\n.El\n");
            assert_head_list_rows(
                "",
                &inner,
                NormalizedListKind::Definition,
                Some(style),
                &rows,
            );
        }
    }
}

#[test]
fn column_list_phases_and_tab_reset_do_not_depend_on_output_owner() {
    // Every exact input ran first through the pristine pinned -Tutf8 oracle.
    // Its -Ttree probe confirmed the original nested-list HEAD ownership;
    // filled tabs issue the expected parser warning.
    // mdoc_term.c::termp_bl_pre() flushes the old field, It BODY post calls
    // term_flushln(), and Bl BLOCK post calls term_newln() before default
    // tab reset (1129-1151,936-963). print_bvspace() preserves the initial
    // HEAD item row unless compact or a negative .sp consumes it (583-628).
    for (before, inner, rows) in [
        (
            "",
            ".Bl -column one\n.It one\n.El\n",
            vec!["", "one", "X", "BodyWord"],
        ),
        (
            "",
            ".Bl -column -compact one\n.It one\n.El\n",
            vec!["one", "X", "BodyWord"],
        ),
        (
            "",
            ".Bl -column one two\n.It one Ta two\n.It three Ta four\n.El\n",
            vec!["", "one two", "three four", "X", "BodyWord"],
        ),
        (
            "",
            ".Bl -column -compact one two\n.It one Ta two\n.It three Ta four\n.El\n",
            vec!["one two", "three four", "X", "BodyWord"],
        ),
        (
            ".nf\n",
            ".Bl -column one\n.It one\n.El\n",
            vec!["", "one", "X", "BodyWord"],
        ),
        (
            ".sp -1\n",
            ".Bl -column one\n.It one\n.El\n",
            vec!["one", "X", "BodyWord"],
        ),
        (
            "",
            ".Bl -column one\n.It \\&\n.El\n",
            vec!["", "", "X", "BodyWord"],
        ),
        (
            "",
            ".Bl -column one\n.It \"\"\n.El\n",
            vec!["", "", "X", "BodyWord"],
        ),
    ] {
        assert_head_list_rows(before, inner, NormalizedListKind::Column, None, &rows);
    }
}

#[test]
fn noncolumn_list_phases_share_the_head_formatter_and_source_rows() {
    // These 20 exact inputs ran through the pristine pinned UTF-8 oracle
    // before their assertions were written. -Ttree confirms a nested Bl
    // remains in the outer It HEAD (also checked against the owned AST).
    // mdoc_term.c::termp_it_pre() skips item/marker HEAD children, executes
    // generated markers or inset/diag fixed words, and applies pad flags
    // before traversal (625-933). Its post flushes per kind then clears the
    // shared flags (936-963). NODE_LINE/no-fill is executed at each actual
    // child entry (314-325), including BODY text with a different owner.
    for (option, kind, style, filled, unfilled) in [
        (
            "inset",
            NormalizedListKind::Definition,
            Some(DefinitionListStyle::Inset),
            "one two",
            vec!["one", "two"],
        ),
        (
            "diag",
            NormalizedListKind::Definition,
            Some(DefinitionListStyle::Diagnostic),
            "one two",
            vec!["one", "two"],
        ),
        (
            "hang -width 4n",
            NormalizedListKind::Definition,
            Some(DefinitionListStyle::Hang),
            "one two",
            vec!["one", "two"],
        ),
        (
            "tag -width 4n",
            NormalizedListKind::Definition,
            Some(DefinitionListStyle::Tag),
            "one two",
            vec!["one", "two"],
        ),
        (
            "ohang",
            NormalizedListKind::Definition,
            Some(DefinitionListStyle::Overhang),
            "one two",
            vec!["one", "two"],
        ),
        ("item", NormalizedListKind::Plain, None, "two", vec!["two"]),
        (
            "bullet",
            NormalizedListKind::Bullet,
            None,
            "• two",
            vec!["•", "two"],
        ),
        (
            "dash",
            NormalizedListKind::Dash,
            None,
            "- two",
            vec!["-", "two"],
        ),
        (
            "hyphen",
            NormalizedListKind::Dash,
            None,
            "- two",
            vec!["-", "two"],
        ),
        (
            "enum",
            NormalizedListKind::Ordered,
            None,
            "1. two",
            vec!["1.", "two"],
        ),
    ] {
        let inner = format!(".Bl -{option}\n.It one\n.No two\n.El\n");
        assert_head_list_rows("", &inner, kind, style, &["", filled, "X", "BodyWord"]);
        let mut rows = vec![""];
        rows.extend(unfilled);
        rows.extend(["X", "BodyWord"]);
        assert_head_list_rows(".nf\n", &inner, kind, style, &rows);
    }
}

#[test]
fn nested_item_post_clears_flags_before_the_next_item() {
    // Exact UTF-8 oracle probes preceded these assertions. It HEAD/BODY
    // post clears the global list flags, even when HEAD post flushed no
    // bytes (mdoc_term.c:961-963). A second ohang/tag HEAD therefore no
    // longer borrows the enclosing tag's NOBREAK/trailspace. Compact only
    // removes print_bvspace's vertical request (583-628).
    for (option, kind, style, expected) in [
        (
            "inset",
            NormalizedListKind::Definition,
            Some(DefinitionListStyle::Inset),
            vec!["one two", "three four", "X", "BodyWord"],
        ),
        (
            "diag",
            NormalizedListKind::Definition,
            Some(DefinitionListStyle::Diagnostic),
            vec!["one two", "three four", "X", "BodyWord"],
        ),
        (
            "hang -width 4n",
            NormalizedListKind::Definition,
            Some(DefinitionListStyle::Hang),
            vec!["one two", "three four", "X", "BodyWord"],
        ),
        (
            "tag -width 4n",
            NormalizedListKind::Definition,
            Some(DefinitionListStyle::Tag),
            vec!["one two", "three", "four", "X", "BodyWord"],
        ),
        (
            "ohang",
            NormalizedListKind::Definition,
            Some(DefinitionListStyle::Overhang),
            vec!["one two", "three", "four", "X", "BodyWord"],
        ),
        (
            "item",
            NormalizedListKind::Plain,
            None,
            vec!["two", "four", "X", "BodyWord"],
        ),
        (
            "bullet",
            NormalizedListKind::Bullet,
            None,
            vec!["• two", "• four", "X", "BodyWord"],
        ),
        (
            "dash",
            NormalizedListKind::Dash,
            None,
            vec!["- two", "- four", "X", "BodyWord"],
        ),
        (
            "enum",
            NormalizedListKind::Ordered,
            None,
            vec!["1. two", "2. four", "X", "BodyWord"],
        ),
    ] {
        let inner = format!(".Bl -{option} -compact\n.It one\n.No two\n.It three\n.No four\n.El\n");
        assert_head_list_rows("", &inner, kind, style, &expected);
    }
}

#[test]
fn body_fixed_words_replace_only_the_interval_they_actually_represent() {
    // These five complete inputs ran pristine: ASCII/UTF-8/HTML/tree pass;
    // lint reports the existing filled-text TAB warning in the outer HEAD.
    // It HEAD post retains minbl (term.c:235), then BODY pre selects NOSPACE
    // (mdoc_term.c:752-777). HANG generates no fixed word: its retained minbl
    // still separates one/two. Inset/diag instead emit one/two fixed cells,
    // so those cells alone represent the interval without a second pad.
    // The responsive HANG field retains its one-cell word separator; its
    // pristine three-cell BRIND positioning remains recorded independently.
    let cases = [
        ("hang -width 4n", "", false, "one two"),
        ("hang -width 4n", ".br\n", false, "one two"),
        ("hang -width 4n -compact", "", true, "one two"),
        ("inset", "", false, "one two"),
        ("diag", "", false, "one  two"),
    ];
    for (option, before_item, repeated, expected) in cases {
        let later = if repeated {
            ".It three\n.No four\n"
        } else {
            ""
        };
        let inner = format!(".Bl -{option}\n{before_item}.It one\n.No two\n{later}.El\n");
        let source = format!(
            "{HEADER}.Bl -tag -width 4n\n.It Xo\n.ta 2n\n{inner}.No \"X\t\"\n.Xc\n.No BodyWord\n.El\n"
        );
        let style = if option == "inset" {
            DefinitionListStyle::Inset
        } else if option == "diag" {
            DefinitionListStyle::Diagnostic
        } else {
            DefinitionListStyle::Hang
        };
        let expected_rows = if repeated {
            vec!["one two", "three four", "X", "BodyWord"]
        } else {
            vec!["", "one two", "X", "BodyWord"]
        };
        // The shared helper proves the actual nested Bl is in It HEAD,
        // rather than assuming its placement from the source template.
        assert_head_list_rows(
            "",
            &inner,
            NormalizedListKind::Definition,
            Some(style),
            &expected_rows,
        );
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        assert!(!json.contains("\\u0000mant:"), "{source}");
        assert!(
            !json.contains("onetwo") && !json.contains("threefour"),
            "word identity merged: {source}"
        );
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let output = mant_render::render_query_man(&restored.into());
        let (_, rows) = output.split_once("DESCRIPTION\n").unwrap();
        let leading = if repeated { "" } else { "\n" };
        let later = if repeated { "three four\n" } else { "" };
        assert_eq!(
            rows,
            // render_query_man omits the final presentation delimiter;
            // every internal row and its horizontal cells remain exact.
            format!("{leading}{expected}\n{later}X\t\n      BodyWord"),
            "{source}"
        );
        for word in ["one", "two", "BodyWord"] {
            assert_eq!(output.matches(word).count(), 1, "{word}: {source}");
        }
    }
}

#[test]
fn nested_empty_body_controls_inter_item_vertical_space() {
    // print_bvspace() suppresses only the gap after an empty diag BODY;
    // other kinds still execute term_vspace() (mdoc_term.c:623-628).
    // HEAD pre also gives a bodyless tag HANG, then BODY post clears it
    // (812-814,961-963). All three exact cases ran first with the oracle.
    for (option, style, expected) in [
        (
            "diag",
            DefinitionListStyle::Diagnostic,
            vec!["", "one", "three four", "X", "BodyWord"],
        ),
        (
            "inset",
            DefinitionListStyle::Inset,
            vec!["", "one", "", "three four", "X", "BodyWord"],
        ),
        (
            "tag -width 4n",
            DefinitionListStyle::Tag,
            vec!["", "one", "", "three", "four", "X", "BodyWord"],
        ),
    ] {
        let inner = format!(".Bl -{option}\n.It one\n.It three\n.No four\n.El\n");
        assert_head_list_rows(
            "",
            &inner,
            NormalizedListKind::Definition,
            Some(style),
            &expected,
        );
    }
}

#[test]
fn sibling_nested_lists_keep_the_shared_execution_entry_after_post() {
    // The previous list's It post clears flags, not the surrounding
    // definition's execution context. These exact oracle inputs exercise
    // another inset Bl after either an inset or a column Bl returned.
    for (first, kind, style, expected) in [
        (
            ".Bl -inset -compact\n.It one\n.No two\n.El\n",
            NormalizedListKind::Definition,
            Some(DefinitionListStyle::Inset),
            vec!["one two", "three four", "X", "BodyWord"],
        ),
        (
            ".Bl -column -compact one\n.It one\n.El\n",
            NormalizedListKind::Column,
            None,
            vec!["one", "three four", "X", "BodyWord"],
        ),
    ] {
        let inner = format!("{first}.Bl -inset -compact\n.It three\n.No four\n.El\n");
        assert_head_list_rows("", &inner, kind, style, &expected);
    }
}
