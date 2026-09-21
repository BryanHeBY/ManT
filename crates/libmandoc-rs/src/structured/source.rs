//! Source identity, coordinates, spans, and provenance.

use super::{SourceKey, SpanKey};
use std::{num::NonZeroU32, ops::Range};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceFormat {
    Man,
    Mdoc,
    Markdown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceIdentity {
    Path(String),
    BundleMember(String),
    Anonymous(String),
}

impl SourceIdentity {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Path(name) | Self::BundleMember(name) | Self::Anonymous(name) => name,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceCoordinates {
    DecodedUtf8Bytes,
    NativeNormalizedBytes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRecord {
    pub(crate) key: SourceKey,
    pub(crate) identity: SourceIdentity,
    pub(crate) format: SourceFormat,
    pub(crate) decoded_byte_len: u64,
    pub(crate) content_sha256: Option<[u8; 32]>,
    pub(crate) coordinates: SourceCoordinates,
}

impl SourceRecord {
    #[must_use]
    pub const fn key(&self) -> SourceKey {
        self.key
    }
    #[must_use]
    pub const fn identity(&self) -> &SourceIdentity {
        &self.identity
    }
    #[must_use]
    pub const fn format(&self) -> SourceFormat {
        self.format
    }
    #[must_use]
    pub const fn decoded_byte_len(&self) -> u64 {
        self.decoded_byte_len
    }
    #[must_use]
    pub const fn content_sha256(&self) -> Option<&[u8; 32]> {
        self.content_sha256.as_ref()
    }
    #[must_use]
    pub const fn coordinates(&self) -> SourceCoordinates {
        self.coordinates
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineColumn {
    line: NonZeroU32,
    column: NonZeroU32,
}

impl LineColumn {
    #[must_use]
    pub const fn new(line: u32, column: u32) -> Option<Self> {
        match (NonZeroU32::new(line), NonZeroU32::new(column)) {
            (Some(line), Some(column)) => Some(Self { line, column }),
            _ => None,
        }
    }

    #[must_use]
    pub const fn line(self) -> u32 {
        self.line.get()
    }

    #[must_use]
    pub const fn column(self) -> u32 {
        self.column.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineColumns {
    pub(crate) start: LineColumn,
    pub(crate) end: Option<LineColumn>,
}

impl LineColumns {
    #[must_use]
    pub const fn new(start: LineColumn, end: Option<LineColumn>) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn start(self) -> LineColumn {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> Option<LineColumn> {
        self.end
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    pub(crate) source: SourceKey,
    pub(crate) line_columns: Option<LineColumns>,
    pub(crate) byte_range: Option<Range<u64>>,
}

impl SourceSpan {
    #[must_use]
    pub const fn source(&self) -> SourceKey {
        self.source
    }
    #[must_use]
    pub const fn line_columns(&self) -> Option<LineColumns> {
        self.line_columns
    }
    #[must_use]
    pub const fn byte_range(&self) -> Option<&Range<u64>> {
        self.byte_range.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Provenance {
    Authored { span: SpanKey },
    Generated { trigger: Option<SpanKey> },
    Unknown,
}
