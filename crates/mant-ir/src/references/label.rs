//! Bounded original-link label projection shared by protocol and UI consumers.

use super::{ReferenceScanStop, ReferenceWorkBudget};
use crate::{
    ContentAtomKind, ContentContext, Inline, InlineView, LinkLabelPart, LinkOccurrenceKey,
};

/// One plain visible label prefix. An empty original label remains empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceLabel {
    /// Original visible text, without target fallback or generated styling.
    pub text: String,
    /// Additional visible content was omitted by the byte limit.
    pub truncated: bool,
}

/// Project a link's label under the operation's shared work budget.
/// `depth` is the original link location depth; label wrappers continue it.
/// The materialized label is at most 4 KiB, at a valid UTF-8 boundary.
///
/// # Errors
/// Returns a latched scan limit before unbudgeted traversal or text inspection.
pub fn reference_label(
    content: ContentContext<'_>,
    nodes: &[Inline],
    depth: usize,
    budget: &mut ReferenceWorkBudget,
    max_bytes: usize,
) -> Result<ReferenceLabel, ReferenceScanStop> {
    content.reference_label(nodes, depth, budget, max_bytes)
}

/// Project the complete logical label of one link occurrence, even when its
/// structural wrappers are split across roots or display fragments.
///
/// # Errors
/// Returns an invalid-root or latched budget limit before unbounded work.
pub fn reference_occurrence_label(
    content: ContentContext<'_>,
    key: LinkOccurrenceKey,
    depth: usize,
    budget: &mut ReferenceWorkBudget,
    max_bytes: usize,
) -> Result<ReferenceLabel, ReferenceScanStop> {
    content.reference_occurrence_label(key, depth, budget, max_bytes)
}

impl<'store> ContentContext<'store> {
    /// Bounded label read from the occurrence table, not one wrapper's children.
    ///
    /// # Errors
    /// Returns an invalid-root or latched budget limit before unbounded work.
    pub fn reference_occurrence_label(
        self,
        key: LinkOccurrenceKey,
        depth: usize,
        budget: &mut ReferenceWorkBudget,
        max_bytes: usize,
    ) -> Result<ReferenceLabel, ReferenceScanStop> {
        let occurrence = self.occurrence(key).ok_or(ReferenceScanStop::InvalidRoot)?;
        let mut label = ReferenceLabel {
            text: String::new(),
            truncated: false,
        };
        let limit = max_bytes.min(4096);
        for part in &occurrence.label {
            budget.consume(depth.saturating_add(1), 1, 0)?;
            match part {
                LinkLabelPart::Content { content } => {
                    let value = self
                        .resolve_text(*content)
                        .ok_or(ReferenceScanStop::InvalidRoot)?;
                    append_visible_text(value, depth.saturating_add(1), budget, limit, &mut label)?;
                }
                LinkLabelPart::HardBreak { atom } => {
                    let record = self.atom(*atom).ok_or(ReferenceScanStop::InvalidRoot)?;
                    if !matches!(record.kind, ContentAtomKind::HardBreak {}) {
                        return Err(ReferenceScanStop::InvalidRoot);
                    }
                    budget.consume(depth.saturating_add(1), 0, 1)?;
                    if label.text.len() == limit {
                        label.truncated = true;
                    } else {
                        label.text.push('\n');
                    }
                }
            }
            if label.truncated {
                break;
            }
        }
        Ok(label)
    }

    /// Project a link label through this content store under the caller's
    /// shared reference-work budget.
    ///
    /// # Errors
    ///
    /// Returns [`ReferenceScanStop::InvalidRoot`] when retained label content
    /// does not resolve in this store. Other variants report ordinary scan
    /// limits before unbudgeted traversal or text inspection.
    pub fn reference_label(
        self,
        nodes: &'store [Inline],
        depth: usize,
        budget: &mut ReferenceWorkBudget,
        max_bytes: usize,
    ) -> Result<ReferenceLabel, ReferenceScanStop> {
        let mut label = ReferenceLabel {
            text: String::new(),
            truncated: false,
        };
        self.append_reference_label(nodes, depth, budget, max_bytes.min(4096), &mut label)?;
        Ok(label)
    }

    fn append_reference_label(
        self,
        nodes: &'store [Inline],
        depth: usize,
        budget: &mut ReferenceWorkBudget,
        limit: usize,
        label: &mut ReferenceLabel,
    ) -> Result<(), ReferenceScanStop> {
        for node in nodes {
            budget.consume(depth.saturating_add(1), 1, 0)?;
            match self
                .inline(node)
                .map_err(|_| ReferenceScanStop::InvalidRoot)?
            {
                InlineView::Text(value) | InlineView::Code(value) => {
                    append_visible_text(value, depth.saturating_add(1), budget, limit, label)?;
                }
                InlineView::LineBreak => {
                    budget.consume(depth.saturating_add(1), 0, 1)?;
                    if label.text.len() == limit {
                        label.truncated = true;
                        return Ok(());
                    }
                    label.text.push('\n');
                }
                InlineView::Strong(children) | InlineView::Emphasis(children) => {
                    self.append_reference_label(
                        children,
                        depth.saturating_add(1),
                        budget,
                        limit,
                        label,
                    )?;
                }
                InlineView::Link(link) => {
                    self.append_reference_label(
                        link.children(),
                        depth.saturating_add(1),
                        budget,
                        limit,
                        label,
                    )?;
                }
                InlineView::Anchor(_) => {}
            }
            if label.truncated {
                break;
            }
        }
        Ok(())
    }
}

fn append_visible_text(
    value: &str,
    depth: usize,
    budget: &mut ReferenceWorkBudget,
    limit: usize,
    label: &mut ReferenceLabel,
) -> Result<(), ReferenceScanStop> {
    let inspect = value
        .len()
        .min(limit.saturating_sub(label.text.len()).saturating_add(4));
    budget.consume(depth, 0, inspect)?;
    for character in value.chars() {
        if character.len_utf8() > limit.saturating_sub(label.text.len()) {
            label.truncated = true;
            break;
        }
        label.text.push(character);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReferenceScanLimits;
    #[test]
    fn multibyte_label_prefix_is_bounded_without_reading_huge_suffix() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let inline = fixture.text(format!("éé{}", "x".repeat(100_000)));
        let store = fixture.finish();
        let mut budget = ReferenceWorkBudget::new(ReferenceScanLimits {
            bytes: 8,
            ..Default::default()
        });
        let label = reference_label(store.content(), &[inline], 0, &mut budget, 3).unwrap();
        assert_eq!(label.text, "é");
        assert!(label.truncated);
        assert!(budget.remaining_bytes() > 0);
    }
}
