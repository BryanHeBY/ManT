//! Compare authored section destinations independently of displayed heading text.

use libmandoc_rs::{RenderFormat, Renderer};
use mant_ir::{
    Block, Inline, LinkTarget, ResolvedContent, Section,
    visit::{self, Visit},
};
use mant_protocol::{ContentSelector, ExcerptSelection};
use pulldown_cmark::{Event, Parser, Tag};

const HEADER: &str =
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";

#[test]
fn section_macros_resolve_by_source_words_and_keep_their_display() {
    // Pinned CVS post_section/deroff and mdoc_sx_pre: the href names the
    // authored words; Xr/Fl display punctuation appears later in the renderer.
    for (reference, heading, native_id, display) in [
        ("printf 3", ".Sh Xr printf 3", "printf_3", "printf(3)"),
        ("option", ".Sh Fl option", "option", "-option"),
        (
            "Topic (shell)",
            ".Sh Em Topic (shell)",
            "Topic_(shell)",
            "Topic (shell)",
        ),
        (
            "probe topic",
            ".Sh Nm Fl topic",
            "probe_topic",
            "probe -topic",
        ),
        ("tool 3", ".Ss Xr tool 3", "tool_3", "tool(3)"),
    ] {
        let source = format!("{HEADER}.Sx {reference}\n{heading}\nBODY-A.\n");
        assert_native_destinations(&source, &[native_id], &[Some(native_id)]);
        let query = decoded(&source);
        let selected = assert_destinations(&query, &["BODY-A."]);
        let section = excerpt(&query, &selected[0]);
        assert_eq!(
            mant_ir::inline_plain_text(&section.heading.content),
            display
        );
        if heading.contains(" Xr ") {
            let name = reference.split_once(' ').unwrap().0;
            assert!(matches!(
                section.heading.content.as_slice(),
                [Inline::Link {
                    target: LinkTarget::Manual { name: target, manual_section: Some(manual) }, ..
                }] if target == name && manual == "3"
            ));
        }
    }
}

#[test]
fn section_targets_prefer_validated_heading_ids_over_view_or_order() {
    for (reference, weak, strong, native_id) in [
        ("option", "Fl option", "option", "option"),
        (
            "Topic space",
            "Fl Topic space",
            "Topic_space",
            "Topic_space",
        ),
    ] {
        let weak = format!(".Sh {weak}\nBODY-A.\n");
        let strong = format!(".Sh {strong}\nBODY-B.\n");
        for (headings, native_ids) in [
            ([weak.as_str(), strong.as_str()], [None, Some(native_id)]),
            ([strong.as_str(), weak.as_str()], [Some(native_id), None]),
        ] {
            let source = format!("{HEADER}.Sx {reference}\n{}", headings.concat());
            assert_native_destinations(&source, &[native_id], &native_ids);
            assert_destinations(&decoded(&source), &["BODY-B."]);
        }
    }
    let source = format!(
        "{HEADER}.Sx option\n.Tg option\n.Sh Fl alternative\nBODY-A.\n.Sh option\nBODY-B.\n"
    );
    assert_native_destinations(&source, &["option"], &[Some("option"), None]);
    assert_destinations(&decoded(&source), &["BODY-A."]);
}

#[test]
fn same_displayed_section_titles_keep_distinct_authored_destinations() {
    const XR: &str = ".Sh Xr printf 3\nBODY-A.\n";
    const TEXT: &str = ".Sh \"printf(3)\"\nBODY-B.\n";
    for (headings, native_ids) in [
        ([XR, TEXT], [Some("printf_3"), Some("printf(3)")]),
        ([TEXT, XR], [Some("printf(3)"), Some("printf_3")]),
    ] {
        let source = format!(
            "{HEADER}.Sx printf 3\n.Sx \"printf(3)\"\n{}",
            headings.concat()
        );
        assert_native_destinations(&source, &["printf_3", "printf(3)"], &native_ids);
        let query = decoded(&source);
        let selected = assert_destinations(&query, &["BODY-A.", "BODY-B."]);
        assert_ne!(selected[0], selected[1]);
        for id in selected {
            assert_eq!(
                mant_ir::inline_plain_text(&excerpt(&query, &id).heading.content),
                "printf(3)"
            );
        }
    }
}

