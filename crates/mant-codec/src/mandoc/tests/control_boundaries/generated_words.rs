use super::*;

// The selected UTF-8 formatter preserves CVS's executed glyphs: mdoc Ao/Ac
// select the Unicode angle glyphs, while man post_UR always calls term_word
// with literal ASCII "<"/">" (man_term.c:896-908). These assertions were
// rechecked against pristine ASCII/UTF-8/tree/lint before synchronizing them.

#[test]
fn mdoc_unordered_list_markers_retain_their_native_style() {
    // Exact inputs checked with fixed CVS tree, HTML and UTF-8 output.
    // mdoc_html.c::mdoc_bl_pre differentiates Bl-bullet and Bl-dash;
    // mdoc_term.c::termp_it_pre uses a bullet, dash, or no marker.
    for (style, expected) in [
        ("bullet", mant_ir::ListKind::Bullet),
        ("dash", mant_ir::ListKind::Dash),
        ("hyphen", mant_ir::ListKind::Dash),
        ("item", mant_ir::ListKind::Plain),
    ] {
        let source = format!(
            ".Dd September 27, 2026\n.Dt LISTSTYLE 1\n.Os\n.Sh DESCRIPTION\n.Bl -{style}\n.It\nentry\n.El\nafter\n"
        );
        let document = parse_manual_bytes(std::path::Path::new("list-style.1"), source.as_bytes())
            .expect("lower list marker style");
        let [Block::List { kind, .. }, Block::Paragraph { .. }] =
            document.sections[0].blocks.as_slice()
        else {
            panic!("-{style}: {:#?}", document.sections[0].blocks);
        };
        assert_eq!(*kind, expected, "-{style}");
    }
}

#[test]
fn normalized_reference_title_quote_reaches_document_text() {
    // The exact source was run through the fixed oracle before the assertion.
    // CVS mdoc_validate.c::post_rs sets quote_T when %J is present;
    // mdoc_html.c::mdoc__x_pre/post encloses the %T field in curly quotes.
    let source = b".Dd September 27, 2026\n.Dt NORMALIZED-SCOPE 1\n.Os\n.Sh SYNOPSIS\n.Nm normalized-scope\n.Sh AUTHORS\n.An -split\n.An Ada\n.An Babbage\n.Sh SEE ALSO\n.Rs\n.%A Ada\n.%T Title\n.%J Journal\n.Re\n";
    let document = parse_manual_bytes(std::path::Path::new("normalized-scope.1"), source)
        .expect("lower normalized reference");
    let section = document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "SEE ALSO")
        .expect("reference section");
    let text = section
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            _ => String::new(),
        })
        .collect::<String>();
    assert!(text.contains("“Title”"), "{text:?}");
}

