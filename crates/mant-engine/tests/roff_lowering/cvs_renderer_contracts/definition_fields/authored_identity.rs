use super::*;
use mant_ir::{DefinitionBodyAlignment, HeadBodyRelation};

#[test]
fn one_authored_link_keeps_one_identity_across_committed_and_rejected_fields() {
    // These exact inputs passed fixed CVS -Tascii/-Tutf8/-Tlint. Its
    // mdoc_html.c::mdoc_lk_pre() opens one anchor per Lk; term.c::term_flushln()
    // may accept X before a later field returns nbr=0, but cannot create a
    // second source link or revoke the accepted prefix.
    let prefix =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for style in ["tag", "hang"] {
        for (label, expected, rejected) in [(r"X\p Y", "X\nY", false), (r#"X\p "\p Y""#, "X", true)]
        {
            let source = format!(
                "{prefix}.Bl -{style} -width 4n\n.It Xo\n.Lk https://example.com {label}\n.Xc\n.No BODY\n.El\n"
            );
            let native = without_line_indentation(&native_terminal(&source));
            assert!(native.contains("BODY"), "CVS {style} {label}: {native:?}");
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let item = first_definition_item(query.document.as_ref().unwrap());
            let links = item
                .terms
                .iter()
                .flatten()
                .filter_map(|inline| match inline {
                    Inline::Link { children, .. } => Some(children),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(links.len(), 1, "{style} {label}: {item:?}");
            // A closed last row can be represented by Separate layout
            // rather than by a trailing break inside the source Link. Row
            // ownership is verified by the rendered contract below.
            assert_eq!(
                inline_text(links[0]).trim_end_matches('\n'),
                expected,
                "{style} {label}: {item:?}"
            );
            let expected_rows = match (style, rejected) {
                ("tag", false) => vec!["X", "Y", "BODY"],
                ("tag", true) => vec!["X", "", "BODY"],
                (_, false) => vec!["X", "Y BODY"],
                (_, true) => vec!["X", "BODY"],
            };
            let rows = |value: &str| {
                let mut rows = value
                    .split_once("DESCRIPTION\n")
                    .unwrap()
                    .1
                    .lines()
                    // Compact IR keeps URI identity, not its device suffix.
                    .map(|row| row.replace(": https://example.com", ""))
                    .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
                    .collect::<Vec<_>>();
                let end = rows
                    .iter()
                    .position(|row| row.split_whitespace().any(|word| word == "BODY"))
                    .expect("retained BODY");
                rows.truncate(end + 1);
                rows
            };
            // Exact pristine source runs precede these assertions.
            // term_flushln()220 ends each accepted pass; BRIND's new
            // vfield=0 makes the non-HANG tail250-253 end another row even
            // after nbr=0. HANG suppresses that final device endline.
            assert_eq!(rows(&native), expected_rows, "native {style} {label}");
            assert_eq!(
                rows(&lowered_terminal(&source)),
                expected_rows,
                "lowered {style} {label}"
            );
            assert_eq!(
                item.layout.head_body_relation,
                if style == "hang" && !rejected {
                    // The exact accepted X/Y rows above resolve the first
                    // BODY alignment after the final HEAD row; consumers
                    // no longer infer that preference from a multiline term.
                    HeadBodyRelation::separated(DefinitionBodyAlignment::AfterTerm)
                } else {
                    HeadBodyRelation::Separate
                },
                "{style} {label}"
            );
        }
    }

    let prose = format!("{prefix}.Lk https://example.com X\\p Y\n");
    let query = mant_loader::load_roff_bytes(prose.as_bytes()).unwrap();
    let paragraph_links = query.document.as_ref().unwrap().sections[1]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(
                children
                    .iter()
                    .filter(|inline| matches!(inline, Inline::Link { .. }))
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    assert_eq!(paragraph_links, 1, "single CVS mdoc_html.c anchor");

    // Two distinct Lk source nodes with the same target remain two links.
    // Fixed CVS mdoc_html.c opens two anchors for this exact lint-clean input.
    let distinct =
        format!("{prefix}.Lk https://example.com first\n.Lk https://example.com second\n");
    let query = mant_loader::load_roff_bytes(distinct.as_bytes()).unwrap();
    let distinct_links = query.document.as_ref().unwrap().sections[1]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(
                children
                    .iter()
                    .filter(|inline| matches!(inline, Inline::Link { .. }))
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    assert_eq!(distinct_links, 2);
}

#[test]
fn repeated_authored_section_titles_remain_ambiguous() {
    // CVS HTML resolves this to the first duplicate fragment.  ManT's stricter
    // navigation contract deliberately refuses to choose between two authored
    // destinations, while retaining both sections under unique stable IDs.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Sx DETAILS\n",
        ".Sh DETAILS\n.No ONE\n.Sh DETAILS\n.No TWO\n",
    );
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower duplicate headings");
    let document = query.document.as_ref().expect("lowered document");
    assert!(
        document
            .sections
            .iter()
            .any(|section| section.id == "details")
    );
    assert!(
        document
            .sections
            .iter()
            .any(|section| section.id == "details-2")
    );

    for id in ["details", "details-2"] {
        let mut link = AuthoredSectionLink { id, found: false };
        link.visit_document(document);
        assert!(!link.found, "ambiguous authored title resolved to {id}");
    }
    assert!(
        document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("unresolved-section-reference")
        })
    );
}

#[test]
fn discarded_terminal_fields_keep_authored_link_destinations() {
    // Each exact input passed fixed CVS -Tutf8/-Thtml/-Tlint. term_fill()
    // discards the visible NOBREAK field, while mdoc_html.c still emits the
    // Lk/Mt href from the authored operand. Preserve the typed IR target.
    for (link, target) in [
        (
            ".Lk https://example.com visible",
            mant_ir::LinkTarget::External {
                uri: "https://example.com".to_owned(),
            },
        ),
        (
            ".Lk https://example.com",
            mant_ir::LinkTarget::External {
                uri: "https://example.com".to_owned(),
            },
        ),
        (
            ".Mt user@example.com",
            mant_ir::LinkTarget::Email {
                address: "user@example.com".to_owned(),
            },
        ),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo X\n.br\n.No \\p\n{link}\n.br\n.Xc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source);
        let lowered = lowered_terminal(&source);
        assert!(native.contains("X     BODY"), "{link}: {native:?}");
        assert!(lowered.contains("X     BODY"), "{link}: {lowered:?}");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let item = first_definition_item(query.document.as_ref().unwrap());
        let targets = item
            .terms
            .iter()
            .flatten()
            .filter_map(|inline| match inline {
                Inline::Link { target, .. } => Some(target),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(targets, vec![&target], "{link}: {item:?}");
    }
}
