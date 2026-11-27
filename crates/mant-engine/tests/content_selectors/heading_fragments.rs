use std::fmt::Write as _;

use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{ContentReveal, DocumentAddress, ResolvedContent};
use mant_protocol::{
    ContentSelector, EntryProjection, LoadedFragment, ReferenceProjection, ReferenceProjectionMode,
    ReferenceResolution, ReferenceTargetType, UnloadedFragment,
};

fn guide(source: &str) -> ResolvedContent {
    let mut content = mant_loader::load_markdown_text(source, None).unwrap();
    content.address = DocumentAddress::parse_catalog_path("documents/guide");
    super::json_roundtrip::json_string_roundtrip(&content)
}

fn references(content: &ResolvedContent) -> Vec<mant_protocol::ReferenceRecord> {
    mant_query::build_outline_with_references(
        content,
        EntryProjection::None,
        None,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            target_types: vec![ReferenceTargetType::Local, ReferenceTargetType::Document],
            ..ReferenceProjection::default()
        },
    )
    .unwrap()
    .references
    .records
}

fn assert_body(content: &ResolvedContent, index: usize, expected_id: &str, marker: &str) {
    let section = &content.document.as_ref().unwrap().sections[index];
    assert_eq!(section.id.as_str(), expected_id);
    let by_id =
        mant_query::select_excerpt(content, &[format!("id:{}", section.id).parse().unwrap()])
            .unwrap();
    let by_path =
        mant_query::select_excerpt(content, &[ContentSelector::path((index + 1).to_string())])
            .unwrap();
    assert_eq!(by_id, by_path);
    assert!(
        mant_render::render_excerpt_text(&by_id).contains(marker),
        "{expected_id}"
    );
}

#[test]
fn valid_explicit_heading_ids_survive_parsing_and_select_their_original_bodies() {
    for id in [
        "cache_key",
        "_init",
        "tail_",
        "_foo_",
        "known--key",
        "日本_語",
    ] {
        let content = guide(&format!("# Guide\n\n## Caption {{#{id}}}\n\nBODY EXACT.\n"));
        let section = &content.document.as_ref().unwrap().sections[0];
        assert_eq!(section.heading.plain_text(), "Caption");
        assert!(
            section
                .fragment_aliases
                .iter()
                .any(|alias| alias.as_str() == id)
        );
        assert_body(&content, 0, id, "BODY EXACT.");
    }
}

#[test]
fn later_short_heading_keeps_its_id_and_links_under_a_long_hash_collision() {
    let long = "a".repeat(513);
    let literal = format!("{}-02425c0f5b0d", "a".repeat(499));
    for reverse in [false, true] {
        let mut source = format!(
            "# Guide\n\n[Long](#{long}) [Long self](guide.md#{long}) \
             [Literal](#{literal}) [Literal self](guide.md#{literal})\n",
        );
        let mut order = [(&long, "BODY-A"), (&literal, "BODY-B")];
        if reverse {
            order.reverse();
        }
        for (caption, body) in order {
            writeln!(source, "\n## {caption}\n\n{body}").unwrap();
        }
        let content = guide(&source);
        let sections = &content.document.as_ref().unwrap().sections;
        let long_index = usize::from(reverse);
        assert_ne!(sections[long_index].id.as_str(), literal);
        assert_body(&content, 1 - long_index, &literal, "BODY-B");
        assert_body(
            &content,
            long_index,
            sections[long_index].id.as_str(),
            "BODY-A",
        );
        let references = references(&content);
        assert_eq!(references.len(), 4);
        for record in references {
            let index = if record.label.starts_with("Long") {
                long_index
            } else {
                1 - long_index
            };
            assert_eq!(
                record.resolution,
                ReferenceResolution::Loaded {
                    address: content.address.clone(),
                    fragment: LoadedFragment::Valid {
                        reveal: ContentReveal::Section {
                            sections: vec![u32::try_from(index).unwrap()]
                        }
                    }
                },
            );
        }
    }
}

#[test]
fn same_document_links_get_the_same_fragment_outcome_in_both_uri_forms() {
    let source = "# Guide {#actual-root}\n\n[Root](#guide) [Root self](guide.md#guide)\n\n\
        ## Caption {#canonical}\n\n[Title](#caption) [Title self](guide.md#caption) \
        [Canonical](#canonical) [Canonical self](guide.md#canonical) \
        [Gone](#missing) [Gone self](guide.md#missing) [Elsewhere](other.md#missing)\n\nBODY.\n";
    let content = guide(source);
    let records = references(&content);
    assert_eq!(records.len(), 9);
    for record in records {
        let fragment = match record.label.as_str() {
            "Root" | "Root self" => LoadedFragment::Valid {
                reveal: ContentReveal::Document {},
            },
            "Gone" | "Gone self" => LoadedFragment::Missing {},
            "Elsewhere" => {
                assert!(matches!(
                    record.resolution,
                    ReferenceResolution::LogicalAddress {
                        fragment: UnloadedFragment::Unchecked {},
                        ..
                    }
                ));
                continue;
            }
            _ => LoadedFragment::Valid {
                reveal: ContentReveal::Section { sections: vec![0] },
            },
        };
        assert_eq!(
            record.resolution,
            ReferenceResolution::Loaded {
                address: content.address.clone(),
                fragment
            },
            "{}",
            record.label,
        );
    }
    assert_body(&content, 0, "canonical", "BODY.");
}

#[test]
fn duplicate_explicit_fragments_are_never_resolved_to_an_arbitrary_owner() {
    for alias in [
        "b".repeat(511),
        "b".repeat(512),
        "b".repeat(513),
        "dup.X".into(),
        "界".repeat(171),
    ] {
        let content = guide(&format!(
            "# Guide\n\n[Pick](#{alias}) [Pick self](guide.md#{alias}) [First title](#first)\n\n\
             ## First {{#{alias}}}\n\nBODY FIRST.\n\n## Second {{#{alias}}}\n\nBODY SECOND.\n"
        ));
        let doc = content.document.as_ref().unwrap();
        let all = references(&content);
        assert_eq!(all.len(), 3);
        for record in all {
            let fragment = if record.label == "First title" {
                LoadedFragment::Valid {
                    reveal: ContentReveal::Section { sections: vec![0] },
                }
            } else {
                LoadedFragment::Ambiguous {}
            };
            assert_eq!(
                record.resolution,
                ReferenceResolution::Loaded {
                    address: content.address.clone(),
                    fragment,
                }
            );
        }
        let export = render_markdown_with_options(&content, MarkdownOptions::ADDRESSABLE);
        assert!(export.contains("Pick [Pick self]"));
        assert!(!export.contains(&format!("<span id=\"{alias}\"></span>")));
        assert_body(&content, 0, doc.sections[0].id.as_str(), "BODY FIRST.");
        assert_body(&content, 1, doc.sections[1].id.as_str(), "BODY SECOND.");
    }
}

#[test]
fn unclaimed_duplicate_title_slugs_and_overview_alias_conflicts_stay_ambiguous() {
    for source in [
        "# Guide\n\n[Pick](#caption) [Pick self](guide.md#caption)\n\n\
         ## Caption {#one}\n\nBODY ONE.\n\n## Caption {#two}\n\nBODY TWO.\n",
        "# Guide\n\n[Pick](#guide) [Pick self](guide.md#guide)\n\n\
         ## Elsewhere {#guide}\n\nBODY.\n",
    ] {
        let content = guide(source);
        let all = references(&content);
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|record| matches!(
            record.resolution,
            ReferenceResolution::Loaded {
                fragment: LoadedFragment::Ambiguous {},
                ..
            }
        )));
    }
}
