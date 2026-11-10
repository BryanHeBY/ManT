//! Query context and physical source rows have independent separators.
use super::*;

#[test]
fn root_excerpt_framing_without_a_heading_and_with_tldr() {
    const QUICK: &str = "TLDR\n\nQuick example.";
    const CONTEXT: &str = "Catalog\n\nOutline root: OVERVIEW";
    let mut cases = 0;
    for heading in [false, true] {
        for body in [Body::None, Body::Anchor, Body::Gap, Body::HardRow] {
            for tldr in [false, true] {
                let mut original = base(0);
                original.label = LABEL.into();
                if tldr {
                    original.tldr = Some(TldrDocument {
                        title: LABEL.into(),
                        description: vec!["Quick example.".into()],
                        more_information: None,
                        examples: vec![],
                        platform: "common".into(),
                        language: "en".into(),
                        source_path: String::new(),
                        origin: TldrOrigin::Embedded,
                    });
                }
                let document = original.document.as_mut().unwrap();
                if !heading {
                    document.heading = None;
                }
                document.blocks = body.blocks();
                assert_eq!(validate_document(document), []);
                for wire in [false, true] {
                    let value = if wire {
                        query_wire(&original)
                    } else {
                        original.clone()
                    };
                    let expected = match (tldr, body.rows().is_empty()) {
                        (false, true) => LABEL.to_owned(),
                        (false, false) => format!("{LABEL}\n\n\n"),
                        (true, true) => format!("{LABEL}\n\n{QUICK}"),
                        (true, false) => format!("{LABEL}\n\n{QUICK}\n\n\n"),
                    };
                    let id = format!("{heading}/{body:?}/{tldr}/{wire}");
                    assert_eq!(render_query_text(&value), expected, "{id}");
                    let expected = match (heading, body.rows().is_empty()) {
                        (false, true) => CONTEXT.to_owned(),
                        (false, false) => format!("{CONTEXT}\n\n\n"),
                        (true, true) => format!("{CONTEXT}\n\n{LABEL}"),
                        (true, false) => format!("{CONTEXT}\n\n{LABEL}\n\n\n"),
                    };
                    let root = ContentSelector::path("root");
                    let selectable = heading || body != Body::None;
                    let excerpt = mant_query::select_excerpt(&value, std::slice::from_ref(&root));
                    if selectable {
                        let excerpt = excerpt.unwrap();
                        let excerpt = if wire {
                            excerpt_wire(&excerpt)
                        } else {
                            excerpt
                        };
                        assert_eq!(render_excerpt_text(&excerpt), expected, "{id}");
                    } else {
                        // A document with neither a heading nor root blocks has
                        // no DocumentRoot selection, regardless of its TLDR.
                        assert!(
                            matches!(
                                excerpt,
                                Err(mant_query::ProjectionError::UnknownSelector { .. })
                            ),
                            "{id}"
                        );
                    }
                    assert_combined_excerpt(value, root, selectable, &expected, wire, &id);
                    audit(
                        "CG-C",
                        &id,
                        if selectable {
                            "source-row-and-context-framing"
                        } else {
                            "absent-root-rejection/section-framing"
                        },
                    );
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 32);
}

fn assert_combined_excerpt(
    value: ResolvedContent,
    root: ContentSelector,
    selectable: bool,
    expected: &str,
    wire: bool,
    id: &str,
) {
    let mut sectioned = value;
    sectioned.document.as_mut().unwrap().sections.push(section(
        "subject",
        "Subject",
        0,
        vec![paragraph(vec![text("Section body")])],
    ));
    let mut selectors = vec![ContentSelector::id("subject")];
    if selectable {
        selectors.push(root);
    }
    let excerpt = mant_query::select_excerpt(&sectioned, &selectors).unwrap();
    let excerpt = if wire {
        excerpt_wire(&excerpt)
    } else {
        excerpt
    };
    assert_eq!(
        excerpt.selections.len(),
        1 + usize::from(selectable),
        "{id}"
    );
    let section_path = excerpt.selections.last().unwrap().outline().path();
    let prefix = if selectable { expected } else { LABEL };
    assert_eq!(
        render_excerpt_text(&excerpt),
        format!("{prefix}\n\nOutline {section_path}: Subject\n\nSubject\nSection body"),
        "{id}"
    );
}
