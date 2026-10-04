//! Physical row and origin contracts; blank rows and padding stay observable.

fn source(body: &str) -> String {
    format!(
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n{body}"
    )
}

fn description_rows(body: &str) -> Vec<String> {
    let query = mant_loader::load_roff_bytes(source(body).as_bytes()).expect("lower definition");
    let json =
        serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).expect("serialize query");
    assert!(
        !json.contains("\\u0000mant:field-word:"),
        "private execution owner leaked into JSON"
    );
    let decoded: mant_protocol::QueryBundle =
        serde_json::from_str(&json).expect("real JSON roundtrip");
    let text = mant_render::render_query_man(&decoded.into());
    text.split_once("DESCRIPTION\n")
        .expect("description section")
        .1
        .trim_end_matches('\n')
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn future_fill_mode_does_not_rewrite_the_head_row() {
    // Exact input verified with pristine CVS -Tascii/-Tutf8/-Tlint.
    // mdoc_term.c::print_mdoc_node() executes NODE_NOFILL/NODE_LINE at
    // each node entry; a later .nf cannot alter an earlier BODY .br.
    assert_eq!(
        description_rows(".Bl -inset\n.It Xo X\n.Xc\n.br\n.No BodyWord\n.nf\n.No AfterWord\n.El\n"),
        ["X", "BodyWord", "AfterWord"]
    );
}

#[test]
fn buffered_row_origin_is_fixed_at_flush_after_scope_restore() {
    // Both complete inputs verified with pristine CVS before assertions.
    // mdoc_term.c:314 flushes at source entry, then 329 and 437 save/restore
    // offset for each non-roff node. A last buffered word prints after Xo
    // restores its origin; preceding words print before that restore.
    assert_eq!(
        description_rows(
            ".Bl -tag -width 4n\n.It Xo\n.No Alpha\n.nf\n.No Beta\n.No Gamma\n.No Delta\n.Xc\n.No BodyWord\n.El\n"
        ),
        [
            "Alpha",
            "      Beta",
            "      Gamma",
            "Delta",
            "      BodyWord"
        ]
    );
    assert_eq!(
        description_rows(
            ".Bl -tag -width 4n\n.It Xo\n.No Alpha\n.nf\n.No Beta\n.Xc\n.No BodyWord\n.El\n"
        ),
        ["Alpha", "Beta", "      BodyWord"]
    );
}

#[test]
fn empty_native_body_post_preserves_bare_backafter() {
    // Exact complete inputs verified with pristine CVS -Tascii/-Tutf8/-Tlint.
    // term.c::term_newln() flushes only lastcol || viscol; an empty BODY
    // buffer therefore leaves bare BACKAFTER for AFTER, whose A is covered
    // by F. A prior BODY .br already committed the generated fixed gap.
    for kind in ["inset", "diag"] {
        let rows: Vec<_> = description_rows(&format!(
            ".Bl -{kind}\n.It X\n.br\n.No \\z\n.El\n.No AFTER\n"
        ))
        .iter()
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
        assert_eq!(rows, ["X", "FTER"], "{kind}");

        // Separate exact oracle runs: a .mc flush can leave native viscol
        // occupied even after its buffer is consumed (roff_term.c:147-151).
        // This post DOES execute term_flushln(), clearing bare BACKAFTER.
        let rows: Vec<_> = description_rows(&format!(
            ".Bl -{kind}\n.It X\n.mc\n.No \\&\n.mc\n.Sm off\n.No \\z\n.El\n.Sm on\n.No AFTER\n"
        ))
        .iter()
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
        assert_eq!(rows, ["X", "AFTER"], "occupied {kind}");
    }
}

#[test]
fn unknown_special_preserves_native_backafter_for_known_glyphs() {
    // Pristine CVS -Tascii/-Tutf8 agree; lint reports only the deliberately
    // unknown special. term.c:620-633 buffers ASCII_NBRZW without consuming
    // BACKAFTER, which Y then consumes via encode1(). This observes known
    // Y/Z row and word boundaries, not ordinary source-spelling fallback.
    let rows: Vec<_> = description_rows(
        ".Bl -hang -width 4n\n.It Xo\n.No X\\p\n.No \"\\p\\z\\[unknownname]Y\"\n.No Z\n.Xc\n.No BodyWord\n.El\n",
    )
    .iter()
    .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
    .collect();
    assert_eq!(rows, ["X", "YZ BodyWord"]);
}