#[test]
fn section_lookup_preserves_existing_ids_when_macro_display_differs() {
    // Xr prints parentheses and Lk prints the label before the URL in
    // the terminal. Both keep the CVS author spelling for Sx lookup, while
    // mixed-heading canonical IDs keep the already published display slug.
    for (reference, heading, native_id, old_id) in [
        ("printf 3", "printf 3", "printf_3", "printf-3"),
        ("printf 3", "Xr printf 3", "printf_3", "printf-3"),
        ("option", "option", "option", "option"),
        ("option", "Fl option", "option", "option"),
        (
            "https://example.org Label",
            "Lk https://example.org Label",
            "https://example.org_Label",
            "label-https-example-org",
        ),
        (
            "https://example.org",
            "Lk https://example.org",
            "https://example.org",
            "https-example-org",
        ),
    ] {
        let source = format!("{HEADER}.Sx {reference}\n.Sh {heading}\nBODY-A.\n");
        assert_native_destinations(&source, &[native_id], &[Some(native_id)]);
        assert_eq!(
            assert_destinations(&decoded(&source), &["BODY-A."]),
            [old_id]
        );
    }
}

#[test]
fn section_lookup_preserves_safe_fallbacks_and_empty_headings() {
    let source = format!("{HEADER}.Sx TOPIC\n.Sh \"\"\nEMPTY-C.\n.Sh Fl TOPIC\nBODY-A.\n");
    assert_native_destinations(&source, &["TOPIC"], &[None, Some("TOPIC")]);
    let query = decoded(&source);
    assert_destinations(&query, &["BODY-A."]);
    assert_eq!(section_body(&excerpt(&query, "section")), "EMPTY-C.");

    for (reference, headings, native_id, native_ids, body) in [
        (
            "\"printf(3)\"",
            ".Sh Xr printf 3\nBODY-A.\n",
            "printf(3)",
            &[Some("printf_3")][..],
            None,
        ),
        (
            "option",
            ".Sh Fl option\nBODY-A.\n.Sh Em option\nBODY-B.\n",
            "option",
            &[None, None][..],
            None,
        ),
        (
            "Topic",
            ".Sh Fl Topic (qualifier)\nBODY-A.\n",
            "Topic",
            &[Some("Topic_(qualifier)")][..],
            Some("BODY-A."),
        ),
        (
            "Topic",
            ".Sh Fl Topic (qualifier)\nBODY-A.\n.Sh Fl Topic (other)\nBODY-B.\n",
            "Topic",
            &[Some("Topic_(qualifier)"), Some("Topic_(other)")][..],
            None,
        ),
        (
            "Topic",
            ".Sh Fl Topic (qualifier)\nBODY-A.\n.Sh Topic\nBODY-B.\n",
            "Topic",
            &[Some("Topic_(qualifier)"), Some("Topic")][..],
            Some("BODY-B."),
        ),
    ] {
        let source = format!("{HEADER}.Sx {reference}\n{headings}");
        assert_native_destinations(&source, &[native_id], native_ids);
        assert_destinations(&decoded(&source), body.as_slice());
    }
}

#[test]
fn lossy_html_fragment_projection_does_not_alias_unrelated_titles() {
    for (reference, heading, colliding_fragment) in
        [("Other<", "Other>", "Other_"), ("FOO<", "FOO_", "FOO_")]
    {
        let source = format!("{HEADER}.Sx {reference}\n.Sh {heading}\nBODY-A.\n");
        // html_make_id() maps punctuation to underscores even when that
        // erases the difference between two unrelated authored spellings.
        assert_native_destinations(&source, &[colliding_fragment], &[Some(colliding_fragment)]);
        assert_destinations(&decoded(&source), &[]);
    }
}

#[test]
fn exact_validated_heading_tags_keep_punctuation_and_their_original_owners() {
    // The fixed HTML emits the projected fragment, but the validator-
    // approved tag preserves the author's spelling (mdoc_validate.c).
    for (title, fragment) in [
        ("PLAIN", "PLAIN"),
        ("W^X", "W_X"),
        ("Other<", "Other_"),
        ("A~B", "A_B"),
        ("Custom%", "Custom_"),
        ("Hello#id", "Hello_id"),
    ] {
        for (headings, native_ids) in [
            (
                format!(".Tg {title}\n.Sh OTHER\nBODY-B.\n"),
                vec![Some(fragment)],
            ),
            (
                format!(".Sh {title}\nBODY-B.\n.Sh Em {title}\nBODY-A.\n"),
                vec![Some(fragment), None],
            ),
            (
                format!(".Sh Em {title}\nBODY-A.\n.Sh {title}\nBODY-B.\n"),
                vec![None, Some(fragment)],
            ),
        ] {
            let reference = format!(".Sx {title}\n");
            for source in [
                format!("{HEADER}{reference}{headings}"),
                format!("{HEADER}{headings}{reference}"),
            ] {
                assert_native_destinations(&source, &[fragment], &native_ids);
                let query = decoded(&source);
                let ids = document_destinations(&query);
                let [id] = ids.as_slice() else {
                    panic!("wanted one published link, found {ids:?}: {source}");
                };
                assert!(
                    section_body(&excerpt(&query, id)).starts_with("BODY-B."),
                    "{source}"
                );
                let markdown = mant_codec::encode::render_addressable_markdown(&query);
                let reread = mant_loader::load_markdown_text(markdown.text(), None).unwrap();
                assert_eq!(document_destinations(&reread), ids, "{source}");
            }
        }
    }
}

