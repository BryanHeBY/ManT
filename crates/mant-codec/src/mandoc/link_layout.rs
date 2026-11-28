//! Seal native description receipts to final structural link locations.
use std::{
    collections::HashMap,
    ops::{ControlFlow, Range},
};

use mant_ir::{
    Document, Inline, LinkLayoutReceipt, NavigationEvent, NavigationScanOptions,
    ReferenceScanLimits, ReferenceScope, ReferenceWorkBudget, scan_navigation_scope_with_budget,
    visit::{self, VisitMut},
};

use crate::{NativeLinkLayout, NativeLinkRestriction};

const TOKEN: &str = "\0mant:link-layout:";
pub(super) const MAX_LABEL: usize = 4096;
pub(super) const MAX_RUNS: usize = 64;
const MAX_RECEIPTS: usize = 256;
const MAX_RESTRICTIONS: usize = 1024;

pub(super) fn marker(
    occurrence: impl std::fmt::Display,
    children: &[Inline],
    painted_scalars: Option<usize>,
    runs: &[Range<usize>],
) -> String {
    let mut marker = format!("{TOKEN}{occurrence};");
    if !(1..=MAX_RUNS).contains(&runs.len()) {
        return marker;
    }
    let Some(expected) = bounded_label(children) else {
        return marker;
    };
    if painted_scalars != Some(expected.chars().count()) {
        return marker;
    }
    for range in runs {
        use std::fmt::Write;
        let _ = write!(marker, "{}:{},", range.start, range.end);
    }
    marker.push(';');
    marker.push_str(&expected);
    marker
}

// Publication can retain a link's text while cutting an earlier nested link.
// A truncated wrapper has lost the sender's coordinates, but not the fact
// that those coordinates are needed before permitting activation.
pub(in crate::mandoc) fn restrict_marker(title: &str) -> Option<String> {
    occurrence(title).map(|owner| format!("{TOKEN}{owner};"))
}

fn occurrence(title: &str) -> Option<u32> {
    title.strip_prefix(TOKEN)?.split_once(';')?.0.parse().ok()
}

// Bound both collected text and the number/depth of styled nodes copied at sealing.
fn bounded_label(nodes: &[Inline]) -> Option<String> {
    fn append(
        nodes: &[Inline],
        text: &mut String,
        remaining: &mut usize,
        depth: usize,
    ) -> Option<()> {
        if depth > 64 {
            return None;
        }
        *remaining = remaining.checked_sub(nodes.len())?;
        for node in nodes {
            let leaf = match node {
                Inline::Text { value } | Inline::Code { value } => value,
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    append(children, text, remaining, depth + 1)?;
                    continue;
                }
                Inline::LineBreak {} => "\n",
                Inline::Anchor { id, .. } if id.as_str().len() <= MAX_LABEL => continue,
                Inline::Anchor { .. } | Inline::Equation { .. } | Inline::Link { .. } => {
                    return None;
                }
            };
            if leaf.len() > MAX_LABEL - text.len() {
                return None;
            }
            text.push_str(leaf);
        }
        Some(())
    }
    let mut label = String::new();
    append(nodes, &mut label, &mut 512, 0)?;
    Some(label)
}

pub(super) fn is_marker(title: &str) -> bool {
    title.starts_with(TOKEN)
}

fn decode(title: &str, children: &[Inline]) -> Option<(u32, Vec<Range<usize>>)> {
    let mut fields = title.strip_prefix(TOKEN)?.splitn(3, ';');
    let occurrence = fields.next()?.parse().ok()?;
    let members = fields.next()?.strip_suffix(',')?;
    let expected = fields.next()?;
    if expected.len() > MAX_LABEL || expected != bounded_label(children)? {
        return None;
    }
    let len = expected.chars().count();
    let mut ranges = Vec::new();
    for member in members.split(',') {
        let (start, end) = member.split_once(':')?;
        let (start, end) = (start.parse().ok()?, end.parse().ok()?);
        if start >= end
            || end > len
            || ranges
                .last()
                .is_some_and(|previous: &Range<usize>| previous.end >= start)
            || ranges.len() == MAX_RUNS
        {
            return None;
        }
        ranges.push(start..end);
    }
    (!ranges.is_empty()).then_some((occurrence, ranges))
}

