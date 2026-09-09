//! Copy and Markdown export share one URI boundary without host resolution.
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{LinkTarget, ReferenceScope, ReferenceTargetType};
use mant_loader::load_markdown_text;
use mant_protocol::{ReferenceProjection, ReferenceProjectionMode};
use mant_query::project_references;

fn targets(source: &str) -> (mant_ir::ResolvedContent, Vec<LinkTarget>) {
    let query = load_markdown_text(source, None).unwrap();
    let document = query.document.as_ref().unwrap();
    let inventory = project_references(
        document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            target_types: vec![
                ReferenceTargetType::Document,
                ReferenceTargetType::Manual,
                ReferenceTargetType::Local,
                ReferenceTargetType::Email,
                ReferenceTargetType::External,
            ],
            ..Default::default()
        },
    );
    let targets = inventory
        .records
        .into_iter()
        .map(|record| record.target)
        .collect();
    (query, targets)
}

#[test]
fn copied_typed_targets_survive_real_markdown_parsing_and_export() {
    for target in [
        LinkTarget::Document {
            name: "target".into(),
            fragment: Some("details".into()),
        },
        LinkTarget::Document {
            name: "../a b/日本%2F(1).md".into(),
            fragment: Some("Mixed.Target (%25)/part?#".into()),
        },
        LinkTarget::Document {
            name: "./percent%2520".into(),
            fragment: None,
        },
        LinkTarget::Manual {
            name: "demo(1)".into(),
            manual_section: None,
        },
        LinkTarget::Manual {
            name: "demo(1)".into(),
            manual_section: Some("3".into()),
        },
        LinkTarget::Manual {
            name: "a%28b".into(),
            manual_section: Some("3p".into()),
        },
        LinkTarget::Manual {
            name: "printf".into(),
            manual_section: None,
        },
        LinkTarget::Section {
            id: "details".into(),
        },
        LinkTarget::Email {
            address: "a%box&tag@example.org".into(),
        },
        LinkTarget::External {
            uri: "https://example.md/a%2528?q=%20#mixed".into(),
        },
    ] {
        let uri = target.to_uri().expect("representable typed target");
        let source = format!("# Probe\n\n## Details {{#details}}\n\n### [LINK](<{uri}>)\n");
        let (query, parsed) = targets(&source);
        assert_eq!(
            parsed.as_slice(),
            std::slice::from_ref(&target),
            "copy URI {uri}"
        );
        let exported = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
        let (_, reparsed) = targets(&exported);
        assert_eq!(reparsed, [target], "exported {exported}");
    }
}

#[test]
fn readable_reference_labels_are_not_reused_as_copy_destinations() {
    for (target, display, uri) in [
        (
            LinkTarget::Document {
                name: "target".into(),
                fragment: Some("details".into()),
            },
            "target#details",
            "target.md#details",
        ),
        (
            LinkTarget::Manual {
                name: "demo(1)".into(),
                manual_section: None,
            },
            "man:demo(1)",
            "man:demo%281%29",
        ),
    ] {
        assert_eq!(mant_render::reference_target_text(&target), display);
        assert_eq!(target.to_uri().as_deref(), Some(uri));
    }
}
