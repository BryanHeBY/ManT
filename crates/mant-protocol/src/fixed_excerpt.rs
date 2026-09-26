//! Bounded, response-local copies of checked native Fixed display slices.
//!
//! A section read is a physical display view, not a reconstruction from
//! logical text joins or a synthetic Flow document. The producer verifies
//! every slice against the complete Fixed surface before copying it here.

use std::num::NonZeroU32;

use mant_ir::{DisplayStyle, OutputSlice, SourceKey};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Maximum copied visible bytes and physical row/column padding in one read.
pub const MAX_FIXED_EXCERPT_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum copied final-run fragments in one read.
pub const MAX_FIXED_EXCERPT_PARTS: usize = 262_144;

/// One surviving native run slice in a selected structural reading view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedExcerptPart {
    /// Run-relative UTF-8 byte range checked by the producer.
    pub slice: OutputSlice,
    /// One-based final native physical row.
    pub row: NonZeroU32,
    /// Exact zero-based terminal column of this fragment.
    pub column: u32,
    /// Native terminal-cell width, not UTF-8 length or Unicode scalar count.
    pub width: u32,
    /// Final style after native overstrike folding.
    pub style: DisplayStyle,
    /// Exact selected UTF-8 bytes.
    pub text: String,
    /// Surviving source identity, when present on the final run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceKey>,
}

/// A complete selected native display range, with no synthetic text joins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedExcerptSelection {
    /// Fragments in final surface order. Row gaps represent real blank lines.
    pub parts: Vec<FixedExcerptPart>,
}

#[derive(Deserialize)]
#[serde(
    remote = "FixedExcerptSelection",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct FixedExcerptSelectionWire {
    parts: Vec<FixedExcerptPart>,
}

impl<'de> Deserialize<'de> for FixedExcerptSelection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let selection = FixedExcerptSelectionWire::deserialize(deserializer)?;
        selection.validate().map_err(serde::de::Error::custom)?;
        Ok(selection)
    }
}

impl FixedExcerptSelection {
    /// Validate detached UTF-8 lengths, physical order, geometry and budget.
    ///
    /// The producing query additionally checks each part against its original
    /// Fixed surface; a detached response cannot recreate that surface. In
    /// particular, native overstrike can leave a visible scalar on a zero-
    /// width run, so detached validation must not recompute cell width from
    /// Unicode alone.
    ///
    /// # Errors
    /// Returns a finite reason for malformed or oversized copied content.
    pub fn validate(&self) -> Result<(), &'static str> {
        self.budget_use().map(|_| ())
    }

    /// Count copied bytes and physical display padding for a response budget.
    ///
    /// # Errors
    /// Returns a finite reason for malformed or oversized copied content.
    pub fn budget_use(&self) -> Result<u64, &'static str> {
        if self.parts.len() > MAX_FIXED_EXCERPT_PARTS {
            return Err("Fixed excerpt exceeds fragment budget");
        }
        let mut previous: Option<&FixedExcerptPart> = None;
        let mut work = 0u64;
        for part in &self.parts {
            if part.slice.start_byte >= part.slice.end_byte
                || part.slice.end_byte - part.slice.start_byte != part.text.len() as u64
                || part.text.is_empty()
                || part
                    .text
                    .chars()
                    .any(|scalar| scalar.is_control() || matches!(scalar, '\u{2028}' | '\u{2029}'))
                || part.column.checked_add(part.width).is_none()
                || part.column > 1_048_576
                || part.column + part.width > 1_048_576
            {
                return Err("invalid Fixed excerpt fragment");
            }
            let row_gap =
                previous.map_or(0, |prior| part.row.get().saturating_sub(prior.row.get()));
            let prior_end = previous
                .filter(|prior| prior.row == part.row)
                .map_or(0, |prior| prior.column.saturating_add(prior.width));
            work = work
                .checked_add(part.text.len() as u64)
                .and_then(|size| size.checked_add(u64::from(row_gap)))
                .and_then(|size| size.checked_add(u64::from(part.column.saturating_sub(prior_end))))
                .ok_or("Fixed excerpt budget overflows")?;
            if work > MAX_FIXED_EXCERPT_BYTES {
                return Err("Fixed excerpt exceeds copy and geometry budget");
            }
            if let Some(prior) = previous
                && (part.slice.run < prior.slice.run
                    || (part.slice.run == prior.slice.run
                        && (part.slice.start_byte < prior.slice.end_byte
                            || part.row != prior.row
                            || part.style != prior.style
                            || part.source != prior.source))
                    || part.row < prior.row
                    || (part.row == prior.row
                        && part.column < prior.column.saturating_add(prior.width)))
            {
                return Err("unordered Fixed excerpt fragments");
            }
            previous = Some(part);
        }
        Ok(work)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use mant_ir::{DisplayStyle, OutputSlice};

    use super::{FixedExcerptPart, FixedExcerptSelection};

    fn part(run: u32, row: u32, column: u32, text: &str) -> FixedExcerptPart {
        FixedExcerptPart {
            slice: OutputSlice {
                run: NonZeroU32::new(run).unwrap(),
                start_byte: 0,
                end_byte: text.len() as u64,
            },
            row: NonZeroU32::new(row).unwrap(),
            column,
            width: u32::try_from(text.chars().count()).unwrap(),
            style: DisplayStyle {
                bold: false,
                underline: false,
            },
            text: text.to_owned(),
            source: None,
        }
    }

    #[test]
    fn fixed_excerpt_roundtrip_keeps_physical_rows_and_rejects_invalid_ranges() {
        let selection = FixedExcerptSelection {
            parts: vec![part(1, 1, 0, "head"), part(2, 4, 5, "body")],
        };
        assert_eq!(selection.budget_use(), Ok(16));
        let value = serde_json::to_value(&selection).unwrap();
        let decoded: FixedExcerptSelection = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(decoded, selection);

        let mut malformed = value;
        malformed["parts"][1]["slice"]["endByte"] = serde_json::json!(5);
        assert!(serde_json::from_value::<FixedExcerptSelection>(malformed).is_err());

        let overlapping = FixedExcerptSelection {
            parts: vec![part(1, 1, 0, "head"), part(2, 1, 3, "body")],
        };
        assert!(overlapping.validate().is_err());
        let control = FixedExcerptSelection {
            parts: vec![part(1, 1, 0, "a\nb")],
        };
        assert!(control.validate().is_err());

        let mut impossible_run = FixedExcerptSelection {
            parts: vec![part(1, 1, 0, "a"), part(1, 1, 1, "b")],
        };
        impossible_run.parts[1].slice.start_byte = 1;
        impossible_run.parts[1].slice.end_byte = 2;
        assert!(impossible_run.validate().is_ok());
        impossible_run.parts[1].row = NonZeroU32::new(2).unwrap();
        assert!(impossible_run.validate().is_err());
        impossible_run.parts[1].row = NonZeroU32::new(1).unwrap();
        impossible_run.parts[1].style.bold = true;
        assert!(impossible_run.validate().is_err());
        impossible_run.parts[1].style.bold = false;
        impossible_run.parts[1].source = Some(mant_ir::SourceKey::FIRST);
        assert!(impossible_run.validate().is_err());
    }
}