#[test]
fn field_acceptance_preserves_native_zero_width_rows_and_new_body_fields() {
    // Each exact input passed pristine CVS -Tascii/-Tutf8/-Tlint before
    // these assertions. term_fill() counts ASCII_NBRZW as a zero-width
    // graph (term.c:340-353), and nbr=0 rejects only the remaining current
    // field. A BODY .br starts a new field (roff_term.c::roff_term_pre_br).
    // This checks every hard row, including empty rows, and word retention.
    // Responsive definition placement can vary horizontal field padding;
    // the source-offset contract above separately checks exact row origins.
    let mut failures = Vec::new();
    for (body, expected) in [
        (
            ".Bl -hang -width 4n\n.It Xo\n.No X\\p\n.No \"\\p\\&\"\n.No Y\n.Xc\n.No BodyWord\n.El\n",
            vec!["X", "", "Y BodyWord"],
        ),
        (
            ".Bl -hang -width 4n\n.It Xo\n.No \\p\n.No \\zY\n.No Z\n.Xc\n.No BodyWord\n.El\n",
            vec!["BodyWord"],
        ),
        (
            ".Bl -inset\n.It Xo\n.No \"X\\p \\p Y\"\n.Xc\n.br\n.No BodyWord\n.El\n",
            vec!["X", "", "BodyWord"],
        ),
        (
            ".Bl -hang -width 4n\n.It Xo\n.No \"X\\p \\p Y\"\n.br\n.Xc\n.No BodyWord\n.El\n",
            vec!["X", "BodyWord"],
        ),
        (
            ".Bl -hang -width 4n\n.It Xo\n.No \"\\zA\\zBC \\p Z\"\n.Xc\n.No BodyWord\n.El\n",
            vec!["C", "BodyWord"],
        ),
        (
            ".Bl -hang -width 4n\n.It Xo\n.Lk https://example.org \"X\\p Y\" \"\\p Z\"\n.Xc\n.No BodyWord\n.El\n",
            // Pristine CVS accepts X/Y, then rejects the remaining field.
            // termp_lk_pre() executes every description operand before the
            // colon and URI; term_fill() rejects that suffix along with Z.
            vec!["X", "Y", "BodyWord"],
        ),
        (
            // The label projects as `label: uri` exactly like the pristine
            // reference row for this input. Its hidden native writes still
            // reject Z from this field.
            ".Bl -hang -width 4n\n.It Xo\n.Lk \"https://example.org\\p \\p\" X\n.No Z\n.Xc\n.No BodyWord\n.El\n",
            vec!["X: https://example.org", "BodyWord"],
        ),
        (
            // A loop endline after its accepted prefix and the !NOBREAK,
            // !HANG tail endline are distinct (term.c:220, 250-253).
            ".Bl -ohang\n.It Xo\n.No X\n.No \\p\n.No Y\n.Xc\n.No BodyWord\n.El\n",
            vec!["X", "", "BodyWord"],
        ),
        (
            // The same !NOBREAK/!HANG tail also ends a rejected FIRST
            // pass, although there was no accepted glyph before it.
            ".Bl -ohang\n.It Xo\n.No \\p\n.No X\n.Xc\n.No BodyWord\n.El\n.No AFTER\n",
            vec!["", "BodyWord", "AFTER"],
        ),
        (
            ".nf\n.Bl -ohang\n.It Xo\n.No X\n.No \\p\n.No Y\n.Xc\n.No BodyWord\n.El\n",
            // The preceding .nf is not a transparent predecessor for
            // print_bvspace(); this non-compact Bl also has leading space.
            vec!["", "X", "", "Y", "BodyWord"],
        ),
        (
            ".nf\n.Bl -ohang\n.It Xo\n.No X\n.No \\p\n.No \\p\n.No Y\n.Xc\n.No BodyWord\n.El\n",
            vec!["", "X", "", "", "Y", "BodyWord"],
        ),
        (
            // TERMENC_UTF8 SPECIAL calls encode1(U+00A0), overwriting A
            // but retaining its occupied blank row. ASCII uses encode()
            // with ASCII_NBRSP and retains A instead (term.c:620-638).
            ".Bl -hang -width 4n\n.It Xo\n.No \\zA\\~\\p\n.No Y\n.Xc\n.No BodyWord\n.El\n",
            vec!["", "Y BodyWord"],
        ),
        (
            // Unicode U+200B is encode1(width=0), unlike the internal
            // ASCII_NBRZW cell emitted by \\& (term.c:807-817).
            ".Bl -hang -width 4n\n.It Xo\n.No X\\p\n.No \"\\p\\[u200B]\"\n.No Y\n.Xc\n.No BodyWord\n.El\n",
            vec!["X", "\u{200b}", "Y BodyWord"],
        ),
        (
            // The rejected field retains its navigation target but adds
            // no visible BODY text. The Bl boundary keeps its native gap
            // before AFTER, without an additional empty rendered paragraph.
            ".Bl -inset\n.It Xo\n.No \"alpha \\p beta\"\n.Xc\n.No tail text\n.El\n.No AFTER\n",
            vec!["alpha", "", "AFTER"],
        ),
        (
            ".Bl -inset\n.It Xo\n.No one\n.No \\p\n.No two\n.Xc\n.No tail text\n.El\n.No AFTER\n",
            vec!["one", "", "AFTER"],
        ),
    ] {
        let rows: Vec<_> = description_rows(body)
            .iter()
            .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect();
        if rows != expected {
            failures.push(format!("{body}\nactual: {rows:?}\nexpected: {expected:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn rejected_lk_suffix_preserves_accepted_label_identity_and_row_origin() {
    #[derive(Default)]
    struct Links(Vec<(String, String)>);
    impl<'ir> Visit<'ir> for Links {
        fn visit_inline(&mut self, inline: &'ir mant_ir::Inline) {
            if let mant_ir::Inline::Link {
                target: mant_ir::LinkTarget::External { uri },
                children,
                ..
            } = inline
            {
                self.0
                    .push((uri.clone(), mant_ir::inline_plain_text(children)));
            }
            mant_ir::visit::walk_inline(self, inline);
        }
    }

    use mant_ir::visit::Visit;

    // This exact complete source ran through the pristine oracle before the
    // assertion. termp_lk_pre() executes both description operands before
    // the colon/URI; term_fill() accepts X and Y, rejects the remaining
    // current field, and cannot retract those accepted rows or the identity.
    let body = ".Bl -hang -width 4n\n.It Xo\n.Lk https://example.org \"X\\p Y\" \"\\p Z\"\n.Xc\n.No BodyWord\n.El\n";
    // Re-frozen exact pristine profiles confirm BRIND's six-cell origin
    // for accepted Y. The single captured HEAD receipt now preserves that
    // positioning through Link wrapping along with its hard row; the
    // later rejected suffix cannot retract it (term.c:217,225-228).
    assert_eq!(description_rows(body), ["X", "      Y", "      BodyWord"]);
    let query = mant_loader::load_roff_bytes(source(body).as_bytes()).unwrap();
    let mut links = Links::default();
    links.visit_document(query.document.as_ref().unwrap());
    assert!(!links.0.is_empty(), "accepted label lost its link identity");
    assert!(links.0.iter().all(|(uri, label)| {
        uri == "https://example.org" && !label.contains('Z') && !label.contains(uri)
    }));
    let accepted_glyphs: String = links
        .0
        .iter()
        .flat_map(|(_, label)| label.chars())
        .filter(|ch| !ch.is_whitespace())
        .collect();
    assert_eq!(accepted_glyphs, "XY");
}

#[test]
fn extended_head_fixtures_execute_in_the_intended_ast_owner() {
    use libmandoc_rs::{Node, NodeKind, Parser};

    fn find(node: &Node, predicate: impl Fn(&Node) -> bool + Copy) -> Option<&Node> {
        if predicate(node) {
            Some(node)
        } else {
            node.children
                .iter()
                .find_map(|child| find(child, predicate))
        }
    }

    // All five complete sources ran through pristine CVS tree/ASCII/UTF-8
    // and lint before this assertion. mdoc_macro.c::blk_full() explicitly
    // leaves -diag It HEAD unparsed: literal "Xo" cannot exercise the same
    // extended HEAD path as the other list kinds.
    for kind in ["tag", "hang", "ohang", "inset", "diag"] {
        let width = if matches!(kind, "tag" | "hang") {
            " -width 4n"
        } else {
            ""
        };
        let close = if kind == "diag" { "" } else { ".Xc\n" };
        let input = source(&format!(
            ".Bl -{kind}{width}\n.It Xo\n.No HeadWord\n{close}.No BodyWord\n.El\n"
        ));
        let report = Parser::default()
            .parse_bytes("definition-owner.1", input.as_bytes())
            .expect("parse definition fixture");
        let item = find(&report.document.root, |node| {
            node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("It")
        })
        .expect("It block");
        let head = item
            .children
            .iter()
            .find(|node| node.kind == NodeKind::Head)
            .expect("HEAD");
        let body = item
            .children
            .iter()
            .find(|node| node.kind == NodeKind::Body)
            .expect("BODY");
        assert!(find(body, |node| node.text.as_deref() == Some("BodyWord")).is_some());
        if kind == "diag" {
            assert_eq!(head.children[0].text.as_deref(), Some("Xo"));
            assert!(find(head, |node| node.macro_token.as_deref() == Some("Xo")).is_none());
            assert!(find(body, |node| node.text.as_deref() == Some("HeadWord")).is_some());
        } else {
            assert!(
                find(head, |node| node.kind == NodeKind::Block
                    && node.macro_token.as_deref() == Some("Xo"))
                .is_some(),
                "{kind}"
            );
            assert!(find(head, |node| node.text.as_deref() == Some("HeadWord")).is_some());
            assert!(find(body, |node| node.text.as_deref() == Some("HeadWord")).is_none());
        }
    }
}

#[test]
fn field_marker_is_consumed_before_the_next_operands_internal_spaces() {
    // These complete inputs first passed pristine CVS ASCII/UTF-8/lint.
    // term_word()573-580 inserts each real word separator before its text;
    // term_fill()287-306 consumes a prior \p at that separator. A marker
    // whose separator is suppressed remains buffered until a later blank.
    // Diag validation merges `tail text` into one TEXT node, while hang
    // emits two TEXT operands. Destination/parser wrapping cannot replay
    // the previous marker at a new operand's internal space.
    for (body, expected) in [
        (
            ".Bl -diag\n.It X\n.No alpha\\p\n.No tail text\n.El\n",
            vec!["X alpha", "tail text"],
        ),
        (
            ".Bl -inset\n.It Xo\n.No alpha\\p\n.Xc\n.No tail text\n.El\n",
            vec!["alpha tail", "text"],
        ),
        (
            ".Bl -hang -width 4n\n.It Xo\n.No alpha\\p\n.No tail text\n.Xc\n.No BodyWord\n.El\n",
            vec!["alpha", "tail text BodyWord"],
        ),
        (
            ".Bl -hang -width 4n\n.It Xo\n.No alpha\\p\n.Sm off\n.No tail text\n.Xc\n.No BodyWord\n.El\n",
            vec!["alpha", "tailtext BodyWord"],
        ),
        (
            ".Bl -hang -width 4n\n.It Xo\n.No alpha\\p\\c\n.No tail text\n.Xc\n.No BodyWord\n.El\n",
            vec!["alphatail", "text BodyWord"],
        ),
        (
            ".Bl -hang -width 4n\n.It Xo\n.No alpha\\p\\zX\n.No tail text\n.Xc\n.No BodyWord\n.El\n",
            vec!["alphaXtail", "text BodyWord"],
        ),
    ] {
        let rows: Vec<_> = description_rows(body)
            .iter()
            .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect();
        assert_eq!(rows, expected, "{body}");
    }
}

#[test]
fn empty_head_operands_do_not_predict_a_native_line_end() {
    // All complete inputs were run with pristine CVS -Tascii/-Tutf8/tree
    // before these assertions. Empty No operands have no NODE_LINE and run
    // term_word(); only an empty source TEXT with NODE_LINE runs term_vspace
    // (mdoc_term.c:354-378). The source-empty cases intentionally produce
    // CVS's blank-line-in-fill-mode lint warning. A real buffered \p still
    // meets the empty word's automatic separator in term_fill():287-306.
    let cases = [
        (".No \"\"\n.No alpha\n", vec!["alpha BodyWord"]),
        (".No alpha\n.No \"\"\n", vec!["alpha BodyWord"]),
        (".Em \"\"\n.No alpha\n", vec!["alpha BodyWord"]),
        (".No alpha\n.Em \"\"\n", vec!["alpha BodyWord"]),
        (".No \"\"\n.No alpha\\p\n", vec!["alpha BodyWord"]),
        (".No alpha\\p Ns No \"\"\n", vec!["alpha BodyWord"]),
        (".No \\fB\n.No alpha\n", vec!["alpha BodyWord"]),
        (".No alpha\n.No \\fB\n", vec!["alpha BodyWord"]),
        (".No \\&\n.No alpha\n", vec!["alpha BodyWord"]),
        (".No alpha\n.No \\&\n", vec!["alpha BodyWord"]),
        (".No alpha\\p\n.No \"\"\n", vec!["alpha", "BodyWord"]),
        (".No alpha\\p\n.No \\fB\n", vec!["alpha", "BodyWord"]),
        (".No alpha\\p\n.No \\&\n", vec!["alpha", "BodyWord"]),
        (".No alpha\n\n", vec!["alpha", "", "BodyWord"]),
        ("\n.No alpha\n", vec!["", "alpha BodyWord"]),
        (".No alpha\\p\n\n", vec!["alpha", "", "BodyWord"]),
    ];
    for (head, expected) in cases {
        let body = format!(".Bl -inset\n.It Xo\n{head}.Xc\n.No BodyWord\n.El\n");
        let rows: Vec<_> = description_rows(&body)
            .iter()
            .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect();
        assert_eq!(rows, expected, "{body}");
        if !head.contains("\\p") && !head.contains("\n\n") && !head.starts_with('\n') {
            let query = mant_loader::load_roff_bytes(source(&body).as_bytes()).unwrap();
            let document = query.document.as_ref().unwrap();
            let mant_ir::Block::DefinitionList { items, .. } = &document.sections[1].blocks[0]
            else {
                panic!("expected definition list: {document:?}");
            };
            assert_eq!(
                items[0].layout.head_body_relation,
                mant_ir::HeadBodyRelation::separated(mant_ir::DefinitionBodyAlignment::Indented)
            );
        }
    }
}
