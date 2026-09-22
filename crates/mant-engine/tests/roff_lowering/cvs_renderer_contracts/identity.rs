use super::*;

#[test]
fn executed_heading_keeps_authored_navigation_identity() {
    // CVS mdoc_term.c executes the heading through term_word(), so a bare
    // BACKAFTER removes the first displayed character.  mdoc HTML still uses
    // the authored heading as the fragment identity; ManT likewise separates
    // executed presentation from navigation identity.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No \\z\n",
        ".Sh NEXT SECTION\n.No AB\n.Sx NEXT SECTION\n",
    );
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower executed heading");
    let document = query.document.as_ref().expect("lowered document");
    let section = document
        .sections
        .iter()
        .find(|section| section.id.as_str() == "next-section")
        .expect("authored section identity");
    assert_eq!(
        section.heading.plain_text(document.content()),
        "EXT SECTION"
    );

    let mut link = AuthoredSectionLink {
        id: "next-section",
        found: false,
        content: document.content(),
    };
    link.visit_document(document);
    assert!(
        link.found,
        "authored .Sx did not resolve after heading execution"
    );
}

#[test]
fn executed_heading_display_cannot_shadow_an_authored_navigation_title() {
    // The pinned CVS formatter consumes the N in the first heading and shows
    // it as EXT, while its HTML target remains authored NEXT.  A later
    // authored EXT heading therefore remains the unique destination of
    // `.Sx EXT`; the projected display text is not a navigation alias.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No \\z\n",
        ".Sh NEXT\n.Sx EXT\n.Sh EXT\n.No BODY\n",
    );
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower colliding headings");
    let document = query.document.as_ref().expect("lowered document");
    let headings = document
        .sections
        .iter()
        .map(|section| {
            (
                section.id.as_str(),
                section.heading.plain_text(document.content()),
            )
        })
        .collect::<Vec<_>>();
    assert!(headings.contains(&("next", "EXT".to_owned())));
    assert!(headings.contains(&("ext", "EXT".to_owned())));

    let mut link = AuthoredSectionLink {
        id: "ext",
        found: false,
        content: document.content(),
    };
    link.visit_document(document);
    assert!(link.found, "authored EXT reference was shadowed: {query:?}");
    assert!(
        document.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_deref() != Some("unresolved-section-reference")
        })
    );
}

#[test]
fn sx_display_state_cannot_change_its_authored_destination() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh NEXT SECTION\n.No FIRST\n.Sh NEXTSECTION\n.No SECOND\n",
        ".Sh SEE ALSO\n.Sm off\n.Sx NEXT SECTION\n",
    );
    let native = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("sx-authored.1", source.as_bytes())
        .expect("render native Sx identity")
        .output;
    assert!(
        native.contains("href=\"#NEXT_SECTION\">NEXTSECTION</a>"),
        "native HTML: {native}"
    );

    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower Sx identity");
    let document = query.document.as_ref().expect("lowered document");
    let mut correct = AuthoredSectionLink {
        id: "next-section",
        found: false,
        content: document.content(),
    };
    correct.visit_document(document);
    assert!(correct.found, "Sx target was inferred from display text");
}

#[test]
fn section_and_sx_share_cvs_deroff_authored_normalization() {
    for (heading, reference) in [("\\&NEXT", "NEXT"), ("NEXT", "\\&NEXT")] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh {heading}\n.No BODY\n.Sh SEE ALSO\n.Sx {reference}\n"
        );
        let native = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render_bytes("sx-deroff.1", source.as_bytes())
            .expect("render native deroff identity")
            .output;
        assert!(
            native.contains("class=\"Sx\" href=\"#NEXT\""),
            "native HTML: {native}"
        );

        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower deroff identity");
        let document = query.document.as_ref().expect("lowered document");
        let mut link = AuthoredSectionLink {
            id: "next",
            found: false,
            content: document.content(),
        };
        link.visit_document(document);
        assert!(link.found, "authored target did not normalize: {query:?}");
        assert!(document.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_deref() != Some("unresolved-section-reference")
        }));
    }
}

#[test]
fn sx_authored_escape_cannot_collapse_into_a_display_equivalent_heading() {
    // html.c::html_make_id() derives both Sh and Sx fragments from the
    // authored text before terminal `\z` projection.  The two visually equal
    // headings therefore remain distinct navigation identities.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh AC\n.No PLAIN\n.Sh A\\zBC\n.No ESCAPED\n",
        ".Sh SEE ALSO\n.Sx A\\zBC\n",
    );
    let native = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("sx-authored-escape.1", source.as_bytes())
        .expect("render native escape-bearing Sx identity")
        .output;
    assert!(native.contains("id=\"A_zBC\""), "native HTML: {native}");
    assert!(
        native.contains("href=\"#A_zBC\">AC</a>"),
        "native HTML: {native}"
    );

    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower authored Sx escape");
    let document = query.document.as_ref().expect("lowered document");
    assert!(document.sections.iter().any(|section| section.id == "ac"));
    assert!(
        document
            .sections
            .iter()
            .any(|section| section.id == "a-zbc")
    );
    let mut correct = AuthoredSectionLink {
        id: "a-zbc",
        found: false,
        content: document.content(),
    };
    correct.visit_document(document);
    assert!(correct.found, "Sx target was inferred from projected AC");
}

#[test]
fn nested_heading_author_modes_execute_without_losing_inline_adjacency() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh Dq An -split An Alice\n.No BODY\n",
    );
    let native = native_terminal(source);
    assert!(native.contains("“\nAlice”"), "native terminal: {native:?}");
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower nested An heading");
    let document = query.document.as_ref().unwrap();
    let heading = &document.sections[1].heading.content;
    assert!(
        heading
            .iter()
            .any(|inline| matches!(inline, Inline::LineBreak { .. })),
        "nested An split was not executed: {heading:?}"
    );
    assert_eq!(super::inline_text(document.content(), heading), "“\nAlice”");
}

#[test]
fn heading_wrappers_remove_only_the_structural_bold_layer() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh TARGET\n.No BODY\n.Sh Sx TARGET\n.No SX\n",
        ".Sh Lk https://example.org Label\n.No LK\n",
    );
    let native = native_terminal_raw(source);
    assert!(
        native.contains("_\u{8}T\u{8}T"),
        "CVS heading did not combine bold and underline: {native:?}"
    );
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower linked headings");
    let markdown = mant_codec::encode::render_markdown(&query);
    assert!(markdown.contains("## *TARGET*"), "{markdown}");
    assert!(
        markdown.contains("## [*Label*](https://example.org)"),
        "{markdown}"
    );
    assert!(!markdown.contains("***TARGET***"), "{markdown}");
    assert!(!markdown.contains("***Label***"), "{markdown}");
}
