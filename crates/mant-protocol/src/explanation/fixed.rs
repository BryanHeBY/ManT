//! Closed, response-local excerpts of original Fixed display selections.
//!
//! The producer checks each coordinate against its Fixed surface. The wire
//! additionally proves local UTF-8 lengths, ordering and complete form spans;
//! it never invents a Flow block or asks a consumer to search copied prose.

use std::num::NonZeroU32;

use mant_ir::{DisplayStyle, OutputSlice, SourceKey, SourceSpan, TextJoin};
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
            matches!(join, TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text)
                if text.is_empty() || !text.bytes().all(|byte| byte == b' '))
        }) {
            return Err("invalid Fixed native separator");
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
                    TextJoin::AuthoredSeparator(separator)
                    | TextJoin::GeneratedSeparator(separator) => text.push_str(separator),
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            text.push_str(&part.text);
        }
        Some(text)
    }
}

/// One bounded native display window for a literal explanation match.
/// Its selection is independent of Flow block paths and may be returned even
/// when the matching Fixed owner body is omitted by the shared copy budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationFixedPreview {
    /// Exact surviving native display fragments in display/source order.
    pub selection: ExplanationFixedSelection,
    /// Start of the complete match in the window's logical Unicode scalars.
    #[schemars(range(max = 1024))]
    pub match_start_scalar: u32,
    /// Exclusive end of the complete match in logical Unicode scalars.
    #[schemars(range(max = 1024))]
    pub match_end_scalar: u32,
    /// Actual authored location of the matched text, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
    /// The window excludes preceding original text.
    pub clipped_before: bool,
    /// The window excludes following original text.
    pub clipped_after: bool,
}

#[derive(Deserialize)]
#[serde(
    remote = "ExplanationFixedPreview",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct ExplanationFixedPreviewWire {
    selection: ExplanationFixedSelection,
    match_start_scalar: u32,
    match_end_scalar: u32,
    source: Option<SourceSpan>,
    clipped_before: bool,
    clipped_after: bool,
}

impl<'de> Deserialize<'de> for ExplanationFixedPreview {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let preview = ExplanationFixedPreviewWire::deserialize(deserializer)?;
        preview.validate().map_err(serde::de::Error::custom)?;
        Ok(preview)
    }
}

impl ExplanationFixedPreview {
    /// Prove that the copied window has one complete logical reading and a
    /// nonempty match within its bounded Unicode-scalar coordinates.
    /// Original-run ownership and authored source keys are checked by the
    /// producer and enclosing document response respectively.
    ///
    /// # Errors
    /// Returns a finite reason for malformed or oversized detached previews.
    pub fn validate(&self) -> Result<(), &'static str> {
        self.selection.validate()?;
        if self.selection.parts.is_empty() {
            return Err("empty Fixed preview selection");
        }
        let mut scalars = 0usize;
        for part in &self.selection.parts {
            scalars = scalars
                .checked_add(
                    part.text
                        .chars()
                        .take(super::MAX_EXPLANATION_PREVIEW_SCALARS + 1)
                        .count(),
                )
                .ok_or("Fixed preview scalar count overflows")?;
            if scalars > super::MAX_EXPLANATION_PREVIEW_SCALARS {
                return Err("Fixed preview exceeds scalar limit");
            }
        }
        for join in &self.selection.joins {
            match join {
                TextJoin::DirectContact => {}
                TextJoin::AuthoredSeparator(separator)
                | TextJoin::GeneratedSeparator(separator) => {
                    scalars = scalars
                        .checked_add(
                            separator
                                .chars()
                                .take(super::MAX_EXPLANATION_PREVIEW_SCALARS + 1)
                                .count(),
                        )
                        .ok_or("Fixed preview scalar count overflows")?;
                    if scalars > super::MAX_EXPLANATION_PREVIEW_SCALARS {
                        return Err("Fixed preview exceeds scalar limit");
                    }
                }
                TextJoin::HardBoundary | TextJoin::Unknown => {
                    return Err("Fixed preview has no complete logical reading");
                }
            }
        }
        if self.selection.complete_text().is_none() {
            return Err("Fixed preview has no complete logical reading");
        }
        let start = usize::try_from(self.match_start_scalar)
            .map_err(|_| "Fixed preview match starts outside window")?;
        let end = usize::try_from(self.match_end_scalar)
            .map_err(|_| "Fixed preview match ends outside window")?;
        if start >= end || end > scalars {
            return Err("Fixed preview match lies outside window");
        }
        Ok(())
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

    fn preview() -> ExplanationFixedPreview {
        let key = NonZeroU32::MIN;
        ExplanationFixedPreview {
            selection: ExplanationFixedSelection {
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
            },
            match_start_scalar: 0,
            match_end_scalar: 2,
            source: None,
            clipped_before: false,
            clipped_after: false,
        }
    }

    #[test]
    fn fixed_preview_uses_scalar_match_coordinates_and_closed_native_selection() {
        let valid = preview();
        assert_eq!(valid.selection.complete_text().as_deref(), Some("中a"));
        assert!(valid.validate().is_ok());
        let encoded = serde_json::to_value(&valid).unwrap();
        assert_eq!(
            serde_json::from_value::<ExplanationFixedPreview>(encoded.clone()).unwrap(),
            valid
        );
        let mut byte_range = encoded.clone();
        byte_range["matchEndByte"] = 4.into();
        assert!(serde_json::from_value::<ExplanationFixedPreview>(byte_range).is_err());
        let mut outside = encoded.clone();
        outside["matchEndScalar"] = 3.into();
        assert!(serde_json::from_value::<ExplanationFixedPreview>(outside).is_err());
        let mut empty = encoded;
        empty["matchStartScalar"] = 2.into();
        assert!(serde_json::from_value::<ExplanationFixedPreview>(empty).is_err());
    }

    #[test]
    fn fixed_preview_requires_complete_bounded_logical_text() {
        let mut invalid = preview();
        invalid.selection.parts.push(ExplanationFixedPart {
            slice: OutputSlice {
                run: NonZeroU32::new(2).unwrap(),
                start_byte: 0,
                end_byte: 1,
            },
            row: NonZeroU32::new(2).unwrap(),
            run_column: 0,
            column: 0,
            width: 1,
            style: DisplayStyle {
                bold: false,
                underline: false,
            },
            text: "b".into(),
            source: None,
        });
        invalid.selection.joins.push(TextJoin::HardBoundary);
        assert!(invalid.validate().is_err());
        invalid.selection.joins[0] = TextJoin::Unknown;
        assert!(invalid.validate().is_err());
        invalid.selection.joins[0] = TextJoin::DirectContact;
        assert!(invalid.validate().is_ok());
        invalid.selection.joins[0] = TextJoin::GeneratedSeparator(" ".into());
        assert_eq!(invalid.selection.complete_text().as_deref(), Some("中a b"));
        assert!(invalid.validate().is_ok());
        invalid.selection.joins[0] = TextJoin::GeneratedSeparator("\t".into());
        assert!(invalid.validate().is_err());

        let mut oversized = preview();
        oversized.selection.parts[0].text =
            "a".repeat(super::super::MAX_EXPLANATION_PREVIEW_SCALARS + 1);
        oversized.selection.parts[0].slice.end_byte =
            oversized.selection.parts[0].text.len() as u64;
        oversized.match_end_scalar = 1;
        assert!(oversized.validate().is_err());
    }

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
