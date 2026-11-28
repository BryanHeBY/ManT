//! The definition handoff owns surviving native HEAD cells and bindings.

use super::json_roundtrip::json_string_roundtrip;
use mant_ir::{
    Block, DefinitionItem, EntryInlineRoot, EntryKind, EntryOwner, Inline, LinkTarget,
    ParameterKind, ResolvedContent, inline_plain_text,
};
use mant_protocol::{EvidenceClass, ExplanationOptions, ExplanationQuery};

const MDOC: &str =
    ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd audit\n.Sh FIRST\n";

fn lowered(head: &str, body: &str) -> ResolvedContent {
    let source = format!("{MDOC}.Bl -inset -compact\n.It {head}\n{body}\n.El\n");
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let content = json_string_roundtrip(&content);
    assert_eq!(
        mant_ir::validate_document(content.document.as_ref().unwrap()),
        []
    );
    content
}

fn item(content: &ResolvedContent) -> &DefinitionItem {
    let [Block::DefinitionList { items, .. }] = content.document.as_ref().unwrap().sections[1]
        .blocks
        .as_slice()
    else {
        panic!("expected an inset definition")
    };
    let [item] = items.as_slice() else {
        panic!("expected one definition item")
    };
    assert_eq!(item.terms.len(), 1);
    item
}

fn direct_entries(content: &ResolvedContent, name: &str) -> usize {
    mant_query::explain_query(
        content,
        &ExplanationQuery {
            entry: name.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap()
    .evidence
    .iter()
    .filter(|entry| entry.class == EvidenceClass::DirectEntry)
    .count()
}

fn assert_bindings(content: &ResolvedContent, expected: &[&str]) {
    let owner = EntryOwner::Definition(item(content));
    let facts = owner.facts().unwrap();
    assert_eq!(
        facts.kind,
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option
        }
    );
    assert_eq!(facts.names, expected);
    assert_eq!(facts.name_bindings.len(), expected.len());
    for (index, name) in expected.iter().enumerate() {
        let binding = facts
            .name_bindings
            .iter()
            .find(|b| b.name == index)
            .unwrap();
        let [occurrence] = binding.occurrences.as_slice() else {
            panic!("expected one occurrence of {name}")
        };
        assert!(occurrence.parts.iter().all(|part| {
            part.root == EntryInlineRoot::Term { index: 0 } && part.bytes.is_some()
        }));
        assert_eq!(inline_plain_text(&owner.form(occurrence).unwrap()), *name);
        assert_eq!(direct_entries(content, name), 1, "{name}");
    }
}

fn body(item: &DefinitionItem) -> &[Inline] {
    match item.description.as_slice() {
        [Block::Paragraph { children, .. }] => children,
        [] => &[],
        description => panic!("unexpected description: {description:?}"),
    }
}

#[test]
fn a_surviving_inset_head_keeps_both_option_bindings() {
    // CVS mdoc_term.c::termp_it_post leaves the inset HEAD in the field.
    // term.c::encode1 and term_field retain both decorated flags after A;
    // ABC replaces the first dash. HTML puts both authored Fl nodes in dt.
    for (source_body, expected_body) in
        [("Description.", "Description."), (r#".No "\h'|0n'A""#, "")]
    {
        let content = lowered("Fl a , Fl b", source_body);
        let item = item(&content);
        assert_eq!(inline_plain_text(&item.terms[0].content), "-a, -b");
        assert_eq!(inline_plain_text(body(item)).trim(), expected_body);
        assert_bindings(&content, &["-a", "-b"]);
    }

    let content = lowered("Fl a , Fl b", r#".No "\h'|0n'ABC""#);
    let item = item(&content);
    let visible = inline_plain_text(&item.terms[0].content) + &inline_plain_text(body(item));
    assert_eq!(visible.trim(), "ABCa, -b");
    assert_eq!(direct_entries(&content, "-a"), 0);
}

#[test]
fn a_surviving_inset_link_keeps_its_label_and_destination_in_the_term() {
    // CVS termp_lk_pre executes the label, then the terminal suffix.
    // mdoc_html.c::mdoc_lk_pre keeps the original label and href in dt.
    for (source_body, expected_body) in
        [("Description.", "Description."), (r#".No "\h'|0n'A""#, "")]
    {
        let content = lowered("Lk https://z.test test", source_body);
        let item = item(&content);
        let owner = EntryOwner::Definition(item);
        let facts = owner.facts().unwrap();
        assert_eq!(facts.kind, EntryKind::Term);
        assert!(facts.name_bindings.is_empty());
        let term = &item.terms[0].content;
        assert_eq!(inline_plain_text(term), "test: https://z.test");
        let links: Vec<_> = term
            .iter()
            .filter(|node| matches!(node, Inline::Link { .. }))
            .collect();
        let [
            Inline::Link {
                target: LinkTarget::External { uri },
                children,
                ..
            },
        ] = links.as_slice()
        else {
            panic!("expected one link in {term:?}")
        };
        assert_eq!(uri, "https://z.test");
        assert_eq!(inline_plain_text(children), "test");
        assert_eq!(inline_plain_text(body(item)).trim(), expected_body);
        assert!(
            !body(item)
                .iter()
                .any(|node| matches!(node, Inline::Link { .. }))
        );
    }
}

#[test]
fn native_closing_punctuation_stays_outside_option_bindings() {
    // CVS mdoc_macro.c::in_line closes Fl before a separate delimiter;
    // an attached question mark stays in the Fl word. An empty Fl still
    // generates the dash of its punctuation flag (mdoc_term.c::termp_fl_pre).
    for (head, form, names, absent) in [
        ("Fl a ?", "-a?", &["-a"][..], "-a?"),
        ("Fl a .", "-a.", &["-a"][..], "-a."),
        (r"Fl a \.", "-a.", &["-a"][..], "-a."),
        ("Fl a ? .", "-a?.", &["-a"][..], "-a?"),
        ("Fl a ? , Fl b", "-a?, -b", &["-a", "-b"][..], "-a?"),
        ("Fl a?", "-a?", &["-a?"][..], "-a"),
        ("Fl ?", "-?", &["-?"][..], "-"),
        ("Fl ? .", "-?.", &["-?"][..], "-?."),
    ] {
        let content = lowered(head, "Description.");
        assert_eq!(inline_plain_text(&item(&content).terms[0].content), form);
        assert_bindings(&content, names);
        assert_eq!(direct_entries(&content, absent), 0, "{head}");
    }
}
