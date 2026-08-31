//! Resolves same-document references after the full section tree is known.
//!
//! libmandoc validates `.Sx` syntax but represents its target as display text.
//! This pass converts that temporary title into `ManT`'s stable section ID and
//! downgrades invalid or ambiguous references without emitting broken links.

use std::collections::{HashMap, HashSet};

use super::reference::{is_manual_reference_name, is_manual_section};
use super::roff_escape::visible_text;
use libmandoc_rs::{Node, NodeKind};
use mant_ir::{
    Block, Diagnostic, DiagnosticLevel, Inline, LinkTarget, Section,
    visit::{self, VisitMut},
};

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
/// inline IR nodes would add layout work and change ordinary paragraphs.
pub(super) fn explicit_targets(root: &Node) -> HashSet<String> {
    let mut nodes = Vec::new();
    flatten_nodes(root, &mut nodes);
    let mut targets = HashSet::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.macro_name.as_deref() != Some("Tg") {
            continue;
        }
        let target = first_text(node).map(visible_text).or_else(|| {
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

/// Normalize and uniquely allocate formatter-generated native anchors.
///
/// Explicit `.Tg` identities remain source-authored destinations. Other man
/// and mdoc tags are formatter conveniences, so expose them through the same
/// document-local slug contract as semantic entries and disambiguate repeated
/// tags before IR validation observes them.
pub(super) fn normalize_generated_anchors(
    blocks: &mut [Block],
    sections: &mut [Section],
    explicit_targets: &HashSet<String>,
) {
    struct Normalizer<'targets> {
        explicit_targets: &'targets HashSet<String>,
        used: HashSet<String>,
    }

    impl VisitMut for Normalizer<'_> {
        fn visit_inline_mut(&mut self, inline: &mut Inline) {
            if let Inline::Anchor { id } = inline {
                let original = id.to_string();
                if self.explicit_targets.contains(&original) && self.used.insert(original.clone()) {
                    return;
                }
                let base = crate::definitions::document_id_slug(&original);
                let mut candidate = base.clone();
                let mut suffix = 2;
                while self.used.contains(&candidate) || self.explicit_targets.contains(&candidate) {
                    candidate = format!("{base}-{suffix}");
                    suffix += 1;
                }
                self.used.insert(candidate.clone());
                *id = candidate.into();
                return;
            }
            visit::walk_inline_mut(self, inline);
        }
    }

    fn reserve_section_ids(sections: &[Section], used: &mut HashSet<String>) {
        for section in sections {
            used.insert(section.id.to_string());
            reserve_section_ids(&section.children, used);
        }
    }

    let mut used = HashSet::new();
    reserve_section_ids(sections, &mut used);
    let mut normalizer = Normalizer {
        explicit_targets,
        used,
    };
    for block in blocks {
        normalizer.visit_block_mut(block);
    }
    for section in sections {
        normalizer.visit_section_mut(section);
    }
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
    node.tag.as_deref().map(visible_text).or_else(|| {
        first_text(node).and_then(|value| {
            let sanitized = visible_text(value);
            sanitized
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
            .or_insert_with(|| Some(section.id.to_string()));
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
    promote_manual_references(&mut section.blocks);
    for child in &mut section.children {
        resolve_section(child, targets, explicit_targets, diagnostics);
    }
}

fn promote_manual_references(blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                promote_manual_reference_inlines(children);
            }
            Block::List { items, .. } => {
                for item in items {
                    promote_manual_references(&mut item.blocks);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    for term in &mut item.terms {
                        promote_manual_reference_inlines(term);
                    }
                    promote_manual_references(&mut item.description);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                    promote_manual_references(&mut cell.blocks);
                }
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn promote_manual_reference_inlines(nodes: &mut Vec<Inline>) {
    let mut promoted = Vec::with_capacity(nodes.len());
    let mut source = std::mem::take(nodes).into_iter().peekable();
    while let Some(node) = source.next() {
        // Traditional `.BR name (section)` references use bold, while
        // groff's portable `.MR` fallback expands to `.IR` and therefore
        // reaches us as emphasis followed by the parenthesized section.
        let (Inline::Strong { children } | Inline::Emphasis { children }) = &node else {
            promoted.push(node);
            continue;
        };
        let name = crate::inline::plain_text(children);
        let Some(Inline::Text { value }) = source.peek() else {
            promoted.push(node);
            continue;
        };
        let Some((section, remainder)) = manual_section_suffix(value) else {
            promoted.push(node);
            continue;
        };
        if !is_manual_reference_name(&name) {
            promoted.push(node);
            continue;
        }

        source.next();
        promoted.push(Inline::Link {
            target: LinkTarget::Manual {
                name: name.clone(),
                manual_section: Some(section.clone()),
            },
            title: None,
            children: vec![Inline::Text {
                value: format!("{name}({section})"),
            }],
        });
        // Alternating-font macros can split the label and suffix across
        // libmandoc nodes. The roff decoder therefore cannot consume a legacy
        // Sphinx empty destination in this one case; once the styled pair has
        // established an unambiguous manual reference, remove the same exact
        // empty suffix here.
        let remainder = remainder.strip_prefix(" <>").unwrap_or(&remainder);
        if !remainder.is_empty() {
            promoted.push(Inline::Text {
                value: remainder.to_owned(),
            });
        }
    }
    *nodes = promoted;
}

fn manual_section_suffix(value: &str) -> Option<(String, String)> {
    let value = value.strip_prefix('(')?;
    let closing = value.find(')')?;
    let section = &value[..closing];
    if !is_manual_section(section) {
        return None;
    }
    Some((section.to_owned(), value[closing + 1..].to_owned()))
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
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
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
            Inline::Link {
                target: LinkTarget::Section { id },
                title,
                mut children,
            } => {
                resolve_inlines(&mut children, targets, explicit_targets, diagnostics);
                if let Some(section_id) = resolve_section_target(targets, id.as_str()) {
                    resolved.push(Inline::Link {
                        target: LinkTarget::Section {
                            id: section_id.into(),
                        },
                        title,
                        children,
                    });
                } else {
                    diagnostics.push(Diagnostic {
                        level: DiagnosticLevel::Warning,
                        code: Some("unresolved-section-reference".to_owned()),
                        message: format!("cannot resolve section reference: {id}"),
                        source: None,
                    });
                    resolved.extend(children);
                }
            }
            Inline::Link {
                target,
                title,
                mut children,
            } => {
                resolve_inlines(&mut children, targets, explicit_targets, diagnostics);
                resolved.push(Inline::Link {
                    target,
                    title,
                    children,
                });
            }
            Inline::Anchor { id } if explicit_targets.contains(id.as_str()) => {
                resolved.push(Inline::Anchor { id });
            }
            Inline::Anchor { .. } => {}
            leaf => resolved.push(leaf),
        }
    }
    *nodes = resolved;
}

/// Resolve an `.Sx` title without guessing across arbitrary headings.
///
/// Most mdoc sources name a heading exactly.  Some established manual pages
/// use the stable leading title while their target adds a parenthetical
/// qualifier, for example `White Space Splitting` for `White Space Splitting
/// (Field Splitting)`.  Accept that form only when it identifies one target;
/// every other prefix remains unresolved rather than becoming a surprising
/// navigation jump.
fn resolve_section_target(targets: &SectionTargets, reference: &str) -> Option<String> {
    match targets.get(reference) {
        Some(Some(section_id)) => return Some(section_id.clone()),
        Some(None) => return None,
        None => {}
    }
    let mut candidates = targets.iter().filter_map(|(title, section_id)| {
        is_parenthetical_section_qualification(reference, title)
            .then_some(section_id.as_deref())
            .flatten()
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then(|| candidate.to_owned())
}

fn is_parenthetical_section_qualification(reference: &str, title: &str) -> bool {
    title
        .strip_prefix(reference)
        .is_some_and(|suffix| suffix.starts_with('(') || suffix.starts_with(" ("))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mant_ir::Inline;

    use super::{SectionTargets, promote_manual_reference_inlines, resolve_section_target};

    #[test]
    fn resolves_one_parenthetically_qualified_section_title() {
        let targets: SectionTargets = HashMap::from([
            (
                "White Space Splitting (Field Splitting)".to_owned(),
                Some("white-space-splitting-field-splitting-36".to_owned()),
            ),
            ("Other".to_owned(), Some("other-2".to_owned())),
        ]);

        assert_eq!(
            resolve_section_target(&targets, "White Space Splitting"),
            Some("white-space-splitting-field-splitting-36".to_owned())
        );
    }

    #[test]
    fn rejects_ambiguous_parenthetically_qualified_section_titles() {
        let targets: SectionTargets = HashMap::from([
            (
                "Examples (basic)".to_owned(),
                Some("examples-basic-2".to_owned()),
            ),
            (
                "Examples (advanced)".to_owned(),
                Some("examples-advanced-3".to_owned()),
            ),
        ]);

        assert_eq!(resolve_section_target(&targets, "Examples"), None);
    }

    #[test]
    fn promotes_traditional_see_also_pairs_without_consuming_punctuation() {
        let mut nodes = vec![
            Inline::Strong {
                children: vec![Inline::Text {
                    value: "printf".to_owned(),
                }],
            },
            Inline::Text {
                value: "(3), next".to_owned(),
            },
        ];

        promote_manual_reference_inlines(&mut nodes);

        assert!(matches!(
            &nodes[0],
            Inline::Link { target: mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) }, .. }
                if name == "printf" && manual_section == "3"
        ));
        assert!(matches!(&nodes[1], Inline::Text { value } if value == ", next"));
    }

    #[test]
    fn promotes_manual_pairs_outside_see_also_sections() {
        let mut nodes = vec![
            Inline::Strong {
                children: vec![Inline::Text {
                    value: "git-add".to_owned(),
                }],
            },
            Inline::Text {
                value: "(1)".to_owned(),
            },
        ];

        promote_manual_reference_inlines(&mut nodes);

        assert!(matches!(
            &nodes[0],
            Inline::Link { target: mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) }, children, .. }
                if name == "git-add"
                    && manual_section == "1"
                    && crate::inline::plain_text(children) == "git-add(1)"
        ));
    }

    #[test]
    fn promotes_groff_mr_fallback_pairs_from_emphasis() {
        let mut nodes = vec![
            Inline::Emphasis {
                children: vec![Inline::Text {
                    value: "groff_man".to_owned(),
                }],
            },
            Inline::Text {
                value: "(7), next".to_owned(),
            },
        ];

        promote_manual_reference_inlines(&mut nodes);

        assert!(matches!(
            &nodes[0],
            Inline::Link { target: mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) }, .. }
                if name == "groff_man" && manual_section == "7"
        ));
        assert!(matches!(&nodes[1], Inline::Text { value } if value == ", next"));
    }

    #[test]
    fn removes_empty_sphinx_destination_after_styled_reference() {
        let mut nodes = vec![
            Inline::Strong {
                children: vec![Inline::Text {
                    value: "btrfs".to_owned(),
                }],
            },
            Inline::Text {
                value: "(5) <>, next".to_owned(),
            },
        ];

        promote_manual_reference_inlines(&mut nodes);

        assert!(matches!(
            &nodes[0],
            Inline::Link { target: mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) }, .. }
                if name == "btrfs" && manual_section == "5"
        ));
        assert!(matches!(&nodes[1], Inline::Text { value } if value == ", next"));
    }

    #[test]
    fn leaves_prose_and_malformed_sections_unchanged() {
        for suffix in [" documentation", "()", "(0)", "(section one)"] {
            let mut nodes = vec![
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "tool".to_owned(),
                    }],
                },
                Inline::Text {
                    value: suffix.to_owned(),
                },
            ];
            promote_manual_reference_inlines(&mut nodes);
            assert!(matches!(nodes[0], Inline::Strong { .. }));
        }

        let mut emphasized_prose = vec![
            Inline::Emphasis {
                children: vec![Inline::Text {
                    value: "tool".to_owned(),
                }],
            },
            Inline::Text {
                value: " documentation".to_owned(),
            },
        ];
        promote_manual_reference_inlines(&mut emphasized_prose);
        assert!(matches!(emphasized_prose[0], Inline::Emphasis { .. }));
    }
}
