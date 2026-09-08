//! Shared evidence ordering, per-class accounting and literal preview coordinates.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Mutually exclusive owner category, in normative presentation/page order.
/// This is a source-evidence distinction, not a confidence or relevance score.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceClass {
    /// A semantic owner with an exact name, complete form or identity match.
    DirectEntry,
    /// An independent owner reached through a validated explicit relationship.
    RelatedEntry,
    /// An owner included only because its original content mentions the query.
    EntryMention,
    /// An ordinary block, without a semantic owner, mentioning the query.
    ContextMention,
}

impl EvidenceClass {
    /// All categories in normative order, including categories with zero results.
    pub const ALL: [Self; 4] = [
        Self::DirectEntry,
        Self::RelatedEntry,
        Self::EntryMention,
        Self::ContextMention,
    ];

    /// Stable plain-text group title shared by all host presentations.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::DirectEntry => "Direct entries",
            Self::RelatedEntry => "Explicitly related entries",
            Self::EntryMention => "Mentions in other entries",
            Self::ContextMention => "Mentions in ordinary content",
        }
    }
}

/// Fixed explanation ordering; no legacy/source-only ordering mode exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceOrder {
    /// Class, resolved-document BFS position, then original IR owner/block order.
    #[default]
    ClassThenSource,
}

/// Collected lower-bound total and the count returned on the current page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceCount {
    /// Collected owners before pagination (not exhaustive recall).
    pub total: u32,
    /// Owners included on the current page.
    pub returned: u32,
}

/// Fixed four-class counts. Each column sums to the response total/returned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceCounts {
    /// Direct semantic matches.
    pub direct_entry: EvidenceCount,
    /// Explicitly related semantic owners.
    pub related_entry: EvidenceCount,
    /// Other semantic owners with literal mentions.
    pub entry_mention: EvidenceCount,
    /// Ordinary content blocks with literal mentions.
    pub context_mention: EvidenceCount,
}
impl EvidenceCounts {
    /// Inspect a class without deriving it from optional materialized details.
    #[must_use]
    pub const fn get(&self, class: EvidenceClass) -> EvidenceCount {
        match class {
            EvidenceClass::DirectEntry => self.direct_entry,
            EvidenceClass::RelatedEntry => self.related_entry,
            EvidenceClass::EntryMention => self.entry_mention,
            EvidenceClass::ContextMention => self.context_mention,
        }
    }
    /// Account for one collected owner and, optionally, its returned record.
    pub fn record(&mut self, class: EvidenceClass, returned: bool) {
        let count = match class {
            EvidenceClass::DirectEntry => &mut self.direct_entry,
            EvidenceClass::RelatedEntry => &mut self.related_entry,
            EvidenceClass::EntryMention => &mut self.entry_mention,
            EvidenceClass::ContextMention => &mut self.context_mention,
        };
        count.total = count.total.saturating_add(1);
        count.returned = count.returned.saturating_add(u32::from(returned));
    }
}

/// One representative literal window, not a replacement IR block or a summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationPreview {
    /// Final IR coordinate rooted at "root" or "sections/sN[/sN...]";
    /// bN selects a block, iN/dN a list/definition item, and rN/cN a table cell.
    /// All indices are zero-based; this is not a source/Markdown byte offset.
    pub block_path: String,
    /// Matched block source position, when known (never a substituted entry span).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<mant_ir::SourceSpan>,
    /// Safely projected original text, at most 1024 Unicode scalar values.
    pub text: String,
    /// Start of the complete match in text, in zero-based Unicode scalars.
    pub match_start_char: u32,
    /// Exclusive end of the complete match in text, in Unicode scalars.
    pub match_end_char: u32,
    /// Positions of this reported match in an available returned body, not
    /// offsets in this clipped window. Empty when the body is unavailable.
    pub content_ranges: Vec<super::ExplanationContentRange>,
    /// The representative window excludes preceding block text.
    pub clipped_before: bool,
    /// The representative window excludes following block text.
    pub clipped_after: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn class_order_counts_and_closed_preview_are_explicit() {
        assert!(EvidenceClass::ALL.windows(2).all(|w| w[0] < w[1]));
        let mut counts = EvidenceCounts::default();
        counts.record(EvidenceClass::DirectEntry, true);
        counts.record(EvidenceClass::ContextMention, false);
        assert_eq!(
            counts.direct_entry,
            EvidenceCount {
                total: 1,
                returned: 1
            }
        );
        assert_eq!(
            counts.context_mention,
            EvidenceCount {
                total: 1,
                returned: 0
            }
        );
        assert_eq!(
            serde_json::to_value(EvidenceOrder::default()).unwrap(),
            "class-then-source"
        );
        assert!(
            serde_json::from_value::<EvidenceCounts>(
                serde_json::json!({"directEntry":{"total":0,"returned":0}})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<ExplanationPreview>(serde_json::json!({
                "blockPath":"root/b0", "text":"日本", "matchStartChar":0, "matchEndChar":2,
                "clippedBefore":false, "clippedAfter":false, "unknown":true
            }))
            .is_err()
        );
    }
}
