//! Audits source-side mdoc ordinal candidates against their final IR shape.

use std::collections::HashMap;

use libmandoc_rs::{DefinitionListStyle, Node, NodeKind, NormalizedListKind};
use mant_ir::{Block, Document, ListKind, Section, SourceSpan};
use serde::Serialize;

use super::ordinal_labels;

type Origin = (u32, u32);
type ObservedLists = HashMap<Origin, Vec<ObservedBlock>>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OrdinalConversion {
    pub(super) source_line: u32,
    pub(super) list_style: &'static str,
    pub(super) terms: Vec<String>,
    pub(super) expected_disposition: &'static str,
    pub(super) observed_dispositions: Vec<&'static str>,
    pub(super) observed_ir_paths: Vec<String>,
}

pub(super) fn ordinal_conversions(root: &Node, document: &Document) -> Vec<OrdinalConversion> {
    let mut candidates = Vec::new();
    let mut slots = HashMap::new();
    collect_source_candidates(root, &mut slots, &mut candidates);
    let mut observed = ObservedLists::new();
    collect_blocks(&document.blocks, "document", &mut observed);
    for (index, section) in document.sections.iter().enumerate() {
        collect_section(section, &format!("section[{index}]"), &mut observed);
    }
    candidates
        .into_iter()
        .map(|candidate| {
            let matching = observed
                .get(&candidate.origin)
                // A removed sibling must not shift every later occurrence
                // and let an unrelated list satisfy the missing candidate.
                .filter(|blocks| blocks.len() == slots[&candidate.origin])
                .and_then(|blocks| blocks.get(candidate.slot));
            OrdinalConversion {
                source_line: candidate.origin.0,
                list_style: candidate.list_style,
                terms: candidate.terms,
                expected_disposition: candidate.expected_disposition,
                observed_dispositions: matching
                    .map(|block| block.disposition)
                    .into_iter()
                    .collect(),
                observed_ir_paths: matching
                    .map(|block| block.path.clone())
                    .into_iter()
                    .collect(),
            }
        })
        .collect()
}

pub(super) fn conversion_violations(conversions: &[OrdinalConversion]) -> Vec<String> {
    conversions
        .iter()
        .filter(|conversion| {
            !conversion
                .observed_dispositions
                .contains(&conversion.expected_disposition)
        })
        .map(|conversion| {
            format!(
                "mdoc -{} ordinal candidate at line {} expected {} but observed {:?}",
                conversion.list_style,
                conversion.source_line,
                conversion.expected_disposition,
                conversion.observed_dispositions
            )
        })
        .collect()
}

struct SourceCandidate {
    origin: Origin,
    slot: usize,
    list_style: &'static str,
    terms: Vec<String>,
    expected_disposition: &'static str,
}

fn collect_source_candidates(
    node: &Node,
    slots: &mut HashMap<Origin, usize>,
    output: &mut Vec<SourceCandidate>,
) {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        return;
    }
    if node.kind == NodeKind::Block
        && node.macro_token.as_deref() == Some("Bl")
        && node.list_kind != Some(NormalizedListKind::Column)
    {
        let origin = (node.line, node.column);
        let slot = slots.entry(origin).or_default();
        if node.list_kind == Some(NormalizedListKind::Definition)
            && let Some(style) = node.definition_list_style
            && let Some(labels) = ordinal_labels::observe(
                node.children
                    .iter()
                    .filter(|part| part.kind == NodeKind::Body)
                    .flat_map(|body| &body.children)
                    .filter(|item| item.macro_token.as_deref() == Some("It")),
            )
        {
            let recovered = style == DefinitionListStyle::Tag && labels.recoverable();
            output.push(SourceCandidate {
                origin,
                slot: *slot,
                list_style: style_name(style),
                terms: labels.terms,
                expected_disposition: if recovered {
                    "recovered-ordered-list"
                } else {
                    "retained-definition-list"
                },
            });
        }
        // Every independent actual list consumes a source slot. In
        // particular, a bullet sibling expanded at the same byte cannot
        // supply a definition or ordered candidate's ordinal slot.
        *slot += 1;
    }
    for child in &node.children {
        collect_source_candidates(child, slots, output);
    }
}

