//! Bounded original-link label projection shared by protocol and UI consumers.

use super::{ReferenceScanStop, ReferenceWorkBudget};
use crate::{ContentContext, Inline, InlineView};

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
    nodes: &[Inline],
    depth: usize,
    budget: &mut ReferenceWorkBudget,
    max_bytes: usize,
) -> Result<ReferenceLabel, ReferenceScanStop> {
    ContentContext::detached().reference_label(nodes, depth, budget, max_bytes)
}

impl<'store> ContentContext<'store> {
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
                    let inspect = value
                        .len()
                        .min(limit.saturating_sub(label.text.len()).saturating_add(4));
                    budget.consume(depth.saturating_add(1), 0, inspect)?;
                    for character in value.chars() {
                        if character.len_utf8() > limit.saturating_sub(label.text.len()) {
                            label.truncated = true;
                            return Ok(());
                        }
                        label.text.push(character);
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReferenceScanLimits;
    #[test]
    fn multibyte_label_prefix_is_bounded_without_reading_huge_suffix() {
        let mut budget = ReferenceWorkBudget::new(ReferenceScanLimits {
            bytes: 8,
            ..Default::default()
        });
        let label = reference_label(
            &[Inline::Text {
                value: format!("éé{}", "x".repeat(100_000)),
            }],
            0,
            &mut budget,
            3,
        )
        .unwrap();
        assert_eq!(label.text, "é");
        assert!(label.truncated);
        assert!(budget.remaining_bytes() > 0);
    }
}
