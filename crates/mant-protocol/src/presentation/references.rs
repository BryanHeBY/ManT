//! One deterministic reference presentation shared by terminal and agent hosts.

use crate::{
    ReferenceCount, ReferenceInventory, ReferenceProjectionMode, ReferenceResolution,
    TextPresentation, TextRole, sanitize_terminal_text,
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
        let status = match &record.resolution {
            ReferenceResolution::NotApplicable {} => "not probed".to_owned(),
            ReferenceResolution::NotQueried { .. } => {
                "document not queried; fragment unchecked".to_owned()
            }
            ReferenceResolution::MissingContext { .. } => {
                "source has no registered namespace; target unchecked".to_owned()
            }
            ReferenceResolution::LogicalAddress { address, .. } => format!(
                "logical address={}; document not loaded; fragment unchecked",
                address.catalog_path()
            ),
            ReferenceResolution::Loaded { fragment, .. } => {
                format!("loaded source; fragment={fragment:?}")
            }
            ReferenceResolution::Restricted {} => "restricted target".to_owned(),
        };
        lines.push(format!("  {}", paint(TextRole::Notice, &status)));
    }
    lines.join("\n")
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
