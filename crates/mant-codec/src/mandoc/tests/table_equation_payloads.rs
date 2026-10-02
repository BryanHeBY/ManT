//! Opaque tbl cells and parsed equation trees have distinct authority.
use super::{Block, Inline, Parser, inline_text, lower_mandoc_document, parse_manual_bytes};

fn table_payloads(document: &mant_ir::Document) -> Vec<String> {
    document
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .filter_map(|block| match block {
            Block::Table { rows, .. } => Some(rows),
            _ => None,
        })
        .flatten()
        .flat_map(|row| &row.cells)
        .map(|cell| {
            cell.blocks
                .iter()
                .map(|block| match block {
                    Block::Paragraph { children, .. } => inline_text(children),
                    _ => panic!("unexpected native cell payload"),
                })
                .collect()
        })
        .collect()
}

const OPAQUE_CASES: &[(&str, &str, &[&str], Option<&str>)] = &[
    (
        "tbl-eqn-basic",
        ".TH TEST 1 \"October 3, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\n.TS\nl.\n$x over 2$\n.TE\n.SH ENDTEST\nDONE\n",
        &["$x over 2$"],
        None,
    ),
    (
        "tbl-eqn-define",
        ".TH TEST 1 \"October 3, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.EQ\ndefine XX /sqrt { x }/\ndelim $$\n.EN\n.TS\nl.\n$XX$\n.TE\nOUTSIDE $XX$\n.SH ENDTEST\nDONE\n",
        &["$XX$"],
        Some("sqrt(x)"),
    ),
    (
        "tbl-eqn-redefine",
        ".TH TEST 1 \"October 3, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.EQ\ndefine XX /sqrt { x }/\ndelim $$\n.EN\n.TS\nl.\n$XX$\n.TE\n.EQ\ndefine XX /y over z/\n.EN\n.TS\nl.\n$XX$\n.TE\nOUTSIDE $XX$\n.SH ENDTEST\nDONE\n",
        &["$XX$", "$XX$"],
        Some("y / z"),
    ),
    (
        "tbl-eqn-delim-on",
        ".TH TEST 1 \"October 3, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\n.EQ\ndelim off\n.EN\n.TS\nl.\n$x$\n.TE\n.EQ\ndelim on\n.EN\n.TS\nl.\n$x$\n.TE\nOUTSIDE $x$\n.SH ENDTEST\nDONE\n",
        &["$x$", "$x$"],
        Some("x"),
    ),
    (
        "tbl-eqn-uncalled-definition",
        ".TH TEST 1 \"October 3, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.de UNUSED\n.EQ\ndelim $$\n.EN\n..\n.TS\nl.\n$x$\n.TE\n.SH ENDTEST\nDONE\n",
        &["$x$"],
        None,
    ),
    (
        "tbl-eqn-ignored-definition",
        ".TH TEST 1 \"October 3, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.ig\n.EQ\ndelim $$\n.EN\n..\n.TS\nl.\n$x$\n.TE\n.SH ENDTEST\nDONE\n",
        &["$x$"],
        None,
    ),
    (
        "no-delimiter",
        ".TH TEST 1 \"October 3, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.TS\nl.\n$x over 2$\n.TE\n.SH ENDTEST\nDONE\n",
        &["$x over 2$"],
        None,
    ),
    (
        "custom-delimiter-unicode",
        ".TH TEST 1 \"October 3, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.EQ\ndelim %%\n.EN\n.TS\nl.\n%α over 2%\n.TE\n.SH ENDTEST\nDONE\n",
        &["%α over 2%"],
        None,
    ),
    (
        "delimiter-change",
        ".TH TEST 1 \"October 3, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\n.TS\nl.\n$x$\n.TE\n.EQ\ndelim %%\n.EN\n.TS\nl.\n%x%\n.TE\n.SH ENDTEST\nDONE\n",
        &["$x$", "%x%"],
        None,
    ),
    (
        "executed-macro-context",
        ".TH TEST 1 \"October 3, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.de CONFIG\n.EQ\ndefine XX /α/\ndelim $$\n.EN\n..\n.CONFIG\n.TS\nl.\n$XX$\n.TE\nOUTSIDE $XX$\n.SH ENDTEST\nDONE\n",
        &["$XX$"],
        None,
    ),
    (
        "literal-eqn-token",
        ".TH TEST 1 \"October 3, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\n.TS\nl.\n\"$ldots2$\"\n.TE\n.SH ENDTEST\nDONE\n",
        &["\"$ldots2$\""],
        None,
    ),
];

