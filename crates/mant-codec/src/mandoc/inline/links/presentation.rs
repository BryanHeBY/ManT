//! Private Lk ownership over final accepted native output, never execution.

use std::collections::{HashMap, HashSet};

mod description_runs;

use super::{Inline, InlineBuilder};
use crate::mandoc::inline::ownership::annotations::AnnotationKey;
use description_runs::{AcceptedDescription, PhysicalWriter};
use mant_ir::LinkTarget;

const OWNER_PREFIX: &str = "\0mant:lk-presentation:";

#[cfg(test)]
std::thread_local! {
    static PRESENTATION_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn note_visit() {
    crate::mandoc::inline::ownership::metrics::note(|counts| counts.normalization += 1);
    PRESENTATION_VISITS.with(|visits| visits.set(visits.get().saturating_add(1)));
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum Part {
    Description,
    CapturedDescription,
    Colon,
    Suffix,
    Uri,
}

impl Part {
    const fn spelling(self) -> &'static str {
        match self {
            Self::Description => "d",
            Self::CapturedDescription => "d+",
            Self::Colon => "c",
            Self::Suffix => "s",
            Self::Uri => "u",
        }
    }
}

pub(super) fn scope_marker(owner: AnnotationKey, part: Part) -> String {
    format!("\0mant:output-scope:lk:{owner}:{}", part.spelling())
}

fn placeholder(owner: AnnotationKey, part: Part, address: &str, children: Vec<Inline>) -> Inline {
    // This NUL-prefixed target is an internal, invalid public-IR value. It
    // never passes URI admission or executes as an href: finalization below
    // removes it after native acceptance and before semantic recognition.
    Inline::Link {
        target: LinkTarget::External {
            uri: format!("{OWNER_PREFIX}{owner}:{}:{address}", part.spelling()),
        },
        // Authored Lk targets cannot supply this matching private title.
        // The pair distinguishes ownership metadata from an invalid authored
        // address which merely resembles the NUL-prefixed target namespace.
        title: Some(scope_marker(owner, part)),
        children,
    }
}

/// Execute once in the caller's state, then bind its new output by a stable
/// local marker. `encode1()` may release the preceding operand's delayed glyph:
/// the zero-advance receipt retains that operand's owner and original style.
pub(super) fn append_owned_part(
    builder: &mut InlineBuilder,
    owner: AnnotationKey,
    part: Part,
    address: &str,
    preceding: Option<Part>,
    append: impl FnOnce(&mut InlineBuilder),
) {
    let (_, membership) = builder.append_semantic_scope(owner, true, append, |children| {
        vec![placeholder(owner, part, address, children)]
    });
    let Inline::Link { target, title, .. } = placeholder(owner, part, address, Vec::new()) else {
        unreachable!()
    };
    builder.zero_advance.bind_deferred(
        membership,
        &crate::mandoc::inline::ownership::Annotation::Link { target, title },
        false,
    );
    let _ = preceding;
}

/// A label glyph released by the colon remains a description. Keep that
/// prefix outside the suffix owner; the colon and URI still execute in source
/// order and their accepted native children remain the sole public text.
pub(super) fn wrap_suffix(
    owner: AnnotationKey,
    address: &str,
    children: Vec<Inline>,
) -> Vec<Inline> {
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    for node in children {
        if suffix.is_empty()
            && (matches!(node, Inline::Anchor { .. } | Inline::LineBreak { .. })
                || metadata(&node).is_some_and(|(_, part, _, _)| part == Part::Description))
        {
            prefix.push(node);
        } else {
            suffix.push(node);
        }
    }
    prefix.push(placeholder(owner, Part::Suffix, address, suffix));
    prefix
}

fn metadata(node: &Inline) -> Option<(AnnotationKey, Part, &str, bool)> {
    let Inline::Link {
        target: mant_ir::LinkTarget::External { uri },
        title: Some(title),
        ..
    } = node
    else {
        return None;
    };
    let mut fields = uri.strip_prefix(OWNER_PREFIX)?.splitn(3, ':');
    let owner = fields.next()?.parse().ok()?;
    let part = match fields.next()? {
        "d" => Part::Description,
        "d+" => Part::CapturedDescription,
        "c" => Part::Colon,
        "s" => Part::Suffix,
        "u" => Part::Uri,
        _ => return None,
    };
    let capture = part == Part::CapturedDescription;
    let normal_part = if capture { Part::Description } else { part };
    (title == &scope_marker(owner, part)).then_some((owner, normal_part, fields.next()?, capture))
}

/// The URI owner carries the same authored destination fact as a typed Link,
/// even when `term_fill()` rejects every native glyph. Preserve that receipt
/// while trimming rejected text; `mdoc_lk_pre()` still emits its authored href.
pub(in crate::mandoc::inline) fn retains_authored_identity(node: &Inline) -> bool {
    metadata(node).is_some_and(|(_, part, address, _)| part == Part::Uri && !address.is_empty())
}

/// `wrap_output_from()` can split a semantic owner at a real native commit.
/// Retain the existing authored-link seam only for the two potential anchors;
/// a colon/suffix is presentation rather than another link occurrence.
pub(in crate::mandoc::inline) fn same_link_owner(left: &Inline, right: &Inline) -> bool {
    matches!((metadata(left), metadata(right)),
        (Some((left_owner, left_part, left_address, left_capture)), Some((right_owner, right_part, right_address, right_capture)))
            if left_owner == right_owner
                && left_part == right_part
                && matches!(left_part, Part::Description | Part::Uri)
                && left_address == right_address
                && left_capture == right_capture)
}

pub(in crate::mandoc::inline) fn is_private_owner(node: &Inline) -> bool {
    metadata(node).is_some()
}

/// Link activation needs an actual readable cell, rather than merely a native
/// graph scalar. Zero-width scalars remain accepted content; they cannot by
/// themselves provide a readable label instead of the authored address.
fn text_is_readable(value: &str) -> bool {
    let mut scalar = [0; 4];
    value.chars().any(|character| {
        !character.is_whitespace()
            && mant_ir::geometry::text_width(character.encode_utf8(&mut scalar)) > 0
    })
}

fn has_readable_text(nodes: &[Inline]) -> bool {
    nodes.iter().any(|node| {
        #[cfg(test)]
        note_visit();
        match node {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                text_is_readable(value)
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => has_readable_text(children),
            Inline::Anchor { .. } | Inline::LineBreak { .. } => false,
        }
    })
}

#[derive(Default)]
struct AcceptedLabel {
    readable: bool,
    uri_readable: bool,
    description: AcceptedDescription,
}

/// Final native field receipts already chose these children. One collection
/// and one rewrite visit the local drained output, not one scan per Lk, and
/// every private placeholder disappears before public IR can be returned.
pub(in crate::mandoc::inline) fn finalize_accepted_links(nodes: &mut Vec<Inline>) {
    let mut labels = HashMap::new();
    collect_labels(nodes, &mut labels, None, PhysicalWriter::Authored);
    if !labels.is_empty() {
        #[cfg(test)]
        description_runs::observe(&labels);
        resolve_parts(nodes, &labels, &mut HashSet::new());
    }
}

fn collect_labels(
    nodes: &[Inline],
    labels: &mut HashMap<AnnotationKey, AcceptedLabel>,
    owned: Option<(AnnotationKey, Part, bool)>,
    writer: PhysicalWriter,
) {
    for node in nodes {
        #[cfg(test)]
        note_visit();
        let (owned, writer) = if let Some((owner, part, _, capture)) = metadata(node) {
            let label = labels.entry(owner).or_default();
            if capture {
                label.description.enter();
            }
            (Some((owner, part, capture)), PhysicalWriter::Authored)
        } else if let Some(epoch) = crate::mandoc::inline::ownership::described_foreign_writer(node)
        {
            (owned, PhysicalWriter::Earlier(epoch))
        } else {
            (owned, writer)
        };
        match node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => collect_labels(children, labels, owned, writer),
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                if let Some((owner, part, capture)) = owned {
                    let label = labels.entry(owner).or_default();
                    match part {
                        Part::Description => {
                            label.readable |= text_is_readable(value);
                            if capture {
                                label.description.record_text(writer, value);
                            }
                        }
                        Part::Uri => label.uri_readable |= text_is_readable(value),
                        Part::Colon | Part::Suffix | Part::CapturedDescription => {}
                    }
                }
            }
            Inline::LineBreak { .. } => {
                if let Some((owner, Part::Description, true)) = owned {
                    labels
                        .entry(owner)
                        .or_default()
                        .description
                        .record(writer, 1);
                }
            }
            Inline::Anchor { .. } => {}
        }
    }
}

