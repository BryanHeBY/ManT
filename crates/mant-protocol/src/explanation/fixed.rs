//! Closed, response-local excerpts of original Fixed display selections.
//!
//! The producer checks each coordinate against its Fixed surface. The wire
//! additionally proves local UTF-8 lengths, ordering and complete form spans;
//! it never invents a Flow block or asks a consumer to search copied prose.

use std::num::NonZeroU32;

use mant_ir::{DisplayStyle, OutputSlice, SourceKey, TextJoin};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// One exact final-run fragment copied under an explanation response budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationFixedPart {
    /// Byte range in the original Fixed surface's final run.
    pub slice: OutputSlice,
    /// Final physical row containing the run.
    pub row: NonZeroU32,
    /// Run's native starting terminal column; UTF-8 bytes are not columns.
    pub run_column: u32,
    /// Checked terminal column of this selected fragment.
    pub column: u32,
    /// Selected fragment's terminal-cell width, not its UTF-8 byte length.
    pub width: u32,
    /// Final native style after overstrike folding.
    pub style: DisplayStyle,
    /// Exact copied UTF-8 bytes from this slice.
    pub text: String,
    /// Source-qualified native run identity, when it survived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceKey>,
}

/// One bounded selection with native join evidence and no second full body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationFixedSelection {
    /// Ordered, copied final-run fragments.
    pub parts: Vec<ExplanationFixedPart>,
    /// One native join fact per adjacent pair.
    pub joins: Vec<TextJoin>,
}

#[derive(Deserialize)]
#[serde(
    remote = "ExplanationFixedSelection",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct ExplanationFixedSelectionWire {
    parts: Vec<ExplanationFixedPart>,
    joins: Vec<TextJoin>,
}

impl<'de> Deserialize<'de> for ExplanationFixedSelection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let selection = ExplanationFixedSelectionWire::deserialize(deserializer)?;
        selection.validate().map_err(serde::de::Error::custom)?;
        Ok(selection)
    }
}

impl ExplanationFixedSelection {
    /// Check bounded local UTF-8 coverage and selection ordering.
    /// Source/run ownership is checked by the producer against its Fixed body.
    ///
    /// # Errors
    /// Returns a finite reason for malformed detached selection data.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.parts.len() > super::MAX_EXPLANATION_POSITIONS
            || self.joins.len() != self.parts.len().saturating_sub(1)
        {
            return Err("invalid Fixed selection part count");
        }
        let mut previous: Option<&ExplanationFixedPart> = None;
        let mut display_padding = 0u64;
        for part in &self.parts {
            if part.slice.start_byte >= part.slice.end_byte
                || part.slice.end_byte - part.slice.start_byte != part.text.len() as u64
                || part.text.is_empty()
                || part.text.chars().any(char::is_control)
                || part.column < part.run_column
                || part.column.checked_add(part.width).is_none()
                || part.column > 1_048_576
            {
                return Err("invalid Fixed selection fragment");
            }
            let row_gap =
                previous.map_or(0, |prior| part.row.get().saturating_sub(prior.row.get()));
            let prior_end = previous
                .filter(|prior| prior.row == part.row)
                .map_or(0, |prior| prior.column.saturating_add(prior.width));
            display_padding = display_padding
                .checked_add(u64::from(row_gap) + u64::from(part.column.saturating_sub(prior_end)))
                .ok_or("Fixed display padding overflows")?;
            if display_padding > u64::from(super::MAX_EXPLANATION_CONTENT_BYTES) {
                return Err("Fixed display padding exceeds response budget");
            }
            if let Some(prior) = previous
                && (part.slice.run < prior.slice.run
                    || (part.slice.run == prior.slice.run
                        && part.slice.start_byte < prior.slice.end_byte)
                    || part.row < prior.row
                    || (part.row == prior.row
                        && part.column < prior.column.saturating_add(prior.width)))
            {
                return Err("unordered Fixed selection fragment");
            }
            previous = Some(part);
        }
        if self.joins.iter().any(|join| {
            matches!(join, TextJoin::AuthoredSeparator(text)
                if text.is_empty() || !text.bytes().all(|byte| byte == b' '))
        }) {
            return Err("invalid Fixed authored separator");
        }
        Ok(())
    }

    /// Reconstruct one complete logical form only when every native join is
    /// known. Body selections may contain hard or unknown joins and return None.
    #[must_use]
    pub fn complete_text(&self) -> Option<String> {
        let mut text = String::new();
        for (index, part) in self.parts.iter().enumerate() {
            if index > 0 {
                match &self.joins[index - 1] {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator) => text.push_str(separator),
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            text.push_str(&part.text);
        }
        Some(text)
    }
}

