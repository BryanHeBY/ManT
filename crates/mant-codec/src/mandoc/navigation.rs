//! Resolves same-document references after the full section tree is known.
//!
//! libmandoc validates `.Sx` syntax but represents its target as display text.
//! This pass converts that temporary title into `ManT`'s stable section ID and
//! downgrades invalid or ambiguous references without emitting broken links.

use std::collections::{HashMap, HashSet};

use super::reference::{is_manual_reference_name, is_manual_section};
use mant_ir::{
    Block, Diagnostic, DiagnosticLevel, Inline, LinkTarget, Section,
    visit::{self, Visit, VisitMut},
};

pub(super) type SectionTargets = HashMap<String, Option<String>>;

pub(super) fn promote_manual_navigation(
    content: &super::content::LegacyContent,
    root_blocks: &mut [Block],
    sections: &mut [Section],
) {
    promote_manual_references(content, root_blocks);
    for section in sections {
        promote_manual_section(content, section);
    }
}

fn promote_manual_section(content: &super::content::LegacyContent, section: &mut Section) {
    promote_manual_reference_inlines(content, &mut section.heading.content);
    promote_manual_references(content, &mut section.blocks);
    for child in &mut section.children {
        promote_manual_section(content, child);
    }
}

pub(super) fn resolve_navigation(
    content: &mut mant_ir::ContentStore,
    root_blocks: &mut [Block],
    sections: &mut [Section],
    authored_section_targets: &SectionTargets,
    retained_targets: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Navigation names belong to the authored heading namespace.  Formatter
    // execution can change the visible heading (for example, a preceding
    // zero-width glyph can consume its first character), but that projection
    // must neither create aliases nor make a distinct authored heading
    // ambiguous.
    let targets = authored_section_targets.clone();
    resolve_blocks(
        content,
        root_blocks,
        &targets,
        retained_targets,
        diagnostics,
    );
    for section in sections {
        resolve_section(content, section, &targets, retained_targets, diagnostics);
    }
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
            if let Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } = inline
            {
                if crate::definitions::is_internal_definition_owner_marker(id.as_str()) {
                    return;
                }
                let original = id.to_string();
                let base = crate::definitions::document_id_slug(&original);
                let mut candidate = base.clone();
                let mut suffix = 2;
                while self.used.contains(&candidate)
                    || (candidate != original && self.explicit_targets.contains(&candidate))
                {
                    candidate = format!("{base}-{suffix}");
                    suffix += 1;
                }
                self.used.insert(candidate.clone());
                if candidate != original && self.explicit_targets.contains(&original) {
                    fragment_aliases.push(original.into());
                }
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

/// Collect every normalized native anchor before navigation pruning.
///
/// These zero-width nodes are part of the navigation contract even when they
/// do not also become semantic definitions. Keeping the complete set prevents
/// an automatic function or paragraph target from disappearing merely because
/// its normalized spelling collides with a section identity.
pub(super) fn native_anchor_ids(root_blocks: &[Block], sections: &[Section]) -> HashSet<String> {
    #[derive(Default)]
    struct Collector {
        ids: HashSet<String>,
    }

    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor { id, .. } = inline {
                if !crate::definitions::is_internal_definition_owner_marker(id.as_str()) {
                    self.ids.insert(id.to_string());
                }
            } else {
                visit::walk_inline(self, inline);
            }
        }
    }

    let mut collector = Collector::default();
    for block in root_blocks {
        collector.visit_block(block);
    }
    for section in sections {
        collector.visit_section(section);
    }
    collector.ids
}

fn resolve_section(
    content: &mut mant_ir::ContentStore,
    section: &mut Section,
    targets: &SectionTargets,
    retained_targets: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    resolve_inlines(
        content,
        &mut section.heading.content,
        targets,
        retained_targets,
        diagnostics,
    );
    resolve_blocks(
        content,
        &mut section.blocks,
        targets,
        retained_targets,
        diagnostics,
    );
    for child in &mut section.children {
        resolve_section(content, child, targets, retained_targets, diagnostics);
    }
}