#[test]
fn opaque_cells_do_not_borrow_guessed_eqn_context_from_root_source() {
    // All exact sources below ran the registered pristine ASCII/UTF8/HTML,
    // tree and lint profiles before these assertions. roff.c::roff_parseln
    // calls roff_eqndelim only when r->tbl == NULL; eqn_reset retains aliases
    // and delimiters, which a local fragment or root-source scan cannot prove.
    for &(label, source, expected, outside_equation) in OPAQUE_CASES {
        let path = std::path::Path::new(label);
        let native = Parser::default()
            .parse_bytes(path, source.as_bytes())
            .unwrap();
        let with_source = parse_manual_bytes(path, source.as_bytes()).unwrap();
        let owned_only = lower_mandoc_document(path, &native);
        assert_eq!(table_payloads(&with_source), expected, "{label}: source");
        assert_eq!(table_payloads(&owned_only), expected, "{label}: owned");
        assert!(
            mant_ir::content_complete(&with_source.diagnostics),
            "{label}"
        );
        assert!(
            mant_ir::semantics_complete(&with_source.diagnostics),
            "{label}"
        );
        assert!(
            !with_source.diagnostics.iter().any(|diagnostic| {
                diagnostic.code.as_deref() == Some("manual.inline-equation-budget")
            }),
            "{label}"
        );
        if let Some(expected_equation) = outside_equation {
            assert!(
                with_source
                    .sections
                    .iter()
                    .flat_map(|section| &section.blocks)
                    .any(|block| matches!(block, Block::Paragraph { children, .. }
                    if children.iter().any(|inline| matches!(inline,
                        Inline::Equation { value, expression } if value == expected_equation
                            && value == &expression.readable_text())))),
                "{label}"
            );
        }
        let json = serde_json::to_string(&with_source).unwrap();
        let decoded: mant_ir::Document = serde_json::from_str(&json).unwrap();
        assert_eq!(table_payloads(&decoded), expected, "{label}: JSON");
        assert_eq!(mant_ir::validate_document(&decoded).len(), 0, "{label}");
    }
}

#[test]
fn bundled_eqn_context_keeps_included_tbl_payload_without_root_source_recovery() {
    // Exact root/include files ran pristine profiles with a scoped working
    // directory first. roff EQ/EN retains its actual included alias state,
    // but tbl bypasses eqndelim; included T{} has native operands only.
    let source = ".TH TEST 1 \"October 3, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.so included.roff\n.TS\nl.\n$XX$\n.TE\nOUTSIDE $XX$\n.SH ENDTEST\nDONE\n";
    let included = ".EQ\ndefine XX /x sub i/\ndelim $$\n.EN\n.TS\nl.\nT{\n.B INCLUDED\nT}\n.TE\n";
    let mut bundle = libmandoc_rs::SourceBundle::new();
    bundle.insert("root.1", source.as_bytes().to_vec()).unwrap();
    bundle
        .insert("included.roff", included.as_bytes().to_vec())
        .unwrap();
    let parser = Parser::default();
    let native = parser.parse_bundle("root.1", &bundle).unwrap();
    let owned = lower_mandoc_document(std::path::Path::new("root.1"), &native);
    assert_eq!(table_payloads(&owned), ["INCLUDED", "$XX$"]);
    let json = serde_json::to_string(&owned).unwrap();
    let decoded: mant_ir::Document = serde_json::from_str(&json).unwrap();
    assert_eq!(table_payloads(&decoded), ["INCLUDED", "$XX$"]);
    assert_eq!(mant_ir::validate_document(&decoded).len(), 0);
    assert!(mant_ir::content_complete(&decoded.diagnostics));
    // The included definition still executes for ordinary inline eqn.
    // Pristine terminal renders the x/i subscript and HTML retains msub.
    for document in [&owned, &decoded] {
        assert!(
            document
                .sections
                .iter()
                .flat_map(|section| &section.blocks)
                .any(|block| matches!(block, Block::Paragraph { children, .. }
                if children.iter().any(|inline| matches!(inline,
                    Inline::Equation { value, expression }
                        if value == "x _ i" && value == &expression.readable_text()))))
        );
    }

    // The byte codec's existing include rejection policy does not change.
    // A root label, or an included source's coincident line, grants no access.
    let denied = parse_manual_bytes(std::path::Path::new("root.1"), source.as_bytes()).unwrap();
    assert_eq!(table_payloads(&denied), ["$XX$"]);
    assert!(denied.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("included.roff") && diagnostic.message.contains("disabled")
    }));
    assert!(mant_ir::content_complete(&denied.diagnostics));
    assert!(
        denied
            .sections
            .iter()
            .flat_map(|section| &section.blocks)
            .filter_map(|block| {
                if let Block::Paragraph { children, .. } = block {
                    Some(children)
                } else {
                    None
                }
            })
            .flatten()
            .all(|inline| !matches!(inline, Inline::Equation { .. }))
    );
}
