//! Audits source-side mdoc ordinal candidates against their final IR shape.

use libmandoc_rs::{DefinitionListStyle, Node, NodeKind, NormalizedListKind};
use mant_ir::{Block, Document, ListKind, Section};
use serde::Serialize;

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
    collect_source_candidates(root, &mut candidates);
    let mut observed = Vec::new();
    collect_blocks(&document.blocks, "document", &mut observed);
    for (index, section) in document.sections.iter().enumerate() {
        collect_section(section, &format!("section[{index}]"), &mut observed);
    }
    candidates
        .into_iter()
        .map(|candidate| {
            let matching = observed
                .iter()
                .filter(|block| block.source_line == candidate.source_line)
                .collect::<Vec<_>>();
            OrdinalConversion {
                source_line: candidate.source_line,
                list_style: candidate.list_style,
                terms: candidate.terms,
                expected_disposition: candidate.expected_disposition,
                observed_dispositions: matching.iter().map(|block| block.disposition).collect(),
                observed_ir_paths: matching.iter().map(|block| block.path.clone()).collect(),
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
    source_line: u32,
    list_style: &'static str,
    terms: Vec<String>,
    expected_disposition: &'static str,
}

fn collect_source_candidates(node: &Node, output: &mut Vec<SourceCandidate>) {
    if node.macro_name.as_deref() == Some("Bl")
        && node.list_kind == Some(NormalizedListKind::Definition)
        && let Some(style) = node.definition_list_style
    {
        let items = direct_part(node, NodeKind::Body)
            .map(|body| {
                body.children
                    .iter()
                    .filter(|child| child.macro_name.as_deref() == Some("It"))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let terms = items
            .iter()
            .map(|item| {
                direct_part(item, NodeKind::Head)
                    .map(source_text)
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        if terms.iter().any(|term| ordinal(term).is_some()) {
            let complete_sequence = items.len() >= 2
                && items
                    .iter()
                    .all(|item| direct_part(item, NodeKind::Body).is_some_and(has_visible_text))
                && consecutive_sequence(&terms);
            output.push(SourceCandidate {
                source_line: node.line,
                list_style: style_name(style),
                terms,
                expected_disposition: if style == DefinitionListStyle::Tag && complete_sequence {
                    "recovered-ordered-list"
                } else {
                    "retained-definition-list"
                },
            });
        }
    }
    for child in &node.children {
        collect_source_candidates(child, output);
    }
}

fn direct_part(node: &Node, kind: NodeKind) -> Option<&Node> {
    node.children.iter().find(|child| child.kind == kind)
}

fn has_visible_text(node: &Node) -> bool {
    node.text
        .as_deref()
        .is_some_and(|text| !node.flags.no_print && !text.trim().is_empty())
        || node.children.iter().any(has_visible_text)
}

fn source_text(node: &Node) -> String {
    let mut fragments = Vec::new();
    collect_source_text(node, &mut fragments);
    fragments.join(" ")
}

fn collect_source_text(node: &Node, output: &mut Vec<String>) {
    if let Some(text) = node.text.as_deref().filter(|_| !node.flags.no_print) {
        let text = text.trim();
        if !text.is_empty() {
            output.push(text.to_owned());
        }
    }
    for child in &node.children {
        collect_source_text(child, output);
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum OrdinalStyle {
    Period,
    ClosingParenthesis,
    Parenthesized,
    Bracketed,
}

fn ordinal(value: &str) -> Option<(u64, OrdinalStyle)> {
    let value = value.trim();
    let (digits, style) = if let Some(digits) = value.strip_suffix('.') {
        (digits, OrdinalStyle::Period)
    } else if let Some(digits) = value.strip_suffix(')') {
        if let Some(digits) = digits.strip_prefix('(') {
            (digits, OrdinalStyle::Parenthesized)
        } else {
            (digits, OrdinalStyle::ClosingParenthesis)
        }
    } else {
        let digits = value
            .strip_prefix('[')
            .and_then(|digits| digits.strip_suffix(']'))?;
        (digits, OrdinalStyle::Bracketed)
    };
    digits.parse().ok().map(|value| (value, style))
}

fn consecutive_sequence(terms: &[String]) -> bool {
    let Some((mut previous, style)) = terms.first().and_then(|term| ordinal(term)) else {
        return false;
    };
    for term in &terms[1..] {
        let Some((current, current_style)) = ordinal(term) else {
            return false;
        };
        if current_style != style || previous.checked_add(1) != Some(current) {
            return false;
        }
        previous = current;
    }
    true
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
    source_line: u32,
    disposition: &'static str,
    path: String,
}

fn collect_section(section: &Section, path: &str, output: &mut Vec<ObservedBlock>) {
    collect_blocks(&section.blocks, path, output);
    for (index, child) in section.children.iter().enumerate() {
        collect_section(child, &format!("{path}/section[{index}]"), output);
    }
}

fn collect_blocks(blocks: &[Block], path: &str, output: &mut Vec<ObservedBlock>) {
    for (index, block) in blocks.iter().enumerate() {
        let block_path = format!("{path}/block[{index}]");
        match block {
            Block::List {
                kind: ListKind::Ordered,
                source,
                items,
                ..
            } => {
                output.push(ObservedBlock {
                    source_line: source.map_or(0, |source| source.line),
                    disposition: "recovered-ordered-list",
                    path: block_path.clone(),
                });
                for (item_index, item) in items.iter().enumerate() {
                    collect_blocks(
                        &item.blocks,
                        &format!("{block_path}/item[{item_index}]"),
                        output,
                    );
                }
            }
            Block::DefinitionList { source, items, .. } => {
                output.push(ObservedBlock {
                    source_line: source.map_or(0, |source| source.line),
                    disposition: "retained-definition-list",
                    path: block_path.clone(),
                });
                for (item_index, item) in items.iter().enumerate() {
                    collect_blocks(
                        &item.description,
                        &format!("{block_path}/definition[{item_index}]/description"),
                        output,
                    );
                }
            }
            Block::List { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    collect_blocks(
                        &item.blocks,
                        &format!("{block_path}/item[{item_index}]"),
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
    use mant_engine::lower_mandoc_document;
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
    fn source_ledger_rejects_an_overconversion_after_the_terms_disappear() {
        let (report, mut document) = parse("hang");
        let source = match &document.sections[0].blocks[0] {
            Block::DefinitionList { source, .. } => *source,
            block => panic!("expected retained definition list, got {block:?}"),
        };
        document.sections[0].blocks[0] = Block::List {
            kind: ListKind::Ordered,
            start: Some(1),
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