const fn style_name(style: DefinitionListStyle) -> &'static str {
    match style {
        DefinitionListStyle::Tag => "tag",
        DefinitionListStyle::Diagnostic => "diag",
        DefinitionListStyle::Hang => "hang",
        DefinitionListStyle::Inset => "inset",
        DefinitionListStyle::Overhang => "ohang",
    }
}

struct ObservedBlock {
    disposition: &'static str,
    path: String,
}

fn collect_section(section: &Section, path: &str, output: &mut ObservedLists) {
    collect_blocks(&section.blocks, path, output);
    for (index, child) in section.children.iter().enumerate() {
        collect_section(child, &format!("{path}/section[{index}]"), output);
    }
}

fn record_observed(
    output: &mut ObservedLists,
    source: Option<SourceSpan>,
    path: &str,
    disposition: &'static str,
) {
    if let Some(source) = source {
        output
            .entry((source.line, source.column))
            .or_default()
            .push(ObservedBlock {
                disposition,
                path: path.to_owned(),
            });
    }
}

fn collect_blocks(blocks: &[Block], path: &str, output: &mut ObservedLists) {
    for (index, block) in blocks.iter().enumerate() {
        let block_path = format!("{path}/block[{index}]");
        match block {
            Block::List {
                kind,
                source,
                items,
                ..
            } => {
                let disposition = if matches!(kind, ListKind::Ordered { .. }) {
                    "recovered-ordered-list"
                } else {
                    "other-list"
                };
                record_observed(output, *source, &block_path, disposition);
                for (item_index, item) in items.iter().enumerate() {
                    collect_blocks(
                        &item.blocks,
                        &format!("{block_path}/item[{item_index}]"),
                        output,
                    );
                }
            }
            Block::DefinitionList { source, items, .. } => {
                record_observed(output, *source, &block_path, "retained-definition-list");
                for (item_index, item) in items.iter().enumerate() {
                    collect_blocks(
                        &item.description,
                        &format!("{block_path}/definition[{item_index}]/description"),
                        output,
                    );
                }
            }
            Block::Table { rows, .. } => {
                for (row_index, row) in rows.iter().enumerate() {
                    for (cell_index, cell) in row.cells.iter().enumerate() {
                        collect_blocks(
                            &cell.blocks,
                            &format!("{block_path}/row[{row_index}]/cell[{cell_index}]"),
                            output,
                        );
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use libmandoc_rs::Parser;
    use mant_codec::lower_mandoc_document;
    use mant_ir::{Block, LayoutHint, ListKind};

    use super::{conversion_violations, ordinal_conversions};

    fn parse(style: &str) -> (libmandoc_rs::ParseReport, mant_ir::Document) {
        let source = format!(
            ".Dd September 4, 2026\n.Dt ORDINAL-AUDIT 7\n.Os\n.Sh EXAMPLES\n\
.Bl -{style} -width 1.\n.It 1.\nFirst.\n.It 2.\nSecond.\n.El\n"
        );
        let report = Parser::default()
            .parse_bytes("ordinal-audit.7", source.as_bytes())
            .expect("parse ordinal audit source");
        let document = lower_mandoc_document(Path::new("ordinal-audit.7"), &report);
        (report, document)
    }

    #[test]
    fn source_ledger_accepts_only_tag_recovery_and_all_other_definition_styles() {
        for style in ["tag", "diag", "hang", "inset", "ohang"] {
            let (report, document) = parse(style);
            let conversions = ordinal_conversions(&report.document.root, &document);
            assert_eq!(conversions.len(), 1, "-{style}");
            assert!(conversion_violations(&conversions).is_empty(), "-{style}");
            assert_eq!(
                conversions[0].expected_disposition,
                if style == "tag" {
                    "recovered-ordered-list"
                } else {
                    "retained-definition-list"
                }
            );
        }
    }

    #[test]
    fn independent_ordinal_truth_rejects_conversion_of_retained_labels() {
        for (source, recovered) in crate::ordinal_labels::cases::inputs() {
            let report = Parser::default()
                .parse_bytes("ordinal-audit.1", source.as_bytes())
                .unwrap();
            let mut document = lower_mandoc_document(Path::new("ordinal-audit.1"), &report);
            let conversions = ordinal_conversions(&report.document.root, &document);
            assert_eq!(conversions.len(), 1, "{source}");
            assert_eq!(
                conversions[0].expected_disposition,
                if recovered {
                    "recovered-ordered-list"
                } else {
                    "retained-definition-list"
                },
                "{source}"
            );
            assert!(
                conversion_violations(&conversions).is_empty(),
                "{source}: {conversions:?}"
            );
            if !recovered {
                let owner = &mut document.sections[1].blocks[0];
                let Block::DefinitionList {
                    items,
                    source: span,
                    ..
                } = owner
                else {
                    panic!("{source}");
                };
                *owner = Block::List {
                    kind: ListKind::Ordered { start: Some(1) },
                    compact: false,
                    items: items
                        .iter()
                        .map(|item| mant_ir::ListItem {
                            layout: mant_ir::ListItemLayout::default(),
                            source: item.source,
                            entry: None,
                            blocks: item.description.clone(),
                        })
                        .collect(),
                    layout: LayoutHint::default(),
                    source: *span,
                };
                let violations =
                    conversion_violations(&ordinal_conversions(&report.document.root, &document));
                assert!(
                    violations
                        .iter()
                        .any(|violation| violation.contains("expected retained-definition-list")),
                    "{source}: {violations:?}"
                );
            }
        }
    }

    #[test]
    fn same_source_coordinate_pairs_lists_by_occurrence_including_bullet_slots() {
        let source = crate::ordinal_labels::cases::SAME_ORIGIN;
        let report = Parser::default()
            .parse_bytes("ordinal-audit.1", source.as_bytes())
            .unwrap();
        let document = lower_mandoc_document(Path::new("ordinal-audit.1"), &report);
        let conversions = ordinal_conversions(&report.document.root, &document);
        assert_eq!(conversions.len(), 2);
        assert!(conversion_violations(&conversions).is_empty());
        for (slot, conversion) in (1..=2).zip(&conversions) {
            assert_eq!(conversion.source_line, 26);
            assert_eq!(
                conversion.observed_ir_paths,
                [format!("section[1]/block[{slot}]")]
            );
        }

        let mut swapped = document.clone();
        swapped.sections[1].blocks.swap(1, 2);
        let violations =
            conversion_violations(&ordinal_conversions(&report.document.root, &swapped));
        assert_eq!(violations.len(), 2, "{violations:?}");
        for removed in 0..=2 {
            let mut missing = document.clone();
            missing.sections[1].blocks.remove(removed);
            let violations =
                conversion_violations(&ordinal_conversions(&report.document.root, &missing));
            assert_eq!(
                violations.len(),
                2,
                "missing slot {removed}: {violations:?}"
            );
        }
    }

    #[test]
    fn source_ledger_rejects_an_overconversion_after_the_terms_disappear() {
        let (report, mut document) = parse("hang");
        let source = match &document.sections[0].blocks[0] {
            Block::DefinitionList { source, .. } => *source,
            block => panic!("expected retained definition list, got {block:?}"),
        };
        document.sections[0].blocks[0] = Block::List {
            kind: ListKind::Ordered { start: Some(1) },
            compact: false,
            items: Vec::new(),
            layout: LayoutHint::default(),
            source,
        };

        let conversions = ordinal_conversions(&report.document.root, &document);
        let violations = conversion_violations(&conversions);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("-hang"));
        assert!(violations[0].contains("expected retained-definition-list"));
    }
}
