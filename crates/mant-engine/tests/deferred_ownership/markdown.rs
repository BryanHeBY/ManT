//! Portable fences flatten annotation by policy. Ordinary labels still retain
//! their exact original owner; neither reader can absorb an earlier recovery.
use crate::{cases, json_roundtrip};
use mant_codec::encode::{
    MarkdownOptions, render_addressable_markdown_with_options, render_markdown_with_options,
};
use mant_ir::{LinkTarget, ResolvedContent};

pub(super) fn check(original: &ResolvedContent, case: &cases::Case) {
    let original_snapshot = cases::snapshot(original);
    for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
        let mut expected = case.clone();
        let mut ordinals: Vec<Option<usize>> = Vec::new();
        expected.links = case
            .links
            .iter()
            .zip(&original_snapshot.links)
            .filter_map(|(link, actual)| {
                // The frozen format policy flattens literal/table fences, and
                // default export omits local section wrappers. This selects a
                // policy, never invents a label expectation from candidate IR.
                let retained = !(actual.owner.contains("/preformatted")
                    || actual.owner.contains("/row/")
                    || options == MarkdownOptions::default()
                        && matches!(link.target, LinkTarget::Section { .. }));
                ordinals.push(
                    retained.then(|| ordinals.iter().filter(|ordinal| ordinal.is_some()).count()),
                );
                retained.then(|| link.clone())
            })
            .collect();
        for recovery in &mut expected.recoveries {
            recovery.link = recovery.link.and_then(|ordinal| ordinals[ordinal]);
        }
        for direct in [false, true] {
            let mut current = original.clone();
            for round in 1..=2 {
                let markdown = render_markdown_with_options(&current, options);
                let artifact = render_addressable_markdown_with_options(&current, options);
                assert_eq!(artifact.text(), markdown, "{} round{round}", case.id);
                assert!(!markdown.contains("mant:deferred-origin:"));
                for node in artifact.nodes() {
                    assert!(
                        markdown.get(node.range()).is_some(),
                        "{} artifact byte boundary",
                        case.id
                    );
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
                current = json_roundtrip::json_string_roundtrip(&current);
                assert_eq!(
                    mant_ir::validate_document(current.document.as_ref().unwrap()),
                    []
                );
                // Every recovery's expected spelling, source ordinal and
                // projected link membership comes from the sealed source.
                cases::assert_rich(&current, &expected, false);
                let snapshot = cases::snapshot(&current);
                cases::check_portable(case, &snapshot, &ordinals);
                // Where the portable representation retains inline style,
                // compare each source member, not only stripped full text.
                for recovery in &case.recoveries {
                    let original_text = original_snapshot.text();
                    let old = cases::positions(&original_text, &recovery.text)[recovery.ordinal];
                    if original_snapshot.cells[old].owner.contains("/preformatted")
                        || original_snapshot.cells[old].owner.contains("/row/")
                    {
                        continue;
                    }
                    let text = snapshot.text();
                    let at = cases::positions(&text, &recovery.text)[recovery.ordinal];
                    for cell in &snapshot.cells[at..at + recovery.text.chars().count()] {
                        assert_eq!(cell.style, recovery.style, "{} round{round}", case.id);
                    }
                }
                super::queries::check(&current, &expected);
            }
        }
    }
}