fn resolve_parts(
    nodes: &mut Vec<Inline>,
    labels: &HashMap<AnnotationKey, AcceptedLabel>,
    bound: &mut HashSet<(AnnotationKey, Part)>,
) {
    let mut output = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        #[cfg(test)]
        note_visit();
        let part = metadata(&node)
            .map(|(owner, part, address, capture)| (owner, part, address.to_owned(), capture));
        match &mut node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => resolve_parts(children, labels, bound),
            _ => {}
        }
        if crate::mandoc::inline::ownership::described_foreign_writer(&node).is_some() {
            let Inline::Link { children, .. } = node else {
                unreachable!()
            };
            // Reconstruct the unchanged legacy subtree. The current wire
            // cannot put separate runs under the same authored occurrence.
            output.extend(children);
        } else if let Some((owner, part, address, capture)) = part {
            let Inline::Link { children, .. } = node else {
                unreachable!();
            };
            let label = labels.get(&owner).is_some_and(|label| label.readable);
            match part {
                Part::CapturedDescription => {
                    unreachable!("metadata normalizes captured descriptions")
                }
                Part::Description if label && !address.is_empty() => {
                    if super::super::plain_text(&children).is_empty()
                        && labels.get(&owner).is_some_and(|label| match part {
                            Part::Description => label.readable,
                            Part::Uri => label.uri_readable,
                            _ => false,
                        })
                    {
                        output.extend(children);
                    } else {
                        let title = labels.get(&owner).filter(|_| capture).and_then(|label| {
                            crate::mandoc::link_layout::marker(
                                owner,
                                &children,
                                &label.description.authored_members(),
                            )
                        });
                        bind_accepted_link(
                            &mut output,
                            children,
                            (address, title),
                            owner,
                            part,
                            bound,
                        );
                    }
                }
                Part::Uri if !label && !address.is_empty() => {
                    if super::super::plain_text(&children).is_empty()
                        && labels.get(&owner).is_some_and(|label| match part {
                            Part::Description => label.readable,
                            Part::Uri => label.uri_readable,
                            _ => false,
                        })
                    {
                        output.extend(children);
                    } else {
                        bind_accepted_link(
                            &mut output,
                            children,
                            (address, None),
                            owner,
                            part,
                            bound,
                        );
                    }
                }
                _ => output.extend(children),
            }
        } else {
            output.push(node);
        }
    }
    *nodes = output;
}

