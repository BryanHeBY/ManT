//! Publishes navigation annotations after the full section tree is known.
//!
//! libmandoc validates `.Sx` syntax but represents its target as display text.
//! This pass converts that temporary title into `ManT`'s stable section ID and
//! downgrades invalid or ambiguous references without emitting broken links.

use std::collections::{HashMap, HashSet};

use super::reference::{is_manual_reference_name, is_manual_section};
use mant_ir::{
    Block, Diagnostic, DiagnosticLevel, Inline, LinkTarget, Section,
    visit::{self, VisitMut},
};

type SectionTargets = HashMap<String, Option<String>>;

pub(super) fn publish_inline_navigation(
    nodes: &mut Vec<Inline>,
    targets: &SectionTargets,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    // Navigation names belong to the authored heading namespace.  Formatter
    // execution can change the visible heading (for example, a preceding
    // zero-width glyph can consume its first character), but that projection
    // must neither create aliases nor make a distinct authored heading
    // ambiguous.
    let changed = resolve_section_references(nodes, targets, diagnostics);
    promote_manual_reference_inlines(nodes) || changed
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

    fn reserve_section_targets(sections: &[Section], used: &mut HashSet<String>) {
        for section in sections {
            used.insert(section.id.to_string());
            used.extend(section.fragment_aliases.iter().map(ToString::to_string));
            reserve_section_targets(&section.children, used);
        }
    }

    let mut used = HashSet::new();
    reserve_section_targets(sections, &mut used);
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

fn promote_manual_reference_inlines(nodes: &mut Vec<Inline>) -> bool {
    // Most inline roots have no split reference to publish.
    if !nodes.windows(2).any(|pair| {
        matches!(pair, [Inline::Strong { .. } | Inline::Emphasis { .. }, Inline::Text { value }]
            if value.starts_with('('))
    }) {
        return false;
    }
    let mut changed = false;
    let mut promoted = Vec::with_capacity(nodes.len());
    let mut source = std::mem::take(nodes).into_iter().peekable();
    while let Some(node) = source.next() {
        // Traditional `.BR name (section)` references use bold, while
        // groff's portable `.MR` fallback uses the emphasis pair `.IR`.
        // CVS man_term.c::pre_alternate applies fonts to individual
        // operands, so keep that original subtree within the new link.
        let (Inline::Strong { children } | Inline::Emphasis { children }) = &node else {
            promoted.push(node);
            continue;
        };
        let Some(Inline::Text { value }) = source.peek() else {
            promoted.push(node);
            continue;
        };
        let Some((section, end)) = manual_section_prefix(value) else {
            promoted.push(node);
            continue;
        };
        let name = mant_ir::inline_plain_text(children);
        if !is_manual_reference_name(&name) || !can_annotate_name(children) {
            promoted.push(node);
            continue;
        }

        let manual_section = Some(section.to_owned());
        let Some(Inline::Text { mut value }) = source.next() else {
            unreachable!("the peeked section operand was text");
        };
        let remainder = value.split_off(end);
        promoted.push(Inline::Link {
            target: LinkTarget::Manual {
                name,
                manual_section,
            },
            title: None,
            children: vec![node, Inline::Text { value }],
        });
        // Publication may have consumed an independently witnessed Sphinx
        // marker. Any suffix still present here is authored visible text.
        if !remainder.is_empty() {
            promoted.push(Inline::Text { value: remainder });
        }
        changed = true;
    }
    *nodes = promoted;
    changed
}

/// Keep existing navigation and structured content out of heuristic links,
/// even when their flattened display happens to be a valid manual name.
fn can_annotate_name(nodes: &[Inline]) -> bool {
    nodes.iter().all(|node| match node {
        Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } => true,
        Inline::Strong { children } | Inline::Emphasis { children } => can_annotate_name(children),
        Inline::Link { .. } | Inline::Equation { .. } | Inline::LineBreak { .. } => false,
    })
}

fn manual_section_prefix(value: &str) -> Option<(&str, usize)> {
    let closing = value.find(')')?;
    let section = value.get(..closing)?.strip_prefix('(')?;
    is_manual_section(section).then_some((section, closing + 1))
}

