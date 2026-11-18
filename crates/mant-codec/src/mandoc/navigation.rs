//! Publishes navigation annotations after the full section tree is known.
//!
//! Resolve authored section destinations and recognize conventional manual
//! references as typed links before publishing the document.

use super::{
    reference::{is_manual_reference_name, is_manual_section},
    section_identity::RoffSectionTargets,
};
use mant_ir::{Diagnostic, DiagnosticLevel, Inline, LinkTarget};

mod anchors;
pub(super) use anchors::normalize_generated_anchors;

pub(super) fn publish_inline_navigation(
    nodes: &mut Vec<Inline>,
    targets: &RoffSectionTargets,
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
    targets: &RoffSectionTargets,
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
            if let Some(section_id) = targets.resolve(id.as_str()) {
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

#[cfg(test)]
mod tests {
    use mant_ir::{Inline, LinkTarget};

    use super::promote_manual_reference_inlines;

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
