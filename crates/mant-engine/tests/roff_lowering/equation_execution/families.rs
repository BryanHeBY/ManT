//! Separate owner, native mode, shape and retirement witnesses complement the matrix.

use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Families {
    reference_sha256: String,
    cases: Vec<Family>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Family {
    id: String,
    source: String,
    source_sha256: String,
    expected: Expected,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Expected {
    State {
        body: String,
        normal_atom: bool,
    },
    Shape {
        reading: String,
        masked_body: String,
    },
    Link {
        literal: bool,
        label: String,
        body: String,
        outside_x: bool,
    },
    Definition {
        literal: bool,
        term: String,
        description: String,
        head_equation: bool,
    },
    End {
        before: String,
        after: Option<String>,
        second_section: bool,
    },
    Empty,
    LiteralReference {
        body: String,
    },
}

fn cases() -> Vec<Family> {
    // All sources first ran all five fixed native profiles. In particular,
    // mdoc_macro.c::find_pending postpones closing an `.It Xo` HEAD, keeping
    // its equation nodes there. man_breakscope's `.TP` transition gives its
    // following equation to BODY. Neither source-line equality nor a rendered
    // substring may move content between those owners. In a literal It HEAD,
    // term_newln only flushes its NOBREAK field while it fits. The next field
    // stays on that physical row with trailspace=2 (mdoc_term.c::termp_it_pre,
    // term.c::term_flushln), so NODE_LINE retains exactly two blanks there.
    let families: Families = serde_json::from_str(include_str!("families.json")).unwrap();
    assert_eq!(
        families.reference_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(families.cases.len(), 118);
    for case in &families.cases {
        assert_eq!(case.source_sha256.len(), 64);
    }
    families.cases
}

fn masked_inline(nodes: &[Inline]) -> String {
    let mut text = String::new();
    for node in nodes {
        match node {
            Inline::Equation { .. } => text.push_str("<MATH>"),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => text.push_str(&masked_inline(children)),
            _ => text.push_str(&inline_plain_text(std::slice::from_ref(node))),
        }
    }
    text
}

fn exactly_one_owner<'a>(inventory: &Inventory<'a>, literal: bool) -> &'a [Inline] {
    let owners = if literal {
        &inventory.literals
    } else {
        &inventory.paragraphs
    };
    let [owner] = owners.as_slice() else {
        panic!("one complete source owner");
    };
    owner
}

fn source_body(item: &DefinitionItem, literal: bool) -> &[Inline] {
    match item.description.as_slice() {
        [Block::Paragraph { children, .. }] if !literal => children,
        [Block::Preformatted { children, .. }] if literal => children,
        _ => panic!(
            "one complete native definition BODY owner: {:?}",
            item.description
        ),
    }
}

fn assert_plain_consumers(content: &ResolvedContent, expected: &str) {
    assert_eq!(
        text_body(content),
        expected,
        "public text preserves each source row and blank"
    );
    for options in [MarkdownOptions::default(), MarkdownOptions::READABLE] {
        let markdown = render_markdown_with_options(content, options);
        assert_eq!(
            markdown_body(&markdown, None),
            expected,
            "parsed Markdown body: {markdown:?}"
        );
    }
    assert_search(content, expected);
}

fn assert_definition(
    content: &ResolvedContent,
    inventory: &Inventory<'_>,
    literal: bool,
    term: &str,
    description: &str,
    head_equation: bool,
) {
    let [item] = inventory.definitions.as_slice() else {
        panic!("one native definition");
    };
    let [native_term] = item.terms.as_slice() else {
        panic!("one complete native HEAD");
    };
    assert_eq!(inline_plain_text(native_term), term);
    assert_eq!(inline_plain_text(source_body(item, literal)), description);
    let mut head = Inventory::default();
    for inline in native_term {
        head.visit_inline(inline);
    }
    assert_eq!(head.equations.len(), usize::from(head_equation));
    assert_eq!(inventory.equations.len(), 1);
    let forms = mant_ir::EntryOwner::Definition(item)
        .forms()
        .expect("valid native forms");
    let projected: Vec<_> = forms.iter().map(inline_plain_text).collect();
    assert_eq!(projected, [term]);
    assert!(!inline_plain_text(source_body(item, literal)).contains("HEAD_SENTINEL"));
    assert!(!inline_plain_text(native_term).contains("BODY_SENTINEL"));
    // Owner assertions above are exact. Public serializers must also keep
    // each distinct author sentinel and X once, independent of list syntax.
    for options in [MarkdownOptions::default(), MarkdownOptions::READABLE] {
        let markdown = render_markdown_with_options(content, options);
        let visible = markdown_body(&markdown, None);
        for sentinel in ["TAG_SENTINEL", "HEAD_SENTINEL", "BODY_SENTINEL", "X"] {
            let expected = term.matches(sentinel).count() + description.matches(sentinel).count();
            assert_eq!(
                visible.matches(sentinel).count(),
                expected,
                "{sentinel}: {markdown:?}"
            );
        }
    }
}

fn assert_shape(
    content: &ResolvedContent,
    inventory: &Inventory<'_>,
    reading: &str,
    masked_body: &str,
) {
    let owner = exactly_one_owner(inventory, false);
    assert_eq!(inventory.equations, [reading]);
    let mut pending = vec![inventory.expressions[0]];
    while let Some(expression) = pending.pop() {
        assert_ne!(
            expression.kind,
            mant_ir::EquationKind::Opaque,
            "supported native operators retain mathematical structure"
        );
        pending.extend(&expression.children);
    }
    assert_eq!(
        masked_inline(owner),
        masked_body,
        "native outside-word boundary stays outside the mathematical owner"
    );
    assert_eq!(text_body(content), masked_body.replace("<MATH>", reading));
    for options in [MarkdownOptions::default(), MarkdownOptions::READABLE] {
        let markdown = render_markdown_with_options(content, options);
        assert_eq!(
            markdown_body(&markdown, Some(reading)),
            masked_body,
            "actual parsed mathematical owner: {markdown:?}"
        );
    }
}

fn assert_retirement(
    content: &ResolvedContent,
    inventory: &Inventory<'_>,
    before: &str,
    after: Option<&str>,
    second_section: bool,
) {
    let document = content.document.as_ref().unwrap();
    let owners: Vec<_> = inventory
        .paragraphs
        .iter()
        .chain(&inventory.literals)
        .map(|nodes| inline_plain_text(nodes))
        .collect();
    assert_eq!(
        owners.iter().filter(|text| text.as_str() == before).count(),
        1,
        "retired owner remains complete: {owners:?}"
    );
    if let Some(after) = after {
        assert_eq!(
            owners.iter().filter(|text| text.as_str() == after).count(),
            1,
            "next owner starts with its own state: {owners:?}"
        );
        assert_eq!(owners.len(), 2);
    } else {
        assert_eq!(owners.len(), 1);
    }
    assert_eq!(inventory.equations, ["x"]);
    assert_eq!(
        document.sections.iter().any(|section| section
            .heading
            .content
            .iter()
            .any(|node| inline_plain_text(std::slice::from_ref(node)) == "SECOND")),
        second_section
    );
    let exported = render_markdown_with_options(content, MarkdownOptions::READABLE);
    if let Some(after) = after {
        let visible: String = Parser::new(&exported)
            .filter_map(|event| match event {
                Event::Text(value) | Event::Code(value) => Some(value.into_string()),
                _ => None,
            })
            .collect();
        assert_eq!(
            visible.matches("NEXT_OWNER").count(),
            after.matches("NEXT_OWNER").count()
        );
    }
}

fn assert_family(content: &ResolvedContent, case: &Family) {
    let document = content.document.as_ref().unwrap();
    assert_eq!(mant_ir::validate_document(document), Vec::new());
    let inventory = observe(document);
    assert_eq!(inventory.displays, 0);
    match &case.expected {
        Expected::State { body, normal_atom } => {
            if *normal_atom {
                assert_eq!(inventory.equations, ["x"]);
            }
            assert_plain_consumers(content, body);
        }
        Expected::Shape {
            reading,
            masked_body,
        } => assert_shape(content, &inventory, reading, masked_body),
        Expected::Link {
            literal,
            label,
            body,
            outside_x,
        } => {
            let owner = exactly_one_owner(&inventory, *literal);
            assert_eq!(inline_plain_text(owner), *body);
            let [native_label] = inventory.links.as_slice() else {
                panic!("one original link label");
            };
            assert_eq!(inline_plain_text(native_label), *label);
            if *outside_x {
                assert_eq!(label.matches('X').count(), 0);
                assert_eq!(body.matches('X').count(), 1);
            }
            assert_eq!(inventory.equations, ["x"]);
            assert_plain_consumers(content, body);
        }
        Expected::Definition {
            literal,
            term,
            description,
            head_equation,
        } => {
            assert_definition(
                content,
                &inventory,
                *literal,
                term,
                description,
                *head_equation,
            );
        }
        Expected::End {
            before,
            after,
            second_section,
        } => assert_retirement(
            content,
            &inventory,
            before,
            after.as_deref(),
            *second_section,
        ),
        Expected::Empty => {
            assert_eq!(inventory.paragraphs.len(), 0);
            assert_eq!(inventory.literals.len(), 0);
            assert_eq!(inventory.equations.len(), 0);
            assert_eq!(document.blocks.len(), 0);
            assert!(
                !document
                    .sections
                    .iter()
                    .any(|section| inline_plain_text(&section.heading.content) == "OVERVIEW")
            );
        }
        Expected::LiteralReference { body } => {
            assert_eq!(inventory.equations, ["printf(3)"]);
            assert_eq!(
                inventory.links.len(),
                0,
                "formula words do not enable ordinary document-reference recognition"
            );
            assert_eq!(text_body(content), *body);
            let owner = exactly_one_owner(&inventory, false);
            assert_eq!(masked_inline(owner), "A <MATH>B C");
            for options in [MarkdownOptions::default(), MarkdownOptions::READABLE] {
                let markdown = render_markdown_with_options(content, options);
                assert_eq!(markdown_body(&markdown, Some("printf(3)")), "A <MATH>B C");
            }
        }
    }
}

#[test]
fn native_formula_owners_modes_shapes_and_retirement_keep_their_boundaries() {
    super::super::case_failures::verify_cases(
        cases(),
        |case| &case.id,
        |case| {
            let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            assert_family(&original, case);
            let transported = super::super::json_roundtrip::json_string_roundtrip(&original);
            assert_eq!(transported, original);
            assert_family(&transported, case);
        },
    );
}
