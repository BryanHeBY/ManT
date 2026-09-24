//! Definition diagnostics policy; coordinated by the parent discovery passes.
#![allow(clippy::similar_names)] // ContentContext and DefinitionContext are distinct inputs.
use super::context::{DefinitionContext, child_definition_context, definition_group_context};
use mant_ir::inline_plain_text as plain_text;
use mant_ir::{Block, ContentContext, DefinitionItem, EntryKind, Section, SourceSpan};

/// Report definition-shaped native content that a semantic section could not
/// classify without guessing.
pub(crate) fn manual_discovery_diagnostics(
    content: ContentContext<'_>,
    sections: &[Section],
) -> Vec<mant_ir::Diagnostic> {
    let mut diagnostics = Vec::new();
    visit_manual_discovery_sections(
        content,
        sections,
        DefinitionContext::Generic,
        &mut diagnostics,
    );
    diagnostics
}

fn visit_manual_discovery_sections(
    content: ContentContext<'_>,
    sections: &[Section],
    parent_context: DefinitionContext,
    output: &mut Vec<mant_ir::Diagnostic>,
) {
    for section in sections {
        let context =
            DefinitionContext::for_section(&section.heading.plain_text(content), parent_context);
        visit_manual_discovery_blocks(content, &section.blocks, context, true, output);
        visit_manual_discovery_sections(content, &section.children, context, output);
    }
}

fn visit_manual_discovery_blocks(
    content: ContentContext<'_>,
    blocks: &[Block],
    context: DefinitionContext,
    report_unclassified: bool,
    output: &mut Vec<mant_ir::Diagnostic>,
) {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    visit_manual_discovery_blocks(
                        content,
                        &item.blocks,
                        context,
                        report_unclassified,
                        output,
                    );
                }
            }
            Block::DefinitionList { items, source, .. } => {
                visit_manual_definition_items(
                    content,
                    items,
                    *source,
                    context,
                    report_unclassified,
                    output,
                );
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    visit_manual_discovery_blocks(
                        content,
                        &cell.blocks,
                        context,
                        report_unclassified,
                        output,
                    );
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::FixedDisplay { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn visit_manual_definition_items(
    content: ContentContext<'_>,
    items: &[DefinitionItem],
    source: Option<SourceSpan>,
    context: DefinitionContext,
    report_unclassified: bool,
    output: &mut Vec<mant_ir::Diagnostic>,
) {
    let item_context = definition_group_context(content, items, context);
    for item in items {
        let identity = item.entry.as_ref();
        let role = identity.map_or(EntryKind::Term, |identity| identity.kind);
        if report_unclassified
            && item_context != DefinitionContext::Generic
            && role == EntryKind::Term
            && identity.is_some_and(|identity| identity.names.is_empty())
        {
            report_unclassified_definition(content, item, item_context, source, output);
        }
        visit_manual_discovery_blocks(
            content,
            &item.description,
            child_definition_context(role, item_context),
            false,
            output,
        );
    }
}

fn report_unclassified_definition(
    content: ContentContext<'_>,
    item: &DefinitionItem,
    context: DefinitionContext,
    source: Option<SourceSpan>,
    output: &mut Vec<mant_ir::Diagnostic>,
) {
    let term = item
        .terms
        .first()
        .map_or_else(String::new, |term| plain_text(content, term));
    if term.trim().is_empty() {
        return;
    }
    output.push(mant_ir::Diagnostic {
        impact: mant_ir::DiagnosticImpact::SemanticCoverage,
        level: mant_ir::DiagnosticLevel::Warning,
        code: Some("manual.semantic-entry.unclassified-definition".to_owned()),
        message: format!(
            "definition term '{}' did not match the complete {} name grammar and remains an unclassified term",
            term.trim(),
            context.label()
        ),
        source,
        source_key: None,
        coverage_scope: None,
    });
}