#[test]
fn validated_punctuation_exactness_does_not_erase_ambiguity() {
    // html_make_id maps both distinct spellings to W_X. ManT keeps exact
    // spelling identity and declines a genuinely duplicated authored tag.
    let duplicate =
        format!("{HEADER}.Sx W^X\n.Tg W^X\n.Sh FIRST\nBODY-A.\n.Tg W^X\n.Sh SECOND\nBODY-B.\n");
    assert_native_destinations(&duplicate, &["W_X"], &[Some("W_X"), Some("W_X~2")]);
    assert_destinations(&decoded(&duplicate), &[]);
    let collision = format!("{HEADER}.Sx W^X\n.Sx W>X\n.Sh W^X\nBODY-A.\n.Sh W>X\nBODY-B.\n");
    assert_native_destinations(&collision, &["W_X", "W_X"], &[Some("W_X"), Some("W_X~2")]);
    assert_destinations(&decoded(&collision), &["BODY-A.", "BODY-B."]);
    for (headings, body) in [
        (".Sh W^X (shell)\nBODY-A.\n", &["BODY-A."][..]),
        (
            ".Sh W^X (shell)\nBODY-A.\n.Sh W^X (other)\nBODY-B.\n",
            &[][..],
        ),
    ] {
        let source = format!("{HEADER}.Sx W^X\n{headings}");
        assert_destinations(&decoded(&source), body);
    }
}

#[test]
fn validated_multiword_targets_preserve_punctuation_and_destination_owners() {
    // CVS post_section() and html_make_id() agree on the word separator.
    // ManT retains each other authored character before selecting an owner.
    for (label, tag, fragment) in [
        ("Foo Bar", "Foo_Bar", "Foo_Bar"),
        ("W^X Test", "W^X_Test", "W_X_Test"),
        ("W^X_Test", "W^X_Test", "W_X_Test"),
        ("A~B and C", "A~B_and_C", "A_B_and_C"),
        ("Other< Test", "Other<_Test", "Other__Test"),
        ("Hello#id Test", "Hello#id_Test", "Hello_id_Test"),
        ("Custom% Name", "Custom%_Name", "Custom__Name"),
    ] {
        let reference = format!(".Sx {label}\n");
        let destination = format!(".Tg {tag}\n.Sh DESTINATION\nBODY-A.\n");
        for source in [
            format!("{HEADER}{reference}{destination}"),
            format!("{HEADER}{destination}{reference}"),
        ] {
            assert_native_destinations(&source, &[fragment], &[Some(fragment)]);
            let query = decoded(&source);
            let ids = document_destinations(&query);
            let [id] = ids.as_slice() else {
                panic!("expected only the proven target: {source}");
            };
            assert!(section_body(&excerpt(&query, id)).starts_with("BODY-A."));
            let markdown = mant_codec::encode::render_addressable_markdown(&query);
            let reread = mant_loader::load_markdown_text(markdown.text(), None).unwrap();
            assert_eq!(document_destinations(&reread), ids, "{source}");
        }
    }
}

#[test]
fn multiword_section_lookup_rejects_lost_punctuation_and_duplicate_owners() {
    let duplicate = format!(
        "{HEADER}.Sx W^X Test\n.Tg W^X_Test\n.Sh FIRST\nBODY-A.\n.Tg W^X_Test\n.Sh SECOND\nBODY-B.\n"
    );
    assert_native_destinations(
        &duplicate,
        &["W_X_Test"],
        &[Some("W_X_Test"), Some("W_X_Test~2")],
    );
    assert_destinations(&decoded(&duplicate), &[]);

    let distinct = format!(
        "{HEADER}.Sx W^X Test\n.Sx W>X Test\n.Tg W^X_Test\n.Sh FIRST\nBODY-A.\n.Tg W>X_Test\n.Sh SECOND\nBODY-B.\n"
    );
    assert_native_destinations(
        &distinct,
        &["W_X_Test", "W_X_Test"],
        &[Some("W_X_Test"), Some("W_X_Test~2")],
    );
    assert_destinations(&decoded(&distinct), &["BODY-A.", "BODY-B."]);

    let mismatch = format!("{HEADER}.Sx Other< Test\n.Tg Other>_Test\n.Sh OTHER\nBODY-A.\n");
    assert_native_destinations(&mismatch, &["Other__Test"], &[Some("Other__Test")]);
    assert_destinations(&decoded(&mismatch), &[]);
}

