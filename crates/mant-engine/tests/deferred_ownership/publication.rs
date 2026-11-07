use crate::{cases, json_roundtrip, publication_fixture};
use mant_codec::encode::{
    MarkdownOptions, render_addressable_markdown_with_options, render_markdown_with_options,
};
use mant_ir::{LinkTarget, ResolvedContent};
#[test]
fn published_labels_and_literal_suffixes_survive_two_markdown_rounds() {
    // CVS man_html.c keeps both terms within the original anchor while
    // man_term.c::pre_literal executes the .EE break. Roff escape processing retains
    // the literal brackets, except for ManT's source-proven Sphinx extension
    // outside existing links. Portable Markdown omits manual navigation.
    for (id, text, label) in [
        (
            "N4/UR/filled/EE/text",
            "FIRSTMARK\nLASTMARK <https://one.test> TAILMARK\n",
            Some("FIRSTMARK\nLASTMARK"),
        ),
        (
            "E12/exact/word/UR",
            "printf(3) <> <https://one.test>\n",
            Some("printf(3) <>"),
        ),
        (
            "E12extra/twice",
            "printf(3) <> and open(2) <> <https://one.test>\n",
            Some("printf(3) <> and open(2) <>"),
        ),
        ("E12/raw/BR/plain", "printf(3) <>\n", None),
        ("E12/exact/word/plain", "printf(3)\n", None),
    ] {
        let input = publication_fixture::case(id);
        let original = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(input.source.as_bytes()).unwrap(),
        );
        let native = cases::snapshot(&original);
        assert_eq!(native.text(), text, "{id} original");
        assert_eq!(native.links.len(), 1, "{id} original");
        let target = label.map_or_else(
            || LinkTarget::Manual {
                name: "printf".into(),
                manual_section: Some("3".into()),
            },
            |_| LinkTarget::External {
                uri: "https://one.test".into(),
            },
        );
        assert_eq!(native.links[0].target, target, "{id} original");
        assert_eq!(
            native.links[0].text,
            label.unwrap_or("printf(3)"),
            "{id} original"
        );
        for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
            for direct in [false, true] {
                let mut current = original.clone();
                for round in 1..=2 {
                    let markdown = render_markdown_with_options(&current, options);
                    let artifact = render_addressable_markdown_with_options(&current, options);
                    assert_eq!(artifact.text(), markdown, "{id} round {round}");
                    for node in artifact.nodes() {
                        assert!(markdown.get(node.range()).is_some(), "{id} round {round}");
                    }
                    current = if direct {
                        let parsed = mant_codec::parse_markdown(&markdown, None).unwrap();
                        ResolvedContent {
                            address: None,
                            label: original.label.clone(),
                            document: Some(parsed.document),
                            tldr: parsed.tldr,
                        }
                    } else {
                        mant_loader::load_markdown_text(&markdown, None).unwrap()
                    };
                    let restored =
                        cases::snapshot(&json_roundtrip::json_string_roundtrip(&current));
                    assert_eq!(restored.text(), text, "{id} round {round}");
                    assert_eq!(restored.links.len(), usize::from(label.is_some()), "{id}");
                    if let Some(label) = label {
                        assert_eq!(restored.links[0].target, target, "{id}");
                        assert_eq!(restored.links[0].text, label, "{id}");
                    }
                }
            }
        }
    }
}
