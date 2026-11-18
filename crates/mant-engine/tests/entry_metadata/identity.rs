use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{Document, ResolvedContent};
use mant_loader::load_markdown_text;
use mant_protocol::{ContentSelector, EntryProjection, OutlineNode};
use mant_query::{build_outline_projection, select_excerpt};

use super::{facts, json_roundtrip::json_string_roundtrip, parse};

#[test]
fn explicit_entry_ids_and_aliases_use_the_published_utf8_byte_budget() {
    for (id, accepted) in [
        ("é".repeat(256), true),
        ("é".repeat(257), false),
        ("界".repeat(170), true),
        ("界".repeat(171), false),
        (format!("xy{}", "界".repeat(170)), true),
        ("a".repeat(513), false),
    ] {
        let body = format!(
            "- `--probe`: UNIQUEBODY. <!-- mant:entry {{\"id\":\"{id}\"}} -->\n\
             - `--relative`: SECONDOWNER. <!-- mant:entry {{\"aliasOf\":\"{id}\"}} -->",
        );
        let content = json_string_roundtrip(&resolved(parse(&body)));
        let document = content.document.as_ref().unwrap();
        let expected = if accepted {
            id.as_str()
        } else {
            "option-probe"
        };
        let entries = facts(document);
        assert_eq!(entries[expected].names, ["--probe"]);
        assert_eq!(
            entries["option-relative"].alias_of.as_deref(),
            accepted.then_some(id.as_str()),
        );
        let diagnostic_count = document
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code.as_deref() == Some("markdown.semantic-entry-metadata")
                    && diagnostic.source.is_some()
            })
            .count();
        assert_eq!(diagnostic_count, if accepted { 0 } else { 2 });
        assert_probe_owner_is_readable(&content, expected);

        let markdown = render_markdown_with_options(
            &content,
            MarkdownOptions {
                preserve_semantics: true,
                ..MarkdownOptions::default()
            },
        );
        let readback = load_markdown_text(&markdown, None).unwrap();
        assert_probe_owner_is_readable(&readback, expected);
        assert_eq!(
            facts(readback.document.as_ref().unwrap())["option-relative"]
                .alias_of
                .as_deref(),
            accepted.then_some(id.as_str()),
        );
        if !accepted {
            assert!(!markdown.contains(&id));
            let text = mant_render::render_query_text(&readback);
            assert!(text.contains("--probe") && text.contains("UNIQUEBODY."));
            assert!(text.contains("--relative") && text.contains("SECONDOWNER."));
        }
    }
}

#[test]
fn rejected_metadata_claims_do_not_redirect_aliases_to_other_owners() {
    for rejected in ["option-second", "reserved", "document-overview", "1/e3"] {
        let body = format!(
            "- `--probe`: UNIQUEBODY. <!-- mant:entry {{\"id\":\"{rejected}\"}} -->\n\
             - `--second`: SECONDOWNER.\n\
             - `--relative`: RELATIVE. <!-- mant:entry {{\"aliasOf\":\"{rejected}\"}} -->",
        );
        let content = if rejected == "reserved" {
            load_markdown_text(&format!(
                "# Probe\n\n## Reserved\n\n<!-- mant:entries role=option case=sensitive -->\n{body}\n"
            ), None).unwrap()
        } else {
            resolved(parse(&body))
        };
        let document = content.document.as_ref().unwrap();
        let entries = facts(document);
        assert_eq!(entries["option-probe"].names, ["--probe"]);
        assert_eq!(entries["option-second"].names, ["--second"]);
        assert_eq!(entries["option-relative"].alias_of, None);
        assert_eq!(document.diagnostics.len(), 2);
        assert_probe_owner_is_readable(&content, "option-probe");
    }
    let control = parse(
        "- `--second`: SECONDOWNER.\n- `--relative`: RELATIVE. <!-- mant:entry {\"aliasOf\":\"option-second\"} -->",
    );
    assert_eq!(
        facts(&control)["option-relative"].alias_of.as_deref(),
        Some("option-second")
    );
    assert_eq!(control.diagnostics, []);
}

fn resolved(document: Document) -> ResolvedContent {
    ResolvedContent {
        document: Some(document),
        address: None,
        label: "Probe".into(),
        tldr: None,
    }
}

fn assert_probe_owner_is_readable(content: &ResolvedContent, expected: &str) {
    fn find(nodes: &[OutlineNode]) -> Option<&OutlineNode> {
        for node in nodes {
            if node.title() == "--probe" {
                return Some(node);
            }
            if let Some(found) = find(node.children()) {
                return Some(found);
            }
        }
        None
    }
    let outline = build_outline_projection(content, EntryProjection::All, None).unwrap();
    let entry = find(&outline.nodes).expect("declared entry");
    assert_eq!(entry.id(), expected);
    let by_id = select_excerpt(content, &[format!("id:{}", entry.id()).parse().unwrap()]).unwrap();
    assert_eq!(
        by_id,
        select_excerpt(content, &[ContentSelector::path(entry.path())]).unwrap(),
    );
    assert!(mant_render::render_excerpt_text(&by_id).contains("UNIQUEBODY."));
}
