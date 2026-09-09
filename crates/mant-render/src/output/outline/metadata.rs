//! Outline relationship and summary labels over returned protocol facts only.

use mant_ir::{EntryKind, EntrySummary, ParameterKind};
use mant_protocol::{EntryDocumentTarget, EntryValueDomain, OutlineNode};

/// Render the semantic-relationship suffix for one outline node.
///
/// Terminal frontends should style this already-rendered suffix rather than
/// reconstructing document targets or value domains independently.
#[must_use]
pub fn render_outline_relationships(node: &OutlineNode) -> String {
    let OutlineNode::DocumentEntry {
        document_targets,
        value_domain,
        alias_groups,
        alias_of,
        ..
    } = node
    else {
        return String::new();
    };
    let mut relationships = Vec::new();
    if !alias_groups.is_empty() {
        relationships.push(format!(
            "alias groups: {}",
            alias_groups
                .iter()
                .map(|group| group.join(" = "))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if let Some(target) = alias_of {
        relationships.push(format!("alias of: {target}"));
    }
    if !document_targets.is_empty() {
        relationships.push(format!(
            "documents: {}",
            document_targets
                .iter()
                .map(entry_document_label)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(EntryValueDomain::EntrySet {
        reference,
        address,
        entry_kinds,
    }) = value_domain.as_deref()
    {
        let document = resolved_reference_label(reference, address.as_ref());
        let kinds = entry_kinds
            .iter()
            .map(|kind| entry_kind_label(*kind, true))
            .collect::<Vec<_>>()
            .join(", ");
        relationships.push(format!("values: {kinds} in {document}"));
    }
    if relationships.is_empty() {
        String::new()
    } else {
        format!(" — {}", relationships.join("; "))
    }
}

fn entry_document_label(target: &EntryDocumentTarget) -> String {
    let destination = resolved_reference_label(&target.reference, target.address.as_ref());
    if target.label == destination {
        destination
    } else {
        format!("{} → {destination}", target.label)
    }
}

fn resolved_reference_label(
    reference: &mant_ir::DocumentReference,
    address: Option<&mant_ir::DocumentAddress>,
) -> String {
    let Some(address) = address else {
        return semantic_reference_label(reference);
    };
    let mut destination = address.catalog_path();
    if let mant_ir::DocumentReference::Document {
        fragment: Some(fragment),
        ..
    } = reference
    {
        destination.push('#');
        destination.push_str(fragment);
    }
    destination
}

fn semantic_reference_label(reference: &mant_ir::DocumentReference) -> String {
    match reference {
        mant_ir::DocumentReference::Document { name, fragment } => fragment
            .as_ref()
            .map_or_else(|| name.clone(), |fragment| format!("{name}#{fragment}")),
        mant_ir::DocumentReference::Manual {
            name,
            manual_section,
        } => manual_section.as_ref().map_or_else(
            || name.clone(),
            |section| format!("manual/{section}/{name}"),
        ),
    }
}

pub(in crate::output) fn outline_summary(node: &OutlineNode) -> Option<&EntrySummary> {
    match node {
        OutlineNode::DocumentRoot { entry_summary, .. }
        | OutlineNode::DocumentSection { entry_summary, .. }
        | OutlineNode::DocumentEntry { entry_summary, .. } => entry_summary.as_ref(),
        OutlineNode::Tldr { .. } => None,
    }
}

/// Render one compact semantic-entry summary suffix shared by terminal transports.
#[must_use]
pub fn render_outline_entry_summary(summary: &EntrySummary) -> String {
    if summary.is_empty() {
        return String::new();
    }
    let mut counts = summary
        .by_kind
        .iter()
        .map(|count| {
            format!(
                "{} {}",
                count.count,
                entry_kind_label(count.kind, count.count == 1)
            )
        })
        .collect::<Vec<_>>();
    counts.push(format!(
        "{} {}",
        summary.forms,
        if summary.forms == 1 { "form" } else { "forms" }
    ));
    format!(
        " — {} direct, {} nested ({})",
        summary.direct,
        summary.descendants,
        counts.join(", ")
    )
}

pub(in crate::output) const fn entry_kind_label(kind: EntryKind, singular: bool) -> &'static str {
    match (kind, singular) {
        (EntryKind::Command, true) => "command",
        (EntryKind::Command, false) => "commands",
        (
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            },
            true,
        ) => "option",
        (
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            },
            false,
        ) => "options",
        (
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Marker,
            },
            true,
        ) => "marker",
        (
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Marker,
            },
            false,
        ) => "markers",
        (
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Operand,
            },
            true,
        ) => "operand",
        (
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Operand,
            },
            false,
        ) => "operands",
        (EntryKind::ConfigurationKey, true) => "configuration key",
        (EntryKind::ConfigurationKey, false) => "configuration keys",
        (EntryKind::EnvironmentVariable, true) => "environment variable",
        (EntryKind::EnvironmentVariable, false) => "environment variables",
        (EntryKind::Variable, true) => "variable",
        (EntryKind::Variable, false) => "variables",
        (EntryKind::Value, true) => "value",
        (EntryKind::Value, false) => "values",
        (EntryKind::Term, true) => "term",
        (EntryKind::Term, false) => "terms",
    }
}