#[test]
fn bsd_two_operand_forms_execute_generated_words_and_joiners() {
    for (label, opening, closing, operands, follower, expected) in [
        ("plain", "", "", "4.4 Tahoe", "", "4.4BSD-Tahoe"),
        (
            "word-end",
            "",
            "",
            r"\p\c Tahoe",
            ".No AFTER LAST\n",
            "BSD-Tahoe\nAFTER LAST",
        ),
        (
            "continued-literal",
            ".Bd -literal\n",
            ".Ed\n",
            r"\c Tahoe",
            ".No AFTER\n",
            "BSD-Tahoe\nAFTER",
        ),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{opening}.Bx {operands}\n{follower}{closing}"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("bsd-two-operands-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse two-operand Bx fixture");
        let children = match document.sections[0].blocks.as_slice() {
            [Block::Paragraph { children, .. }] if opening.is_empty() => children,
            [Block::Preformatted { children, .. }] => children,
            blocks => panic!("{label}: unexpected blocks: {blocks:#?}"),
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }

    for (label, opening, closing, operands, follower, expected) in [
        (
            "kept",
            ".Bk -words\n",
            ".Ek\n",
            r"\p\c Tahoe",
            ".No AFTER LAST\n",
            "BSD-Tahoe\nAFTER LAST",
        ),
        (
            "private-enclosure",
            ".Eo [\n",
            ".Ec\n",
            r"\c Tahoe",
            ".No FINAL\n",
            "[BSD-Tahoe FINAL",
        ),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{opening}.Bx {operands}\n{closing}{follower}"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("bsd-two-operands-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse scoped two-operand Bx fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn bsd_replacement_executes_as_a_generated_formatter_word() {
    for (label, wrapper_open, wrapper_close, operand, expected, breaks) in [
        ("filled", "", "", r"\c", "BSD AFTER", 0),
        ("literal", ".Bd -literal\n", ".Ed\n", r"\c", "BSD\nAFTER", 1),
        ("word-end", "", "", r"\p\c", "BSD\nAFTER LAST", 1),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{wrapper_open}.Bx {operand}\n.No AFTER{}\n{wrapper_close}",
            if label == "word-end" { " LAST" } else { "" }
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-bx-generated-word-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse generated BSD word fixture");
        let children = match document.sections[0].blocks.as_slice() {
            [Block::Paragraph { children, .. }] if label != "literal" => children,
            [Block::Preformatted { children, .. }] if label == "literal" => children,
            blocks => panic!("{label}: unexpected blocks: {blocks:#?}"),
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert_eq!(
            children
                .iter()
                .filter(|node| matches!(node, Inline::LineBreak { .. }))
                .count(),
            breaks,
            "{label}: {children:?}"
        );
    }

    // CVS post_bx() inserts Ns + BSD after the first authored operand.  An
    // explicit empty formatter word consumes an incoming `\c` before that
    // generated tight word; ordinary input still keeps its inter-word space.
    for (operand_label, operand) in [("empty", r#""""#), ("zero-width", r"\&"), ("font", r"\fB")] {
        for (boundary_label, preceding, expected) in [
            ("ordinary", "A", "A BSD B"),
            ("continued", r"A\c", "ABSD B"),
        ] {
            let manual = format!(
                ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {preceding}\n.Bx {operand}\n.No \\fPB\n"
            );
            let document = parse_manual_bytes(
                std::path::Path::new(&format!("inline-bx-{operand_label}-{boundary_label}.1")),
                manual.as_bytes(),
            )
            .expect("parse empty BSD operand boundary fixture");
            let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
                panic!("{operand_label}/{boundary_label}: {:#?}", document.sections);
            };
            assert_eq!(
                inline_text(children),
                expected,
                "{operand_label}/{boundary_label}: {children:?}"
            );
        }
    }

    let document = parse_manual_bytes(
        std::path::Path::new("inline-bx-generated-word-keep.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.Bx \\p\\c\n.No AFTER LAST\n.Ek\n",
    )
    .expect("parse kept generated BSD word fixture");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("unexpected kept Bx blocks: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "BSD\nAFTER LAST", "{children:?}");
    assert_eq!(
        children
            .iter()
            .filter(|node| matches!(node, Inline::LineBreak { .. }))
            .count(),
        1,
        "{children:?}"
    );
}

#[test]
fn bibliography_field_posts_emit_native_punctuation_and_author_conjunction() {
    // These exact inputs were checked with fixed CVS -Tascii before writing
    // the assertions. mdoc_term.c::termp__a_pre()/termp____post() emit `and`,
    // commas and a final period at the field's own execution point;
    // termp_under_pre() styles %J and an unquoted %T.
    let authors = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh SEE ALSO\n.Rs\n.%A Ada\n.%A Babbage\n.%T Title\n.Re\nY\n";
    let document = parse_manual_bytes(std::path::Path::new("mdoc-rs-authors.1"), authors)
        .expect("parse author reference fixture");
    let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
        panic!("unexpected reference output: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "Ada and Babbage, Title. Y");
    assert!(children.iter().any(|inline| {
        matches!(inline, Inline::Emphasis { children } if inline_text(children) == "Title")
    }));

    let journal = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh SEE ALSO\n.Rs\n.%A Ada\n.%T Title\n.%J Journal\n.Re\nY\n";
    let document = parse_manual_bytes(std::path::Path::new("mdoc-rs-journal.1"), journal)
        .expect("parse journal reference fixture");
    let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
        panic!("unexpected journal output: {:#?}", document.sections);
    };
    assert!(inline_text(children).contains("Ada, “Title”, Journal. Y"));
    assert!(children.iter().any(|inline| {
        matches!(inline, Inline::Emphasis { children } if inline_text(children) == "Journal")
    }));
}

#[test]
fn bibliography_pre_and_post_obey_no_fill_source_rows() {
    // Both exact inputs were run with the fixed CVS -Tascii and -Tlint oracle.
    // mdoc_term.c::print_mdoc_node() applies NODE_LINE before termp__a_pre(),
    // while termp____post() writes a real word that consumes the field's \c.
    for (label, fields, expected) in [
        (
            "authors",
            ".%A Ada\n.%A Babbage\n.%T Title\n",
            "Ada\nand Babbage,\nTitle.\nY",
        ),
        (
            "post-continuation",
            ".%A Ada\\c\n.%T Title\n",
            "Ada,\nTitle.\nY",
        ),
    ] {
        let manual = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh SEE ALSO\n.nf\n.Rs\n{fields}.Re\nY\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("mdoc-rs-{label}.1")),
            manual.as_bytes(),
        )
        .unwrap();
        let blocks = &document.sections[1].blocks;
        assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children) == expected)), "{label}: {blocks:#?}");
    }
}

#[test]
fn bibliography_entry_preserves_continuation_and_see_also_spacing() {
    // Exact inputs checked with pinned CVS -Tascii/-Tlint. The NODE_LINE
    // check in mdoc_term.c::print_mdoc_node() respects TERMP_NONEWLINE on Rs;
    // termp_rs_pre() inserts vspace between adjacent SEE ALSO references.
    let entry = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh DESCRIPTION\n.nf\n\\zX\\c\n.Rs\nY\n.Re\nZ\n.fi\n";
    let document = parse_manual_bytes(std::path::Path::new("rs-continuation.1"), entry).unwrap();
    assert!(document.sections[1].blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children) == "Y\nZ")), "{:#?}", document.sections[1].blocks);

    let adjacent = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh SEE ALSO\n.Rs\n.%A Ada\n.%T One\n.Re\n.Rs\n.%A Bob\n.%T Two\n.Re\n";
    let document = parse_manual_bytes(std::path::Path::new("rs-adjacent.1"), adjacent).unwrap();
    let blocks = &document.sections[1].blocks;
    assert!(blocks.windows(3).any(|parts| matches!(parts, [Block::Paragraph { children: first, .. }, Block::VerticalSpace { lines: 1, .. }, Block::Paragraph { children: second, .. }] if inline_text(first) == "Ada, One." && inline_text(second) == "Bob, Two.")), "{blocks:#?}");
}