fn promote_manual_references(content: &super::content::LegacyContent, blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                promote_manual_reference_inlines(content, children);
            }
            Block::List { items, .. } => {
                for item in items {
                    promote_manual_references(content, &mut item.blocks);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    for term in &mut item.terms {
                        promote_manual_reference_inlines(content, term);
                    }
                    promote_manual_references(content, &mut item.description);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                    promote_manual_references(content, &mut cell.blocks);
                }
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn promote_manual_reference_inlines(
    content: &super::content::LegacyContent,
    nodes: &mut Vec<Inline>,
) {
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
        let name = content.with_context(|context| mant_ir::inline_plain_text(context, children));
        let Some(Inline::Text { content: suffix }) = source.peek() else {
            promoted.push(node);
            continue;
        };
        let Some(value) = content.text(*suffix) else {
            promoted.push(node);
            continue;
        };
        let Some((section, remainder)) = manual_section_suffix(&value) else {
            promoted.push(node);
            continue;
        };
        if !is_manual_reference_name(&name) {
            promoted.push(node);
            continue;
        }

        let Some(Inline::Text {
            content: mut suffix,
        }) = source.next()
        else {
            unreachable!("peeked manual suffix remains text")
        };
        let label_suffix = format!("({section})");
        if !content.replace_text(&mut suffix, label_suffix) {
            promoted.push(node);
            promoted.push(Inline::Text { content: suffix });
            continue;
        }
        let mut children = vec![node, Inline::Text { content: suffix }];
        let target = LinkTarget::Manual {
            name: name.clone(),
            manual_section: Some(section.clone()),
        };
        let Some(occurrence) = content.attach_link(&children, target, None) else {
            let node = children.remove(0);
            let _ = content.replace_text(&mut suffix, value);
            promoted.push(node);
            promoted.push(Inline::Text { content: suffix });
            continue;
        };
        promoted.push(Inline::Link {
            occurrence,
            children,
        });
        // Alternating-font macros can split the label and suffix across
        // libmandoc nodes. The roff decoder therefore cannot consume a legacy
        // Sphinx empty destination in this one case; once the styled pair has
        // established an unambiguous manual reference, remove the same exact
        // empty suffix here.
        let remainder = remainder.strip_prefix(" <>").unwrap_or(&remainder);
        if !remainder.is_empty() {
            promoted.extend(content.lower(
                mant_ir::ContentRootKind::Body,
                None,
                vec![crate::mandoc::inline::DraftInline::Text {
                    value: remainder.to_owned(),
                }],
            ));
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
    content: &mut mant_ir::ContentStore,
    blocks: &mut [Block],
    targets: &SectionTargets,
    explicit_targets: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for block in blocks {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                resolve_inlines(content, children, targets, explicit_targets, diagnostics);
            }
            Block::List { items, .. } => {
                for item in items {
                    resolve_blocks(
                        content,
                        &mut item.blocks,
                        targets,
                        explicit_targets,
                        diagnostics,
                    );
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    for term in &mut item.terms {
                        resolve_inlines(content, term, targets, explicit_targets, diagnostics);
                    }
                    resolve_blocks(
                        content,
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
                        resolve_blocks(
                            content,
                            &mut cell.blocks,
                            targets,
                            explicit_targets,
                            diagnostics,
                        );
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
    content: &mut mant_ir::ContentStore,
    nodes: &mut Vec<Inline>,
    targets: &SectionTargets,
    explicit_targets: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut resolved = Vec::with_capacity(nodes.len());
    for node in std::mem::take(nodes) {
        match node {
            Inline::Strong { mut children } => {
                resolve_inlines(
                    content,
                    &mut children,
                    targets,
                    explicit_targets,
                    diagnostics,
                );
                resolved.push(Inline::Strong { children });
            }
            Inline::Emphasis { mut children } => {
                resolve_inlines(
                    content,
                    &mut children,
                    targets,
                    explicit_targets,
                    diagnostics,
                );
                resolved.push(Inline::Emphasis { children });
            }
            Inline::Link {
                occurrence,
                mut children,
            } if matches!(
                content.link(occurrence).map(|link| &link.target),
                Some(LinkTarget::Section { .. })
            ) =>
            {
                resolve_inlines(
                    content,
                    &mut children,
                    targets,
                    explicit_targets,
                    diagnostics,
                );
                let Some(LinkTarget::Section { id }) =
                    content.link(occurrence).map(|link| link.target.clone())
                else {
                    unreachable!("guard resolved section target")
                };
                if let Some(section_id) = resolve_section_target(targets, id.as_str()) {
                    if let Some(link) = content.link_mut(occurrence) {
                        link.target = LinkTarget::Section {
                            id: section_id.into(),
                        };
                    }
                    resolved.push(Inline::Link {
                        occurrence,
                        children,
                    });
                } else {
                    diagnostics.push(Diagnostic {
                        impact: mant_ir::DiagnosticImpact::None,
                        level: DiagnosticLevel::Warning,
                        code: Some("unresolved-section-reference".to_owned()),
                        message: format!("cannot resolve section reference: {id}"),
                        source: None,
                    });
                    let _ = content.detach_link(occurrence);
                    resolved.extend(children);
                }
            }
            Inline::Link {
                occurrence,
                mut children,
            } => {
                resolve_inlines(
                    content,
                    &mut children,
                    targets,
                    explicit_targets,
                    diagnostics,
                );
                resolved.push(Inline::Link {
                    occurrence,
                    children,
                });
            }
            Inline::Anchor {
                point,
                id,
                fragment_aliases,
            } if !fragment_aliases.is_empty() || explicit_targets.contains(id.as_str()) => {
                resolved.push(Inline::Anchor {
                    point,
                    id,
                    fragment_aliases,
                });
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
pub(super) fn resolve_section_target(targets: &SectionTargets, reference: &str) -> Option<String> {
    match targets.get(reference) {
        Some(Some(section_id)) => return Some(section_id.clone()),
        Some(None) => return None,
        None => {}
    }
    let mut candidate = None;
    for (title, section_id) in targets {
        if !is_parenthetical_section_qualification(reference, title) {
            continue;
        }
        let section_id = section_id.as_deref()?;
        if candidate.replace(section_id).is_some() {
            return None;
        }
    }
    candidate.map(ToOwned::to_owned)
}

fn is_parenthetical_section_qualification(reference: &str, title: &str) -> bool {
    title
        .strip_prefix(reference)
        .is_some_and(|suffix| suffix.starts_with('(') || suffix.starts_with(" ("))
}

use super::{LoweringContext, Node, targets};

impl LoweringContext<'_> {
    pub(super) fn reserve_section_ids(&mut self, ids: &HashSet<String>) {
        self.explicit_targets.clone_from(ids);
        self.assigned_section_ids.extend(ids.iter().cloned());
    }
    pub(super) fn section_identity_for(
        &mut self,
        title: &str,
        node: &Node,
    ) -> (String, Vec<mant_ir::FragmentAlias>) {
        let id = self.section_id(title);
        self.authored_section_targets
            .entry(title.to_owned())
            .and_modify(|target| *target = None)
            .or_insert_with(|| Some(id.clone()));
        let fragment_aliases = targets::section_target(node)
            .filter(|target| self.explicit_targets.contains(target))
            .map(|target| vec![target.into()])
            .unwrap_or_default();
        (id, fragment_aliases)
    }
    pub(super) fn section_id(&mut self, title: &str) -> String {
        let slug: String = title
            .chars()
            .flat_map(char::to_lowercase)
            .map(|character| {
                if character.is_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let base = if slug.is_empty() {
            "section".to_owned()
        } else if crate::producer_identity::is_reserved_selector(&slug) {
            format!("{slug}-section")
        } else {
            slug
        };
        let count = self.section_ids.entry(base.clone()).or_default();
        loop {
            *count += 1;
            let candidate = if *count == 1 {
                base.clone()
            } else {
                format!("{base}-{count}")
            };
            if self.assigned_section_ids.insert(candidate.clone()) {
                return candidate;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mant_ir::Inline;

    use super::{SectionTargets, promote_manual_reference_inlines, resolve_section_target};

    #[derive(Clone, Copy)]
    enum Style {
        Strong,
        Emphasis,
    }

    fn promote(style: Style, name: &str, suffix: &str) -> (mant_ir::ContentStore, Vec<Inline>) {
        let content = crate::mandoc::content::LegacyContent::default();
        let name = vec![crate::mandoc::inline::DraftInline::Text {
            value: name.to_owned(),
        }];
        let name = match style {
            Style::Strong => crate::mandoc::inline::DraftInline::Strong { children: name },
            Style::Emphasis => crate::mandoc::inline::DraftInline::Emphasis { children: name },
        };
        let mut nodes = content.lower(
            mant_ir::ContentRootKind::Body,
            None,
            vec![
                name,
                crate::mandoc::inline::DraftInline::Text {
                    value: suffix.to_owned(),
                },
            ],
        );
        promote_manual_reference_inlines(&content, &mut nodes);
        (content.finish(), nodes)
    }

    fn manual_target<'a>(
        content: mant_ir::ContentContext<'a>,
        inline: &'a Inline,
    ) -> Option<&'a mant_ir::LinkTarget> {
        content
            .link(inline)
            .ok()
            .flatten()
            .map(mant_ir::LinkView::target)
    }

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
    fn duplicated_qualified_title_keeps_the_parenthetical_fallback_ambiguous() {
        // A `None` row represents duplicate authored headings. It is still a
        // candidate and must not disappear merely because a different
        // qualified title happens to have one destination.
        let targets: SectionTargets = HashMap::from([
            ("Examples (basic)".to_owned(), None),
            (
                "Examples (advanced)".to_owned(),
                Some("examples-advanced".to_owned()),
            ),
        ]);

        assert_eq!(resolve_section_target(&targets, "Examples"), None);
    }

    #[test]
    fn promotes_traditional_see_also_pairs_without_consuming_punctuation() {
        let (store, nodes) = promote(Style::Strong, "printf", "(3), next");
        let content = store.content();

        assert!(matches!(
            manual_target(content, &nodes[0]),
            Some(mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) })
                if name == "printf" && manual_section == "3"
        ));
        assert!(
            matches!(&nodes[1], Inline::Text { content: text } if content.resolve_text(*text) == Some(", next"))
        );
    }

    #[test]
    fn promotes_manual_pairs_outside_see_also_sections() {
        let (store, nodes) = promote(Style::Strong, "git-add", "(1)");
        let content = store.content();

        assert!(matches!(
            manual_target(content, &nodes[0]),
            Some(mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) })
                if name == "git-add" && manual_section == "1"
        ));
        assert_eq!(mant_ir::inline_plain_text(content, &nodes), "git-add(1)");
    }

    #[test]
    fn promotes_groff_mr_fallback_pairs_from_emphasis() {
        let (store, nodes) = promote(Style::Emphasis, "groff_man", "(7), next");
        let content = store.content();

        assert!(matches!(
            manual_target(content, &nodes[0]),
            Some(mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) })
                if name == "groff_man" && manual_section == "7"
        ));
        assert!(
            matches!(&nodes[1], Inline::Text { content: text } if content.resolve_text(*text) == Some(", next"))
        );
    }

    #[test]
    fn removes_empty_sphinx_destination_after_styled_reference() {
        let (store, nodes) = promote(Style::Strong, "btrfs", "(5) <>, next");
        let content = store.content();

        assert!(matches!(
            manual_target(content, &nodes[0]),
            Some(mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) })
                if name == "btrfs" && manual_section == "5"
        ));
        assert!(
            matches!(&nodes[1], Inline::Text { content: text } if content.resolve_text(*text) == Some(", next"))
        );
    }

    #[test]
    fn leaves_prose_and_malformed_sections_unchanged() {
        for suffix in [" documentation", "()", "(0)", "(section one)"] {
            let (_, nodes) = promote(Style::Strong, "tool", suffix);
            assert!(matches!(nodes[0], Inline::Strong { .. }));
        }

        let (_, emphasized_prose) = promote(Style::Emphasis, "tool", " documentation");
        assert!(matches!(emphasized_prose[0], Inline::Emphasis { .. }));
    }
}