fn document_destinations(query: &ResolvedContent) -> Vec<String> {
    query
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .flat_map(destinations)
        .collect()
}

fn assert_native_destinations(source: &str, hrefs: &[&str], headings: &[Option<&str>]) {
    // These are fixed CVS HTML href and heading-ID pairs, independent of
    // the codec and profiler's shared identity computation.
    let html = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("probe.1", source.as_bytes())
        .unwrap()
        .output;
    let references: Vec<_> = html
        .split("<a class=\"Sx\" href=\"#")
        .skip(1)
        .map(|rest| rest.split_once('"').unwrap().0)
        .collect();
    assert_eq!(references, hrefs, "{source}");
    let ids: Vec<_> = html
        .split('<')
        .filter(|tag| tag.starts_with("h2 class=\"Sh\"") || tag.starts_with("h3 class=\"Ss\""))
        .skip(2)
        .map(|tag| {
            tag.split_once(" id=\"")
                .and_then(|(_, id)| id.split_once('"').map(|(id, _)| id))
        })
        .collect();
    assert_eq!(ids, headings, "{source}");
}

fn decoded(source: &str) -> ResolvedContent {
    let mut content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = serde_json::to_vec(content.document.as_ref().unwrap()).unwrap();
    content.document = Some(serde_json::from_slice(&json).unwrap());
    content
}

fn assert_destinations(query: &ResolvedContent, expected: &[&str]) -> Vec<String> {
    let doc = query.document.as_ref().unwrap();
    let ids = destinations(&doc.sections[1]);
    for (id, body) in ids.iter().zip(expected) {
        assert_eq!(section_body(&excerpt(query, id)), *body, "{id}");
    }
    assert_eq!(ids.len(), expected.len());
    assert_eq!(
        doc.diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code.as_deref() == Some("unresolved-section-reference")
            })
            .count(),
        usize::from(expected.is_empty())
    );
    let markdown = mant_codec::encode::render_addressable_markdown(query);
    let reread = mant_loader::load_markdown_text(markdown.text(), None).unwrap();
    let reread_doc = reread.document.as_ref().unwrap();
    assert_eq!(destinations(&reread_doc.sections[1]), ids);
    let mut fragments = Vec::new();
    let mut anchors = Vec::new();
    for event in Parser::new(markdown.text()) {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) if dest_url.starts_with('#') => {
                fragments.push(dest_url);
            }
            Event::Html(value) | Event::InlineHtml(value) => anchors.push(value),
            _ => {}
        }
    }
    for id in &ids {
        // The addressable export publishes the destination as HTML for
        // Markdown readers. Raw HTML is not a semantic reimport format.
        let anchor = format!("<a id=\"{id}\">");
        assert!(
            anchors
                .iter()
                .any(|value| value.lines().any(|line| line.starts_with(&anchor))),
            "missing CommonMark HTML destination {id:?} in {:?}",
            markdown.text()
        );
        assert!(
            fragments
                .iter()
                .any(|fragment| fragment.strip_prefix('#') == Some(id))
        );
    }
    ids
}

fn destinations(section: &Section) -> Vec<String> {
    #[derive(Default)]
    struct Links(Vec<String>);
    impl<'a> Visit<'a> for Links {
        fn visit_inline(&mut self, inline: &'a Inline) {
            if let Inline::Link {
                target: LinkTarget::Section { id },
                ..
            } = inline
            {
                self.0.push(id.to_string());
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut links = Links::default();
    for block in &section.blocks {
        links.visit_block(block);
    }
    links.0
}

fn excerpt(query: &ResolvedContent, id: &str) -> Section {
    let result = mant_query::select_excerpt(query, &[ContentSelector::id(id)]).unwrap();
    let [ExcerptSelection::DocumentSection { section, .. }] = result.selections.as_slice() else {
        panic!("expected {id} to select a section: {:?}", result.selections);
    };
    section.clone()
}

fn section_body(section: &Section) -> String {
    let [Block::Paragraph { children, .. }] = section.blocks.as_slice() else {
        panic!("expected one body paragraph: {:?}", section.blocks);
    };
    mant_ir::inline_plain_text(children)
}