fn bind_accepted_link(
    output: &mut Vec<Inline>,
    children: Vec<Inline>,
    (address, title): (String, Option<String>),
    owner: AnnotationKey,
    part: Part,
    bound: &mut HashSet<(AnnotationKey, Part)>,
) {
    if has_readable_text(&children) {
        bound.insert((owner, part));
        output.push(Inline::Link {
            target: LinkTarget::External { uri: address },
            title,
            children,
        });
    } else {
        // Rejection removes the clickable range, not the authored identity.
        // Resolve it once for this source owner, as an empty typed Link; no
        // rejected glyph, physical row, or alternate label is reconstructed.
        if bound.insert((owner, part)) {
            output.push(Inline::Link {
                target: LinkTarget::External { uri: address },
                title: None,
                children,
            });
        } else {
            output.extend(children);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Inline {
        Inline::Text {
            value: value.to_owned(),
        }
    }

    #[test]
    fn accepted_annotation_visits_each_local_owner_a_bounded_number_of_times() {
        // IR-only complexity contract: public styles are transparent to the
        // already accepted children. This does not claim a terminal gold for
        // arbitrarily deep roff or increase the producer's nesting budget.
        for owners in [1_u32, 32, 256] {
            for depth in [0, 16, 64] {
                let mut nodes = Vec::new();
                for owner in 0..owners {
                    let mut description = vec![text("label")];
                    for _ in 0..depth {
                        description = vec![Inline::Emphasis {
                            children: description,
                        }];
                    }
                    nodes.push(placeholder(
                        owner.into(),
                        Part::Description,
                        "https://example.com",
                        description,
                    ));
                    nodes.push(placeholder(
                        owner.into(),
                        Part::Suffix,
                        "https://example.com",
                        vec![
                            text(": "),
                            placeholder(
                                owner.into(),
                                Part::Uri,
                                "https://example.com",
                                vec![text("https://example.com")],
                            ),
                        ],
                    ));
                }
                PRESENTATION_VISITS.with(|visits| visits.set(0));
                finalize_accepted_links(&mut nodes);
                let work = PRESENTATION_VISITS.with(std::cell::Cell::get);
                let original_nodes = usize::try_from(owners).unwrap() * (depth + 6);
                assert!(
                    work <= original_nodes * 4,
                    "{owners} owners, depth {depth}: {work}"
                );
                assert_eq!(
                    nodes
                        .iter()
                        .filter(|node| matches!(node, Inline::Link { .. }))
                        .count(),
                    usize::try_from(owners).unwrap()
                );
                assert!(!format!("{nodes:?}").contains("mant:lk-presentation"));
            }
        }
    }

    #[test]
    fn private_ownership_requires_the_matching_non_authored_scope_title() {
        let mut node = placeholder(
            17_u32.into(),
            Part::Uri,
            "https://example.com",
            vec![text("URI")],
        );
        assert!(metadata(&node).is_some());
        let Inline::Link { title, .. } = &mut node else {
            panic!("private ownership container");
        };
        *title = None;
        assert!(
            metadata(&node).is_none(),
            "a target alone cannot forge an owner"
        );
        let Inline::Link { title, .. } = &mut node else {
            unreachable!();
        };
        *title = Some(scope_marker(18_u32.into(), Part::Uri));
        assert!(
            metadata(&node).is_none(),
            "another owner cannot lend its proof"
        );
        // This is an intentionally invalid public-IR target for a private
        // metadata unit check, not an admitted public value or a roff oracle.
        // Public validators reject its NUL independently of this detection.
    }

    use mant_ir::visit::{self, Visit};

    #[derive(Default)]
    struct Links(Vec<String>);
    impl<'a> Visit<'a> for Links {
        fn visit_inline(&mut self, inline: &'a Inline) {
            if let Inline::Link {
                target: LinkTarget::External { uri },
                children,
                ..
            } = inline
                && uri == "https://z.test"
            {
                self.0.push(mant_ir::inline_plain_text(children));
            }
            visit::walk_inline(self, inline);
        }
    }

    #[test]
    fn described_link_retains_physical_runs_while_legacy_projection_stays_unchanged() {
        use std::path::Path;

        // Frozen AT split.1. CVS termp_lk_pre executes Z, U, ':' and URI
        // at the live cursor. Pinned UTF-8/ASCII agree that Y survives in
        // between, while mdoc_lk_pre's HTML authored description is Z U.
        const HEADER: &str = ".Dd November 27, 2026\n.Dt GLYPHS 1\n.Os\n.Sh NAME\n.Nm glyphs\n.Nd small roff inputs\n.Sh DESCRIPTION\n";
        let split = ".No HEAD\nY\n.No \"\\h'|1n'R\"\n.Lk https://z.test Z U\nEND\n";
        let source = format!("{HEADER}{split}.Lk https://z.test V\n.Sh ENDTEST\nDone.\n");
        let (document, trace) = description_runs::capture(|| {
            crate::mandoc::parse_plain_manual_with_link_layout(
                Path::new("split.1"),
                source.as_bytes(),
            )
            .unwrap()
            .into_document()
        });
        let mut links = Links::default();
        links.visit_document(&document);
        assert_eq!(links.0.len(), 2);
        let compact =
            |value: &str| -> String { value.chars().filter(|ch| !ch.is_whitespace()).collect() };
        assert_eq!(compact(&links.0[0]), "ZYU", "legacy safety boundary");
        assert_eq!(compact(&links.0[1]), "V");
        let body = document
            .sections
            .iter()
            .find(|section| section.id.as_str() == "description")
            .unwrap();
        let text = body
            .blocks
            .iter()
            .filter_map(|block| match block {
                mant_ir::Block::Paragraph { children, .. } => {
                    Some(mant_ir::inline_plain_text(children))
                }
                _ => None,
            })
            .collect::<String>();
        assert!(
            compact(&text).starts_with("HRAZYU:https://z.testEND"),
            "{text}"
        );

        eprintln!("{trace:?}");
        assert_eq!(trace.len(), 2, "{trace:?}");
        assert_ne!(trace[0].0, trace[1].0, "two authored IDs, same URI");
        let (_, runs) = trace
            .iter()
            .find(|(_, runs)| {
                runs.iter()
                    .any(|run| matches!(run.writer, PhysicalWriter::Earlier(_)))
            })
            .expect("physical old-writer evidence reaches publication");
        let accepted = links.0[0].chars().collect::<Vec<_>>();
        let mut leaves = Vec::new();
        for run in runs {
            assert_eq!(run.fragment, 1);
            let ink = accepted[run.scalars.clone()]
                .iter()
                .copied()
                .collect::<String>();
            let ink = compact(&ink);
            if !ink.is_empty() {
                leaves.push((run.writer, ink));
            }
        }
        assert_eq!(leaves.len(), 3, "{leaves:?}");
        assert_eq!(leaves[0], (PhysicalWriter::Authored, "Z".into()));
        assert!(matches!(leaves[1].0, PhysicalWriter::Earlier(_)));
        assert_eq!(leaves[1].1, "Y");
        assert_eq!(leaves[2], (PhysicalWriter::Authored, "U".into()));
        let (legacy, legacy_trace) = description_runs::capture(|| {
            crate::mandoc::parse_plain_manual(Path::new("split.1"), source.as_bytes()).unwrap()
        });
        assert!(legacy_trace.is_empty(), "legacy parse never captures runs");
        let json = serde_json::to_string(&legacy).unwrap();
        assert_eq!(serde_json::to_string(&document).unwrap(), json);
        assert!(!json.contains("lk-described-foreign"), "{json}");
        assert_public_layout(&source, &json);
    }

    fn assert_public_layout(source: &str, json: &str) {
        use std::path::Path;
        let decoded = crate::mandoc::parse_plain_manual_with_link_layout(
            Path::new("split.1"),
            source.as_bytes(),
        )
        .unwrap();
        let layout = decoded.into_resolved("split".to_owned(), None, None);
        let paired = layout.content().document.as_ref().unwrap();
        assert_eq!(
            serde_json::to_string(&paired).unwrap(),
            json,
            "default wire unchanged"
        );
        let [receipt] = layout.receipts() else {
            panic!("{layout:?}");
        };
        assert_eq!(receipt.members, [0..1, 2..3]);
        assert_eq!(
            mant_ir::inline_plain_text(&receipt.expected_children),
            "ZYU"
        );
        assert_eq!(
            receipt.target,
            LinkTarget::External {
                uri: "https://z.test".into()
            }
        );
        let mut matching_paths = Vec::new();
        let scan = mant_ir::scan_navigation_scope_with_budget(
            paired,
            mant_ir::ReferenceScope::Document,
            &mut mant_ir::ReferenceWorkBudget::new(mant_ir::ReferenceScanLimits::default()),
            mant_ir::NavigationScanOptions::default(),
            |event, _| {
                if let mant_ir::NavigationEvent::Link(link) = event
                    && *link.target == receipt.target
                {
                    matching_paths.push(link.location.to_owned().unwrap());
                }
                std::ops::ControlFlow::Continue(())
            },
        );
        assert!(scan.complete());
        assert_eq!(matching_paths.len(), 2);
        assert_eq!(receipt.origin, matching_paths[0]);
        assert_ne!(receipt.origin, matching_paths[1]);
        eprintln!("final sidecar: {receipt:?}");
    }
}