fn resolve_section_references(
    nodes: &mut Vec<Inline>,
    targets: &SectionTargets,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let mut changed = false;
    let mut unwrap = false;
    for node in nodes.iter_mut() {
        match node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link {
                target:
                    LinkTarget::External { .. }
                    | LinkTarget::Email { .. }
                    | LinkTarget::Manual { .. }
                    | LinkTarget::Document { .. },
                children,
                ..
            } => {
                changed |= resolve_section_references(children, targets, diagnostics);
            }
            Inline::Link {
                target: LinkTarget::Section { .. },
                ..
            } => {
                unwrap = true;
            }
            _ => {}
        }
    }
    if !unwrap {
        return changed;
    }

    let mut resolved = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        if let Inline::Link {
            target: LinkTarget::Section { id },
            children,
            ..
        } = &mut node
        {
            changed |= resolve_section_references(children, targets, diagnostics);
            if let Some(section_id) = resolve_section_target(targets, id.as_str()) {
                if id.as_str() != section_id {
                    *id = section_id.into();
                    changed = true;
                }
            } else {
                diagnostics.push(Diagnostic {
                    impact: mant_ir::DiagnosticImpact::None,
                    level: DiagnosticLevel::Warning,
                    code: Some("unresolved-section-reference".to_owned()),
                    message: format!("cannot resolve section reference: {id}"),
                    source: None,
                });
                resolved.append(children);
                changed = true;
                continue;
            }
        }
        resolved.push(node);
    }
    *nodes = resolved;
    changed
}

/// Resolve an `.Sx` title without guessing across arbitrary headings.
///
/// Most mdoc sources name a heading exactly.  Some established manual pages
/// use the stable leading title while their target adds a parenthetical
/// qualifier, for example `White Space Splitting` for `White Space Splitting
/// (Field Splitting)`.  Accept that form only when it identifies one target;
/// every other prefix remains unresolved rather than becoming a surprising
/// navigation jump.
fn resolve_section_target<'a>(targets: &'a SectionTargets, reference: &str) -> Option<&'a str> {
    if let Some(exact) = targets.get(reference) {
        return exact.as_deref();
    }
    let mut candidates = targets
        .iter()
        .filter(|(title, _)| is_parenthetical_section_qualification(reference, title));
    let (_, candidate) = candidates.next()?;
    // An ambiguous authored title still occupies a candidate slot.
    // Discarding it here would promote another title to a false unique match.
    if candidates.next().is_some() {
        return None;
    }
    candidate.as_deref()
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
        self.section_ids.reserve(ids);
    }
    pub(super) fn section_identity_for(
        &mut self,
        title: &str,
        node: &Node,
    ) -> (String, Vec<mant_ir::FragmentAlias>) {
        let (id, long_fragment) = self.section_id(title);
        self.authored_section_targets
            .entry(title.to_owned())
            .and_modify(|target| *target = None)
            .or_insert_with(|| Some(id.clone()));
        let fragment_aliases = targets::section_target(node)
            .filter(|target| self.explicit_targets.contains(target))
            .map(mant_ir::FragmentAlias::from)
            .into_iter()
            .chain(long_fragment)
            .collect();
        (id, fragment_aliases)
    }
    pub(super) fn section_id(&mut self, title: &str) -> (String, Option<mant_ir::FragmentAlias>) {
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
        self.section_ids.allocate(&base)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mant_ir::{Inline, LinkTarget};

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
            Some("white-space-splitting-field-splitting-36")
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
                    && mant_ir::inline_plain_text(children) == "git-add(1)"
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
    fn preserves_literal_suffix_after_styled_reference() {
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
        assert!(matches!(&nodes[1], Inline::Text { value } if value == " <>, next"));
    }

    #[test]
    fn wraps_only_the_original_subtree_that_can_take_a_new_link() {
        let original = Inline::Strong {
            children: vec![
                Inline::anchor("source-head"),
                Inline::Emphasis {
                    children: vec![Inline::Text {
                        value: "tool".into(),
                    }],
                },
            ],
        };
        let existing_link = Inline::Strong {
            children: vec![Inline::Link {
                target: LinkTarget::External {
                    uri: "https://example.test".into(),
                },
                title: None,
                children: vec![Inline::Text {
                    value: "tool".into(),
                }],
            }],
        };
        let suffix = Inline::Text {
            value: "(3)".into(),
        };
        let mut nodes = vec![
            original.clone(),
            suffix.clone(),
            existing_link.clone(),
            suffix.clone(),
        ];
        assert!(promote_manual_reference_inlines(&mut nodes));
        let [Inline::Link { children, .. }, unmodified, trailing] = nodes.as_slice() else {
            panic!("one manual annotation and the original unrelated link")
        };
        assert_eq!(children, &[original, suffix.clone()]);
        assert_eq!(unmodified, &existing_link);
        assert_eq!(trailing, &suffix);
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
