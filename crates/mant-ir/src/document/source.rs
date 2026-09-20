//! Original-source identities, provenance and checked coordinate value types.

use std::{fmt, num::NonZeroU32};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Stable document-local key for one entry in [`crate::Document::sources`].
///
/// Keys are one-based so zero remains an invalid sentinel at every wire and
/// in-memory boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct SourceKey(NonZeroU32);

impl SourceKey {
    /// The first source-table key.
    pub const FIRST: Self = Self(NonZeroU32::MIN);

    /// Construct a nonzero key.
    #[must_use]
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Return the one-based integer representation.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl<'de> Deserialize<'de> for SourceKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        NonZeroU32::deserialize(deserializer).map(Self)
    }
}

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

/// Caller-visible identity of a source participating in one document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SourceIdentity {
    /// A caller-facing filesystem or virtual path.
    Path {
        /// Stable path; never a temporary decompression filename.
        name: String,
    },
    /// A logical member name inside a caller-provided source bundle.
    BundleMember {
        /// Normalized relative member name using `/` separators.
        name: String,
    },
    /// A stable caller-provided label for source without a path.
    Anonymous {
        /// Nonempty logical label.
        name: String,
    },
}

impl SourceIdentity {
    /// Return the stable caller-facing identity text.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Path { name } | Self::BundleMember { name } | Self::Anonymous { name } => name,
        }
    }

    /// Return a path-like identity when one exists.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Path { name } | Self::BundleMember { name } => Some(name),
            Self::Anonymous { .. } => None,
        }
    }
}

/// Coordinate domain used by byte ranges in one source record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SourceCoordinates {
    /// Offsets index the complete decoded UTF-8 source buffer.
    DecodedUtf8Bytes,
    /// Native input was normalized and exact authored byte offsets are unavailable.
    NativeNormalizedBytes,
}

impl<'de> Deserialize<'de> for SourceCoordinates {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "kebab-case")]
        enum Wire {
            DecodedUtf8Bytes(Empty),
            NativeNormalizedBytes(Empty),
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Empty {}

        match Wire::deserialize(deserializer)? {
            Wire::DecodedUtf8Bytes(Empty {}) => Ok(Self::DecodedUtf8Bytes),
            Wire::NativeNormalizedBytes(Empty {}) => Ok(Self::NativeNormalizedBytes),
        }
    }
}

/// One source participating in the normalized document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceRecord {
    /// Dense one-based key, equal to this record's source-table position.
    pub key: SourceKey,
    /// Stable caller-facing identity.
    pub identity: SourceIdentity,
    /// Syntax family consumed for this source.
    pub format: SourceFormat,
    /// Length of the complete buffer in the declared coordinate domain.
    pub decoded_byte_length: u64,
    /// SHA-256 of the complete decoded source buffer, when retained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<[u8; 32]>,
    /// Coordinate domain for spans referencing this source.
    pub coordinates: SourceCoordinates,
}

/// Zero-based byte offset in the coordinate domain declared by a source.
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
pub struct TextSize(u64);

impl TextSize {
    /// Construct an offset from a zero-based byte count.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the underlying byte count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Convert a platform-sized offset without narrowing it.
    #[must_use]
    pub fn from_usize_saturating(value: usize) -> Self {
        Self(u64::try_from(value).unwrap_or(u64::MAX))
    }
}

/// Half-open byte range (`start..end`) in one source's coordinate domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

/// Location in one member of [`crate::Document::sources`].
///
/// Lines and columns are one-based for diagnostics. `byte_range`, when the
/// source coordinate domain supports exact offsets, is the canonical
/// machine-facing boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceSpan {
    /// Source-table owner of every coordinate in this span.
    pub source: SourceKey,
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

/// Authorship provenance for content that may not map to an exact range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Provenance {
    /// Content copied directly from authored source.
    Authored {
        /// Exact authored location.
        span: SourceSpan,
    },
    /// Content generated by normalization, optionally attributed to a trigger.
    Generated {
        /// Authored construct that caused generation.
        #[serde(skip_serializing_if = "Option::is_none")]
        trigger: Option<SourceSpan>,
    },
    /// Provenance is unavailable.
    Unknown,
}

impl<'de> Deserialize<'de> for Provenance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "kebab-case")]
        enum Wire {
            Authored(Authored),
            Generated(Generated),
            Unknown(Empty),
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Authored {
            span: SourceSpan,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Generated {
            #[serde(default)]
            trigger: Option<SourceSpan>,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Empty {}

        match Wire::deserialize(deserializer)? {
            Wire::Authored(Authored { span }) => Ok(Self::Authored { span }),
            Wire::Generated(Generated { trigger }) => Ok(Self::Generated { trigger }),
            Wire::Unknown(Empty {}) => Ok(Self::Unknown),
        }
    }
}

/// A structural source relation that cannot cross the public IR boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRelationError {
    detail: String,
}

impl SourceRelationError {
    pub(crate) fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for SourceRelationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for SourceRelationError {}
