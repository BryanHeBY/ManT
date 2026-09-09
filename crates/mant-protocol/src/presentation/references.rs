//! One deterministic reference presentation shared by terminal and agent hosts.

use crate::{
    ReferenceCount, ReferenceInventory, ReferenceProjectionMode, ReferenceResolution,
    TextPresentation, TextRole, UnloadedFragment, sanitize_terminal_text,
};

/// Present independent reference counts and the already bounded occurrence page.
/// No destination is loaded and no labels or links are rediscovered from text.
#[must_use]
pub fn render_reference_inventory(inventory: &ReferenceInventory) -> String {
    render_reference_inventory_with(inventory, |_, text| text.to_owned())
}

/// Decorate reference facts without changing their text, order or coordinates.
#[must_use]
pub fn render_reference_inventory_with(
    inventory: &ReferenceInventory,
    decorate: impl Fn(TextPresentation, &str) -> String,
) -> String {
    if inventory.policy.mode == ReferenceProjectionMode::None {
        return String::new();
    }
    let paint = |role: TextRole, text: &str| decorate(role.into(), &sanitize_terminal_text(text));
    let mut lines = vec![paint(
        TextRole::Notice,
        &format!(
            "References: occurrences={}, targets={}; coverage={:?}; offset={}, returned={}",
            count(&inventory.occurrences),
            count(&inventory.targets),
            inventory.coverage.status,
            inventory.page.offset,
            inventory.page.returned,
        ),
    )];
    if let Some(next) = inventory.page.next_offset {
        lines.push(paint(
            TextRole::Notice,
            &format!("nextReferenceOffset={next}"),
        ));
    }
    if let Some(limit) = inventory.page.limited {
        lines.push(paint(
            TextRole::Notice,
            &format!("Reference page limited: {limit:?}"),
        ));
    }
    for record in &inventory.records {
        let target = target_text(&record.target);
        let label = if record.label.is_empty() {
            "(empty label)"
        } else {
            &record.label
        };
        lines.push(format!(
            "- {}{} → {}",
            paint(TextRole::Heading, label),
            if record.label_truncated { "…" } else { "" },
            paint(TextRole::Path, &target)
        ));
        let position = format!("{:?}", record.origin);
        lines.push(format!(
            "  {}",
            paint(TextRole::Path, &format!("source={position}"))
        ));
        lines.push(format!(
            "  {}",
            paint(
                TextRole::Path,
                &format!("readSource={}", record.source_read)
            )
        ));
        let status = resolution_text(&record.resolution);
        lines.push(format!("  {}", paint(TextRole::Notice, &status)));
    }
    lines.join("\n")
}

fn resolution_text(resolution: &ReferenceResolution) -> String {
    match resolution {
        ReferenceResolution::NotApplicable {} => "not probed".to_owned(),
        ReferenceResolution::NotQueried { fragment } => {
            format!("document not queried; {}", unloaded_fragment_text(fragment))
        }
        ReferenceResolution::MissingContext { fragment } => {
            format!(
                "source has no registered namespace; target unchecked; {}",
                unloaded_fragment_text(fragment)
            )
        }
        ReferenceResolution::LogicalAddress { address, fragment } => format!(
            "logical address={}; document not loaded; {}",
            address.catalog_path(),
            unloaded_fragment_text(fragment)
        ),
        ReferenceResolution::Loaded { fragment, .. } => {
            format!("loaded source; fragment={fragment:?}")
        }
        ReferenceResolution::Restricted {} => "restricted target".to_owned(),
    }
}

fn unloaded_fragment_text(fragment: &UnloadedFragment) -> &'static str {
    match fragment {
        UnloadedFragment::Absent {} => "no fragment",
        UnloadedFragment::Unchecked {} => "fragment unchecked",
    }
}

fn count(count: &ReferenceCount) -> String {
    match count {
        ReferenceCount::Exact { value } => format!("exact({value})"),
        ReferenceCount::LowerBound { value } => format!("lower-bound({value})"),
        ReferenceCount::Unknown { reason } => format!("unknown({reason:?})"),
    }
}

fn target_text(target: &mant_ir::LinkTarget) -> String {
    match target {
        mant_ir::LinkTarget::Document { name, fragment } => match fragment {
            Some(fragment) => format!("{name}#{fragment}"),
            None => name.clone(),
        },
        mant_ir::LinkTarget::Manual {
            name,
            manual_section,
        } => match manual_section {
            Some(section) => format!("{name}({section})"),
            None => format!("man:{name} (section not selected)"),
        },
        mant_ir::LinkTarget::Section { id } => format!("#{id}"),
        mant_ir::LinkTarget::External { uri } => uri.clone(),
        mant_ir::LinkTarget::Email { address } => format!("mailto:{address}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_and_unchecked_fragments_remain_distinct_in_every_unloaded_stage() {
        for fragment in [UnloadedFragment::Absent {}, UnloadedFragment::Unchecked {}] {
            let expected = unloaded_fragment_text(&fragment);
            let resolutions = [
                ReferenceResolution::NotQueried {
                    fragment: fragment.clone(),
                },
                ReferenceResolution::MissingContext {
                    fragment: fragment.clone(),
                },
                ReferenceResolution::LogicalAddress {
                    address: crate::DocumentAddress::Manual {
                        name: "printf".into(),
                        manual_section: "3".into(),
                    },
                    fragment,
                },
            ];
            for resolution in resolutions {
                let text = resolution_text(&resolution);
                assert!(text.ends_with(expected), "{text}");
                assert_eq!(
                    text.contains("fragment unchecked"),
                    expected == "fragment unchecked"
                );
                assert!(!text.contains("fragment valid"));
            }
        }
    }
}
