//! Resolves same-document references after the full section tree is known.
//!
//! libmandoc validates `.Sx` syntax but represents its target as display text.
//! This pass converts that temporary title into Mant's stable section ID and
//! downgrades invalid or ambiguous references without emitting broken links.

use std::collections::{HashMap, HashSet};

use mant_ast::{Block, Diagnostic, DiagnosticLevel, Inline, Section};
use mant_mandoc_sys::{Node, NodeKind};

type SectionTargets = HashMap<String, Option<String>>;

pub(super) fn resolve_navigation(
    sections: &mut [Section],
    explicit_targets: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut targets = SectionTargets::new();
    collect_section_targets(sections, &mut targets);
    for section in sections {
        resolve_section(section, &targets, explicit_targets, diagnostics);
    }
}

/// Return only destinations requested by `.Tg`. libmandoc also marks many
/// definitions for renderer-generated permalinks; exposing all of those as
/// inline AST nodes would add layout work and change ordinary paragraphs.
pub(super) fn explicit_targets(root: &Node) -> HashSet<String> {
    let mut nodes = Vec::new();
    flatten_nodes(root, &mut nodes);
    let mut targets = HashSet::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.macro_name.as_deref() != Some("Tg") {
            continue;
        }
        let target = first_text(node).map(str::to_owned).or_else(|| {
            // An argument-less `.Tg` names the first argument of its following
            // node. The validated target is the first tagged node after it.
            nodes[index + 1..]
                .iter()
                .find(|candidate| candidate.flags.deep_link_target)
                .and_then(|candidate| navigation_name(candidate))
        });
        if let Some(target) = target.filter(|target| !target.is_empty()) {
            targets.insert(target);
        }
    }
    targets
}

fn flatten_nodes<'a>(node: &'a Node, output: &mut Vec<&'a Node>) {
    output.push(node);
    for child in &node.children {
        flatten_nodes(child, output);
    }
}

fn first_text(node: &Node) -> Option<&str> {
    if node.kind == NodeKind::Text {
        return node.text.as_deref();
    }
    node.children.iter().find_map(first_text)
}

fn navigation_name(node: &Node) -> Option<String> {
    node.tag.clone().or_else(|| {
        first_text(node).and_then(|value| {
            value
                .trim_start_matches('-')
                .split_whitespace()
                .next()
                .map(str::to_owned)
        })
    })
}

fn collect_section_targets(sections: &[Section], targets: &mut SectionTargets) {
    for section in sections {
        targets
            .entry(section.title.clone())
            .and_modify(|target| *target = None)
            .or_insert_with(|| Some(section.id.clone()));
        collect_section_targets(&section.children, targets);
    }
}

fn resolve_section(
    section: &mut Section,
    targets: &SectionTargets,
    explicit_targets: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    resolve_blocks(&mut section.blocks, targets, explicit_targets, diagnostics);
    for child in &mut section.children {
        resolve_section(child, targets, explicit_targets, diagnostics);
    }
}

fn resolve_blocks(
    blocks: &mut [Block],
    targets: &SectionTargets,
    explicit_targets: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for block in blocks {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                resolve_inlines(children, targets, explicit_targets, diagnostics);
            }
            Block::List { items, .. } => {
                for item in items {
                    resolve_blocks(&mut item.blocks, targets, explicit_targets, diagnostics);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    for term in &mut item.terms {
                        resolve_inlines(term, targets, explicit_targets, diagnostics);
                    }
                    resolve_blocks(
                        &mut item.description,
                        targets,
                        explicit_targets,
                        diagnostics,
                    );
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        resolve_blocks(&mut cell.blocks, targets, explicit_targets, diagnostics);
                    }
                }
            }
            Block::Equation { .. } | Block::VerticalSpace { .. } | Block::Unsupported { .. } => {}
        }
    }
}

fn resolve_inlines(
    nodes: &mut Vec<Inline>,
    targets: &SectionTargets,
    explicit_targets: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut resolved = Vec::with_capacity(nodes.len());
    for node in std::mem::take(nodes) {
        match node {
            Inline::Strong { mut children } => {
                resolve_inlines(&mut children, targets, explicit_targets, diagnostics);
                resolved.push(Inline::Strong { children });
            }
            Inline::Emphasis { mut children } => {
                resolve_inlines(&mut children, targets, explicit_targets, diagnostics);
                resolved.push(Inline::Emphasis { children });
            }
            Inline::ExternalLink {
                uri,
                title,
                mut children,
            } => {
                resolve_inlines(&mut children, targets, explicit_targets, diagnostics);
                resolved.push(Inline::ExternalLink {
                    uri,
                    title,
                    children,
                });
            }
            Inline::EmailLink {
                address,
                mut children,
            } => {
                resolve_inlines(&mut children, targets, explicit_targets, diagnostics);
                resolved.push(Inline::EmailLink { address, children });
            }
            Inline::ManualReference {
                name,
                section,
                mut children,
            } => {
                resolve_inlines(&mut children, targets, explicit_targets, diagnostics);
                resolved.push(Inline::ManualReference {
                    name,
                    section,
                    children,
                });
            }
            Inline::SectionReference {
                target,
                mut children,
            } => {
                resolve_inlines(&mut children, targets, explicit_targets, diagnostics);
                if let Some(Some(section_id)) = targets.get(&target) {
                    resolved.push(Inline::SectionReference {
                        target: section_id.clone(),
                        children,
                    });
                } else {
                    diagnostics.push(Diagnostic {
                        level: DiagnosticLevel::Warning,
                        code: Some("unresolved-section-reference".to_owned()),
                        message: format!("cannot resolve section reference: {target}"),
                        source: None,
                    });
                    resolved.extend(children);
                }
            }
            Inline::Anchor { id } if explicit_targets.contains(&id) => {
                resolved.push(Inline::Anchor { id });
            }
            Inline::Anchor { .. } => {}
            leaf => resolved.push(leaf),
        }
    }
    *nodes = resolved;
}