pub(super) fn seal(document: &mut Document) -> NativeLinkLayout {
    let mut layout = NativeLinkLayout::default();
    let mut unidentified = false;
    let mut budget = ReferenceWorkBudget::new(ReferenceScanLimits::default());
    let _ = scan_navigation_scope_with_budget(
        document,
        ReferenceScope::Document,
        &mut budget,
        NavigationScanOptions {
            targets: false,
            entry_sets: false,
            ..Default::default()
        },
        |event, _| {
            if let NavigationEvent::Link(link) = event
                && let Inline::Link {
                    title: Some(title),
                    children,
                    ..
                } = link.link
                && is_marker(title)
            {
                let Some(origin) = link.location.to_owned() else {
                    unidentified = true;
                    return ControlFlow::Continue(());
                };
                if layout.receipts.len() < MAX_RECEIPTS
                    && let Some((occurrence, members)) = decode(title, children)
                {
                    layout.receipts.push(LinkLayoutReceipt {
                        occurrence,
                        members,
                        origin,
                        target: link.target.clone(),
                        expected_children: children.clone(),
                    });
                } else if layout.restrictions.len() < MAX_RESTRICTIONS {
                    layout.restrictions.push(NativeLinkRestriction {
                        origin,
                        target: link.target.clone(),
                    });
                } else {
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        },
    );
    let located = layout.receipts.len() + layout.restrictions.len();

    // Existing cleanup visits the entire document. During that same walk,
    // count matches only for the bounded proven prefix. Later or restricted
    // duplicates therefore invalidate their own alleged occurrence without
    // revoking independent earlier proofs when the budget runs out.
    let mut occurrences: HashMap<_, u8> = layout
        .receipts
        .iter()
        .map(|receipt| (receipt.occurrence, 0))
        .collect();
    let mut cleanup = Remove {
        occurrences: &mut occurrences,
        marker_count: 0,
    };
    cleanup.visit_document_mut(document);
    layout.restrict_unidentified = unidentified || cleanup.marker_count != located;
    layout.receipts.retain(|receipt| {
        if occurrences.get(&receipt.occurrence) == Some(&1) {
            return true;
        }
        if layout.restrictions.len() < MAX_RESTRICTIONS {
            layout.restrictions.push(NativeLinkRestriction {
                origin: receipt.origin.clone(),
                target: receipt.target.clone(),
            });
        } else {
            layout.restrict_unidentified = true;
        }
        false
    });
    layout
}

struct Remove<'a> {
    occurrences: &'a mut HashMap<u32, u8>,
    marker_count: usize,
}

impl VisitMut for Remove<'_> {
    fn visit_inline_mut(&mut self, inline: &mut Inline) {
        if let Inline::Link { title, .. } = inline
            && let Some(marker) = title.as_deref().filter(|title| is_marker(title))
        {
            self.marker_count += 1;
            if let Some(count) = occurrence(marker).and_then(|key| self.occurrences.get_mut(&key)) {
                *count = count.saturating_add(1);
            }
            *title = None;
        }
        visit::walk_inline_mut(self, inline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::{Block, DocumentSource, LinkTarget, SourceFormat};

    #[test]
    fn a_late_duplicate_revokes_only_its_own_membership() {
        // Synthetic sealed input, not a parser fixture: test the uniqueness
        // check where a later receipt no longer fits the positive budget.
        let pieces = [Inline::Text {
            value: "ZYU".into(),
        }];
        let inline = |owner| Inline::Link {
            title: Some(marker(owner, &pieces, Some(3), &[0..1, 2..3])),
            target: LinkTarget::External {
                uri: "https://one.test".into(),
            },
            children: pieces.to_vec(),
        };
        let mut children: Vec<_> = (0..MAX_RECEIPTS).map(inline).collect();
        children.push(inline(0));
        let mut document = Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Mdoc,
                path: None,
            },
            meta: Default::default(),
            heading: None,
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: vec![Block::Paragraph {
                children,
                inline_layout: Default::default(),
                layout: Default::default(),
                source: None,
            }],
            sections: Vec::new(),
        };
        let layout = seal(&mut document);
        assert_eq!(layout.receipts.len(), MAX_RECEIPTS - 1);
        assert!(
            layout
                .receipts
                .iter()
                .all(|receipt| receipt.occurrence != 0)
        );
        assert_eq!(layout.restrictions.len(), 2);
        assert!(!layout.restrict_unidentified);
        let Block::Paragraph { children, .. } = &document.blocks[0] else {
            unreachable!()
        };
        assert!(
            children
                .iter()
                .all(|inline| matches!(inline, Inline::Link { title: None, .. }))
        );
    }
}
