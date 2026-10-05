//! Visit source-neutral content, entry domains and local layout invariants.
use super::diagnostics::invariant;
use crate::validation::{
    links::{is_valid_email_address, is_valid_external_uri},
    source::validate_source_span,
};
use crate::{
    Block, DefinitionItem, Diagnostic, DocumentReference, Inline, LinkTarget, NodeId, Section,
    ValueDomain,
    visit::{self, Visit},
};

#[derive(Default)]
pub(super) struct InvariantCollector {
    pub(super) section_targets: Vec<NodeId>,
    pub(super) diagnostics: Vec<Diagnostic>,
}

impl InvariantCollector {
    fn validate_inline_layout(&mut self, content: &[Inline], layout: &crate::InlineLayout) {
        if let Err(message) = layout.validate(content) {
            self.diagnostics
                .push(invariant("ir.invalid-inline-layout", message.to_owned()));
        }
    }
    fn validate_entry(&mut self, item: crate::EntryOwner<'_>) {
        if let Some(source) = item.source() {
            validate_source_span(&mut self.diagnostics, source);
        }
        if item.facts().is_some() && item.forms().is_none() {
            self.diagnostics.push(invariant(
                "ir.invalid-entry-content",
                "entry form references must address valid owner content".to_owned(),
            ));
        }
        if matches!(
            item.facts()
                .and_then(|identity| identity.value_domain.as_ref()),
            Some(ValueDomain::Choices { .. })
        ) && !item.has_value_choices()
        {
            self.diagnostics.push(invariant(
                "ir.invalid-entry-choices",
                "a choices domain requires nonempty direct semantic children of kind value"
                    .to_owned(),
            ));
        }
        if let Some(ValueDomain::EntrySet {
            reference,
            entry_kinds,
            source,
        }) = item
            .facts()
            .and_then(|identity| identity.value_domain.as_ref())
        {
            validate_semantic_document_reference(&mut self.diagnostics, reference);
            if let Some(source) = source {
                validate_source_span(&mut self.diagnostics, *source);
            }
            if entry_kinds.is_empty() {
                self.diagnostics.push(invariant(
                    "ir.empty-entry-value-domain",
                    "cross-document entry value domain must select at least one entry kind"
                        .to_owned(),
                ));
            }
            if entry_kinds
                .iter()
                .enumerate()
                .any(|(index, kind)| entry_kinds[..index].contains(kind))
            {
                self.diagnostics.push(invariant(
                    "ir.duplicate-entry-value-kind",
                    "cross-document entry value domain must not repeat entry kinds".to_owned(),
                ));
            }
        }
    }
}

impl<'ir> Visit<'ir> for InvariantCollector {
    fn visit_heading(&mut self, heading: &'ir crate::Heading) {
        self.validate_inline_layout(&heading.content, &heading.inline_layout);
        if let Some(source) = heading.source {
            validate_source_span(&mut self.diagnostics, source);
        }
        visit::walk_heading(self, heading);
    }
    fn visit_section(&mut self, section: &'ir Section) {
        if let Some(source) = section.source {
            validate_source_span(&mut self.diagnostics, source);
        }
        visit::walk_section(self, section);
    }

