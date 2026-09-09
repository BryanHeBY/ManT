//! Derived navigation capabilities, not another content or semantic index.

use std::collections::HashSet;

use mant_ir::{ContentLocation, LinkTarget};

use super::{references::target_parts, sanitize_terminal_text};

/// Where a real occurrence may be presented relative to its original owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceAttachment {
    /// Original document or section heading content.
    Heading,
    /// An occurrence overlapping at least one fully validated original form.
    Form,
    /// Ordinary content, including unavailable or rejected form associations.
    Body,
}

/// Classify structural evidence without inspecting display labels or targets.
///
/// `valid_forms` must come from a complete form-binding validation result;
/// invalid, unrecorded and limited associations pass `None`. The caller must
/// still verify that the original owner is uniquely present in its visible
/// tree. A hidden or ambiguous owner uses a fallback reference group, not an
/// association with a similarly named visible node.
#[must_use]
pub fn reference_attachment(
    origin: &ContentLocation,
    valid_forms: Option<&[u32]>,
) -> ReferenceAttachment {
    match origin {
        ContentLocation::DocumentHeading { .. } | ContentLocation::SectionHeading { .. } => {
            ReferenceAttachment::Heading
        }
        ContentLocation::Content { .. } if valid_forms.is_some_and(|forms| !forms.is_empty()) => {
            ReferenceAttachment::Form
        }
        ContentLocation::Content { .. } => ReferenceAttachment::Body,
    }
}

/// Render a compact capability from an already bounded occurrence collection.
///
/// Exact typed targets (including fragments and manual sections) are deduplicated
/// only for this badge. Neither occurrence records nor source order are changed.
/// `complete` means this collection covers the owner's entire selected reference
/// scope, not that any destination has been loaded or validated. Partial pages
/// must pass `false`, even if their only record happens to be the only known link.
/// At most 1,000 records are inspected and a single-target label is capped at
/// 160 Unicode scalars. This helper never initiates discovery or target lookup.
#[must_use]
pub fn reference_badge<'a>(
    targets: impl IntoIterator<Item = &'a LinkTarget>,
    mut complete: bool,
) -> String {
    let mut unique = HashSet::new();
    let mut first = None;
    for (index, target) in targets.into_iter().take(1001).enumerate() {
        if index == 1000 {
            complete = false;
            break;
        }
        unique.insert(target_key(target));
        first.get_or_insert(target);
    }
    let Some(first) = first else {
        return String::new();
    };
    let mut badge = if unique.len() == 1 {
        let mut chars = target_parts(first).into_iter().flat_map(str::chars);
        let label: String = chars.by_ref().take(160).collect();
        let label = sanitize_terminal_text(&label);
        format!("↗ {label}{}", if chars.next().is_some() { "…" } else { "" })
    } else {
        format!("↗ {} targets", unique.len())
    };
    if !complete {
        badge.push_str(" (known; more may exist)");
    }
    badge
}

fn target_key(target: &LinkTarget) -> (u8, &str, Option<&str>) {
    match target {
        LinkTarget::Document { name, fragment } => (0, name, fragment.as_deref()),
        LinkTarget::Manual {
            name,
            manual_section,
        } => (1, name, manual_section.as_deref()),
        LinkTarget::Section { id } => (2, id.as_str(), None),
        LinkTarget::External { uri } => (3, uri, None),
        LinkTarget::Email { address } => (4, address, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_requires_structural_or_valid_form_evidence() {
        let heading = ContentLocation::DocumentHeading { path: vec![0] };
        assert_eq!(
            reference_attachment(&heading, None),
            ReferenceAttachment::Heading
        );
        let body = ContentLocation::Content {
            sections: vec![0],
            blocks: vec![],
            root: mant_ir::ContentInlineRoot::Inlines,
            path: vec![0],
        };
        assert_eq!(reference_attachment(&body, None), ReferenceAttachment::Body);
        assert_eq!(
            reference_attachment(&body, Some(&[])),
            ReferenceAttachment::Body
        );
        assert_eq!(
            reference_attachment(&body, Some(&[1])),
            ReferenceAttachment::Form
        );
    }

    #[test]
    fn badges_deduplicate_typed_targets_not_names_or_occurrences() {
        let first = LinkTarget::Document {
            name: "tool".into(),
            fragment: Some("Mixed.Target".into()),
        };
        let other = LinkTarget::Document {
            name: "tool".into(),
            fragment: Some("other".into()),
        };
        let manual = LinkTarget::Manual {
            name: "tool".into(),
            manual_section: Some("1".into()),
        };
        assert_eq!(
            reference_badge([&first, &first], true),
            "↗ tool#Mixed.Target"
        );
        assert_eq!(
            reference_badge([&first, &other, &manual], true),
            "↗ 3 targets"
        );
        assert!(reference_badge([&first], false).contains("more may exist"));
        assert!(
            reference_badge(std::iter::repeat_n(&first, 1001), true).contains("more may exist")
        );
        assert_eq!(reference_badge([], false), "");
    }

    #[test]
    fn badges_bound_visible_text_and_never_emit_terminal_controls() {
        let target = LinkTarget::External {
            uri: format!("https://example/\x1b{}", "界".repeat(1000)),
        };
        let text = reference_badge([&target], true);
        assert!(text.ends_with('…'));
        assert!(!text.chars().any(char::is_control));
        assert!(text.chars().count() <= 163);
    }
}
