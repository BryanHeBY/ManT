//! Original-source formats and checked coordinate value types.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Source format consumed by the normalization engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SourceFormat {
    /// Traditional man(7) macros.
    Man,
    /// Semantic mdoc(7) macros.
    Mdoc,
    /// Markdown with `ManT` semantic extensions.
    Markdown,
}

/// Original source identity; temporary decompression paths must not appear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSource {
    /// Syntax family consumed by the parser.
    pub format: SourceFormat,
    /// Stable caller-facing path, when the source has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Zero-based UTF-8 byte offset in the original source.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(transparent)]
pub struct TextSize(u32);

impl TextSize {
    /// Construct an offset from a zero-based UTF-8 byte count.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Return the underlying UTF-8 byte count.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Convert a platform-sized offset, clamping values above `u32::MAX`.
    #[must_use]
    pub fn from_usize_saturating(value: usize) -> Self {
        Self(u32::try_from(value).unwrap_or(u32::MAX))
    }
}

/// Half-open UTF-8 byte range (`start..end`) in the original source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextRange {
    /// Inclusive start offset.
    pub start: TextSize,
    /// Exclusive end offset.
    pub end: TextSize,
}

impl TextRange {
    /// Construct a half-open range.
    ///
    /// # Panics
    ///
    /// Panics when `end` precedes `start`.
    #[must_use]
    pub fn new(start: TextSize, end: TextSize) -> Self {
        assert!(start <= end, "a source range cannot end before it starts");
        Self { start, end }
    }

    /// Return whether the range contains no source bytes.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start.0 == self.end.0
    }
}

/// Location in the original source file.
///
/// Lines and columns are one-based for diagnostics. `byte_range`, when the
/// parser provides exact offsets, is the canonical machine-facing boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceSpan {
    /// Exact half-open byte range, when supplied by the parser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_range: Option<TextRange>,
    /// One-based starting line.
    pub line: u32,
    /// One-based starting column.
    pub column: u32,
    /// One-based inclusive ending line, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    /// One-based exclusive ending column, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u32>,
}