    fn visit_block(&mut self, block: &'ir Block) {
        if let Block::Paragraph {
            children,
            inline_layout,
            ..
        }
        | Block::Preformatted {
            children,
            inline_layout,
            ..
        } = block
        {
            self.validate_inline_layout(children, inline_layout);
        }
        if let Block::Equation {
            value,
            expression: Some(expression),
            ..
        } = block
            && *value != expression.readable_text()
        {
            self.diagnostics.push(invariant(
                "ir.equation-projection-mismatch",
                "equation text does not match its owned expression".to_owned(),
            ));
        }
        if let Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = block
        {
            let mut end = 0;
            for group in declaration_groups {
                if group.start_item < end || group.resolve(items).is_none() {
                    self.diagnostics.push(invariant(
                        "ir.invalid-declaration-group",
                        "declaration groups must be ordered, disjoint, in bounds and end in readable context after empty heads".to_owned(),
                    ));
                }
                end = group.end_item;
            }
        }
        let source = match block {
            Block::Paragraph { source, .. }
            | Block::Preformatted { source, .. }
            | Block::List { source, .. }
            | Block::DefinitionList { source, .. }
            | Block::Table { source, .. }
            | Block::Equation { source, .. }
            | Block::VerticalSpace { source, .. }
            | Block::ThematicBreak { source }
            | Block::Unsupported { source, .. } => *source,
        };
        if let Some(source) = source {
            validate_source_span(&mut self.diagnostics, source);
        }
        if let Block::Table { rows, .. } = block {
            for row in rows {
                if !matches!(&row.kind, crate::TableRowKind::Data) && !row.cells.is_empty() {
                    self.diagnostics.push(invariant(
                        "ir.invalid-table-rule-cells",
                        "whole-row table rules must not contain data cells".to_owned(),
                    ));
                }
                if let crate::TableRowKind::LayoutRule { cells } = &row.kind
                    && cells.is_empty()
                {
                    self.diagnostics.push(invariant(
                        "ir.empty-table-layout-rule",
                        "layout-only table rules must contain at least one rule cell".to_owned(),
                    ));
                }
            }
            for cell in rows.iter().flat_map(|row| &row.cells) {
                if cell.kind != crate::TableCellKind::Text && !cell.blocks.is_empty() {
                    self.diagnostics.push(invariant(
                        "ir.invalid-table-rule-content",
                        "table rule cells must not contain ordinary block content".to_owned(),
                    ));
                }
                if cell.column_span == 0 || cell.row_span == 0 {
                    self.diagnostics.push(invariant(
                        "ir.invalid-table-span",
                        "table row and column spans must be at least one".to_owned(),
                    ));
                }
            }
        }
        visit::walk_block(self, block);
    }

    fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
        for term in &item.terms {
            self.validate_inline_layout(&term.content, &term.inline_layout);
        }
        self.validate_entry(crate::EntryOwner::Definition(item));
        visit::walk_definition_item(self, item);
    }

    fn visit_list_item(&mut self, item: &'ir crate::ListItem) {
        self.validate_entry(crate::EntryOwner::List(item));
        visit::walk_list_item(self, item);
    }

    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Equation { value, expression } = inline
            && *value != expression.readable_text()
        {
            self.diagnostics.push(invariant(
                "ir.equation-projection-mismatch",
                "inline equation text does not match its owned expression".to_owned(),
            ));
        }
        match inline {
            Inline::Link {
                target: LinkTarget::Section { id },
                ..
            } => self.section_targets.push(id.clone()),
            Inline::Link {
                target: LinkTarget::External { uri },
                ..
            } if !is_valid_external_uri(uri) => self.diagnostics.push(invariant(
                "ir.invalid-external-uri",
                format!("external link target '{uri}' is not an absolute URI"),
            )),
            Inline::Link {
                target: LinkTarget::Email { address },
                ..
            } if !is_valid_email_address(address) => self.diagnostics.push(invariant(
                "ir.invalid-email-address",
                format!("email link target '{address}' is not a valid mailbox"),
            )),
            _ => {}
        }
        visit::walk_inline(self, inline);
    }
}

fn validate_semantic_document_reference(
    diagnostics: &mut Vec<Diagnostic>,
    reference: &DocumentReference,
) {
    let empty = match reference {
        DocumentReference::Document { name, fragment } => {
            name.trim().is_empty()
                || fragment
                    .as_deref()
                    .is_some_and(|fragment| fragment.trim().is_empty())
        }
        DocumentReference::Manual {
            name,
            manual_section,
        } => {
            name.trim().is_empty()
                || manual_section
                    .as_deref()
                    .is_some_and(|section| section.trim().is_empty())
        }
    };
    if empty {
        diagnostics.push(invariant(
            "ir.empty-semantic-document-reference",
            "semantic document reference components must not be empty".to_owned(),
        ));
    } else if !reference.is_well_formed() {
        diagnostics.push(invariant(
            "ir.invalid-semantic-document-reference",
            "semantic document reference does not follow the document or manual grammar".to_owned(),
        ));
    }
}
