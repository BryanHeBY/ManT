//! Conservative semantic export: never infer relationships from shared heads.
use mant_ir::{
    Block, DefinitionCase, DefinitionItem, DefinitionRole, Document, EntryFacts, EntryKind,
    EntryNameEvidence, ParameterKind, SemanticDocumentReference, ValueDomain,
    visit::{self, Visit},
};

pub(super) fn supported(document: &Document) -> bool {
    struct Check(bool);
    impl<'a> Visit<'a> for Check {
        fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
            self.0 &= item.identity.is_none();
            visit::walk_definition_item(self, item);
        }
        fn visit_block(&mut self, block: &'a Block) {
            if let Block::List { items, .. } = block
                && let Some(first) = items.iter().find_map(|item| item.entry.as_ref())
            {
                self.0 &= crate::markdown::export_attached_policy(items).is_some()
                    && items.iter().all(|item| {
                        item.entry.as_ref().is_some_and(|facts| {
                            facts.role == first.role
                                && facts.case == first.case
                                && !facts.name_bindings.is_empty()
                                && facts
                                    .name_bindings
                                    .iter()
                                    .all(|b| b.evidence == EntryNameEvidence::Declared)
                                && matches!(item.blocks.first(), Some(Block::Paragraph { .. }))
                                && crate::markdown::export_entry_metadata(facts).is_some()
                                && facts
                                    .value_domain
                                    .as_ref()
                                    .is_none_or(|v| domain(v).is_some())
                        })
                    });
            }
            visit::walk_block(self, block);
        }
    }
    let mut check = Check(
        mant_ir::validate_document(document).is_empty()
            && crate::projection::semantics_complete(&document.diagnostics),
    );
    check.visit_document(document);
    check.0
}

pub(super) fn declaration(facts: &EntryFacts, items: &[mant_ir::ListItem]) -> String {
    format!(
        "<!-- mant:entries role={} case={}{} -->",
        role(facts.role),
        match facts.case {
            DefinitionCase::Sensitive => "sensitive",
            DefinitionCase::Insensitive => "insensitive",
        },
        crate::markdown::export_attached_policy(items).expect("supported semantic export list")
    )
}

pub(super) fn metadata(facts: &EntryFacts) -> String {
    crate::markdown::export_entry_metadata(facts).expect("supported semantic export metadata")
}

pub(super) fn domain(value: &ValueDomain) -> Option<String> {
    let fields = match value {
        ValueDomain::Choices { exhaustive } => format!(
            "choices={}",
            if *exhaustive { "exhaustive" } else { "open" }
        ),
        ValueDomain::EntrySet {
            reference,
            entry_kinds,
            ..
        } => {
            let target = match reference {
                SemanticDocumentReference::Document {
                    name,
                    fragment: None,
                } => crate::markdown::link_destination::document_destination(name, None),
                SemanticDocumentReference::Manual {
                    name,
                    manual_section: Some(section),
                } if !name.contains(char::is_whitespace) => format!("manual/{section}/{name}"),
                _ => return None,
            };
            format!(
                "entries={target} roles={}",
                entry_kinds
                    .iter()
                    .map(|kind| match kind {
                        EntryKind::Command => "command",
                        EntryKind::Parameter {
                            parameter_kind: ParameterKind::Option,
                        } => "option",
                        EntryKind::Parameter {
                            parameter_kind: ParameterKind::Marker,
                        } => "marker",
                        EntryKind::Parameter {
                            parameter_kind: ParameterKind::Operand,
                        } => "operand",
                        EntryKind::ConfigurationKey => "configuration-key",
                        EntryKind::EnvironmentVariable => "environment-variable",
                        EntryKind::Variable => "variable",
                        EntryKind::Value => "value",
                        EntryKind::Term => "term",
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    };
    Some(format!("<!-- mant:domain {fields} -->"))
}

fn role(role: DefinitionRole) -> &'static str {
    match role {
        DefinitionRole::Option => "option",
        DefinitionRole::Command => "command",
        DefinitionRole::Variable => "variable",
        DefinitionRole::EnvironmentVariable => "environment-variable",
        DefinitionRole::ConfigurationKey => "configuration-key",
        DefinitionRole::Marker => "marker",
        DefinitionRole::Operand => "operand",
        DefinitionRole::Value => "value",
        DefinitionRole::Term => "term",
    }
}