/// A complete or partial Unicode scalar range in a returned Fixed form's logical text.
/// Its display fragments resolve through `entry.fixedForms[formIndex]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationFixedFormRange {
    /// Index in this evidence's returned Fixed forms.
    pub form_index: u32,
    /// Inclusive Unicode scalar offset in the joined form.
    pub start_scalar: u64,
    /// Exclusive Unicode scalar offset in the joined form.
    pub end_scalar: u64,
}

impl ExplanationFixedFormRange {
    /// Resolve only a valid UTF-8 range in the returned complete form.
    #[must_use]
    pub fn resolve(&self, forms: &[ExplanationFixedSelection]) -> Option<String> {
        let text = forms.get(self.form_index as usize)?.complete_text()?;
        let start = scalar_to_byte(&text, self.start_scalar)?;
        let end = scalar_to_byte(&text, self.end_scalar)?;
        (start < end)
            .then(|| text.get(start..end).map(str::to_owned))
            .flatten()
    }
}

fn scalar_to_byte(text: &str, scalar: u64) -> Option<usize> {
    let scalar = usize::try_from(scalar).ok()?;
    text.char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .nth(scalar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_form_public_range_counts_scalars_while_slice_addresses_bytes() {
        let key = NonZeroU32::MIN;
        let form = ExplanationFixedSelection {
            parts: vec![ExplanationFixedPart {
                slice: OutputSlice {
                    run: key,
                    start_byte: 0,
                    end_byte: 4,
                },
                row: key,
                run_column: 0,
                column: 0,
                width: 2,
                style: DisplayStyle {
                    bold: false,
                    underline: false,
                },
                text: "中a".into(),
                source: None,
            }],
            joins: Vec::new(),
        };
        let range = ExplanationFixedFormRange {
            form_index: 0,
            start_scalar: 0,
            end_scalar: 2,
        };
        assert_eq!(range.resolve(&[form]).as_deref(), Some("中a"));
        assert!(
            serde_json::from_value::<ExplanationFixedFormRange>(
                serde_json::json!({"formIndex": 0, "startByte": 0, "endByte": 4})
            )
            .is_err()
        );
    }

    #[test]
    fn detached_fixed_geometry_rejects_overlap_and_unbounded_padding() {
        let key = NonZeroU32::MIN;
        let part = ExplanationFixedPart {
            slice: OutputSlice {
                run: key,
                start_byte: 0,
                end_byte: 1,
            },
            row: key,
            run_column: 0,
            column: 0,
            width: 1,
            style: DisplayStyle {
                bold: false,
                underline: false,
            },
            text: "a".into(),
            source: None,
        };
        let mut next = part.clone();
        next.slice.run = NonZeroU32::new(2).unwrap();
        next.text = "b".into();
        let mut selection = ExplanationFixedSelection {
            parts: vec![part, next],
            joins: vec![TextJoin::HardBoundary],
        };
        assert!(selection.validate().is_err(), "overlapping cells");
        selection.parts[1].row =
            NonZeroU32::new(super::super::MAX_EXPLANATION_CONTENT_BYTES + 2).unwrap();
        assert!(selection.validate().is_err(), "unbounded row gap");
    }
}
