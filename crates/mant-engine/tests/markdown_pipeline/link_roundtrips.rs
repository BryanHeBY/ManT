//! Final Markdown bytes keep the original visible and navigable link ranges.

use mant_codec::encode::{
    MarkdownOptions, render_addressable_markdown, render_markdown_with_options,
};
use mant_ir::{EntryOwner, ResolvedContent};
use mant_protocol::{QueryBundle, SearchCase, SearchQuery, SearchScope, SearchSyntax};

#[path = "../support/link_meaning.rs"]
mod link_meaning;
use link_meaning::{Meaning, SOURCES};

fn json_roundtrip(content: &ResolvedContent) -> ResolvedContent {
    let json = serde_json::to_string(&QueryBundle::from(content)).unwrap();
    let restored = ResolvedContent::from(serde_json::from_str::<QueryBundle>(&json).unwrap());
    assert_eq!(&restored, content);
    restored
}

fn search(content: &ResolvedContent, pattern: &str) -> mant_protocol::QuerySearch {
    mant_query::search_query(
        content,
        &SearchQuery {
            pattern: pattern.into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 20,
            offset: 0,
        },
    )
    .unwrap()
}

#[test]
fn actual_links_keep_their_original_scalars_metadata_and_query_ranges_after_two_reads() {
    for (index, link) in SOURCES.iter().enumerate() {
        for context in 0..4 {
            let body = match context {
                0 | 1 => format!("## Payload\n\n{link}"),
                2 => format!("## Payload\n\n- {link}"),
                _ => format!(
                    "## Payload\n\n<!-- mant:entries role=option case=sensitive -->\n- `--link`: {link}"
                ),
            };
            let source = format!("# Probe\n\n{body}\n\n## Next\n\nEND\n");
            let mut original = mant_loader::load_markdown_text(&source, None).unwrap();
            if context == 1 {
                // ATX source headings end at LF, so a multiline title/label
                // cannot be admitted by pasting its source after `##`.
                // Move the already parsed original Link into a legal heading
                // owner instead; its metadata is still independent IR₀.
                let section = &mut original.document.as_mut().unwrap().sections[0];
                let mant_ir::Block::Paragraph { children, .. } = section.blocks.remove(0) else {
                    panic!("original link paragraph")
                };
                section.heading.content = children;
            }
            let expected = Meaning::capture(&original);
            assert!(
                expected
                    .target()
                    .to_uri()
                    .unwrap()
                    .starts_with("https://a.test")
            );
            let mut decoded = json_roundtrip(&original);
            for semantics in [false, true] {
                for round in 1..=2 {
                    let markdown = render_markdown_with_options(
                        &decoded,
                        MarkdownOptions {
                            preserve_anchors: false,
                            preserve_semantics: semantics,
                        },
                    );
                    decoded =
                        json_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
                    let label = format!(
                        "case {index}/context {context}/semantics {semantics}/round {round}: {markdown}"
                    );
                    expected.assert_preserved(&decoded, &label);
                    if semantics && context == 3 {
                        let excerpt =
                            super::semantic_test_read::semantic_excerpt(&decoded, &["--link"])
                                .unwrap();
                        assert!(serde_json::to_string(&excerpt).unwrap().contains("--link"));
                        assert_eq!(search(&decoded, "--link").total, 1, "{label}: name owner");
                    }
                    let pattern = if index == 0 {
                        "! a "
                    } else if index == 8 {
                        "a*b"
                    } else if index == 9 {
                        "LINK_B"
                    } else {
                        "LinkWord"
                    };
                    let result = search(&decoded, pattern);
                    assert_eq!(result.total, 1, "{label}: exact visible search");
                    let artifact = render_addressable_markdown(&decoded);
                    for occurrence in result.matches.iter().flat_map(|hit| &hit.occurrences) {
                        assert_eq!(occurrence.matched_text, pattern);
                        let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
                            ..usize::try_from(occurrence.markdown.end_byte).unwrap();
                        let slice = &artifact.text()[range];
                        let guarded = format!("X {slice} Z");
                        let visible: String = pulldown_cmark::Parser::new(&guarded)
                            .filter_map(|event| match event {
                                pulldown_cmark::Event::Text(value)
                                | pulldown_cmark::Event::Code(value) => Some(value.into_string()),
                                _ => None,
                            })
                            .collect();
                        assert_eq!(
                            visible,
                            format!("X {pattern} Z"),
                            "{label}: source range must cover the matched visible scalars: {slice}"
                        );
                    }
                    if index == 0 {
                        assert_eq!(
                            search(&decoded, "_! a_ ").total,
                            0,
                            "{label}: rejected marker text"
                        );
                    }
                }
                decoded = original.clone();
            }
        }
    }
}

#[test]
fn link_metadata_does_not_move_the_original_entry_name_or_artifact_owner() {
    for link in SOURCES {
        let source = format!(
            "# Probe\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `--link`: {link}\n"
        );
        let content = json_roundtrip(&mant_loader::load_markdown_text(&source, None).unwrap());
        let document = content.document.as_ref().unwrap();
        let mant_ir::Block::List { items, .. } = &document.sections[0].blocks[0] else {
            panic!("original list owner")
        };
        let owner = EntryOwner::List(&items[0]);
        assert_eq!(owner.validated_names().unwrap(), ["--link"]);
        let before = content.clone();
        let artifact = render_addressable_markdown(&content);
        let mapped = artifact
            .nodes()
            .iter()
            .filter(|node| {
                matches!(node.node(),
            mant_codec::encode::MarkdownNode::DocumentEntry { owner: candidate, .. }
            if candidate.facts().unwrap().id == owner.facts().unwrap().id)
            })
            .collect::<Vec<_>>();
        assert_eq!(mapped.len(), 1);
        assert!(artifact.text()[mapped[0].range()].contains("--link"));
        let excerpt = super::semantic_test_read::semantic_excerpt(&content, &["--link"]).unwrap();
        assert!(serde_json::to_string(&excerpt).unwrap().contains("--link"));
        assert_eq!(search(&content, "--link").total, 1);
        assert_eq!(
            content, before,
            "encoding/query never rewrite original owner"
        );
    }
}
