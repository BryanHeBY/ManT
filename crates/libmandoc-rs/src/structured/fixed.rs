//! Native physical geometry referring to the same logical content keys.

use std::ops::Range;

use super::{
    ContentAtomKey, ContentPointKey, NativeBlockKey, NativeFixedLineKey, NativeFixedViewKey,
    NativeTableKey, OwnerKey, ProvenanceKey,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFixedView {
    pub(crate) key: NativeFixedViewKey,
    pub(crate) owner: OwnerKey,
    pub(crate) block: NativeBlockKey,
    pub(crate) table: Option<NativeTableKey>,
    pub(crate) provenance: ProvenanceKey,
}

impl NativeFixedView {
    #[must_use]
    pub const fn key(&self) -> NativeFixedViewKey {
        self.key
    }
    #[must_use]
    pub const fn owner(&self) -> OwnerKey {
        self.owner
    }
    #[must_use]
    pub const fn block(&self) -> NativeBlockKey {
        self.block
    }
    #[must_use]
    pub const fn table(&self) -> Option<NativeTableKey> {
        self.table
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFixedLine {
    pub(crate) key: NativeFixedLineKey,
    pub(crate) view: NativeFixedViewKey,
    pub(crate) ordinal: u32,
    pub(crate) terminal_columns: u32,
}

impl NativeFixedLine {
    #[must_use]
    pub const fn key(&self) -> NativeFixedLineKey {
        self.key
    }
    #[must_use]
    pub const fn view(&self) -> NativeFixedViewKey {
        self.view
    }
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }
    #[must_use]
    pub const fn terminal_columns(&self) -> u32 {
        self.terminal_columns
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeCellMapKind {
    Affine { columns_per_scalar: u8 },
    GraphemeCluster,
    Overlay,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativePlacementTarget {
    Content {
        atom: ContentAtomKey,
        byte_start: u32,
        byte_end: u32,
    },
    Point(ContentPointKey),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativePlacement {
    pub(crate) line: NativeFixedLineKey,
    pub(crate) target: NativePlacementTarget,
    pub(crate) scalar_range: Range<u32>,
    pub(crate) column_range: Range<u32>,
    pub(crate) map: NativeCellMapKind,
}

impl NativePlacement {
    #[must_use]
    pub const fn line(&self) -> NativeFixedLineKey {
        self.line
    }
    #[must_use]
    pub const fn target(&self) -> NativePlacementTarget {
        self.target
    }
    #[must_use]
    pub fn scalar_range(&self) -> Range<u32> {
        self.scalar_range.clone()
    }
    #[must_use]
    pub fn column_range(&self) -> Range<u32> {
        self.column_range.clone()
    }
    #[must_use]
    pub const fn map(&self) -> NativeCellMapKind {
        self.map
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeDecorationKind {
    Border,
    Rule,
    Padding,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeDecoration {
    pub(crate) line: NativeFixedLineKey,
    pub(crate) text: String,
    pub(crate) column_range: Range<u32>,
    pub(crate) kind: NativeDecorationKind,
    pub(crate) provenance: ProvenanceKey,
}

impl NativeDecoration {
    #[must_use]
    pub const fn line(&self) -> NativeFixedLineKey {
        self.line
    }
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
    #[must_use]
    pub fn column_range(&self) -> Range<u32> {
        self.column_range.clone()
    }
    #[must_use]
    pub const fn kind(&self) -> NativeDecorationKind {
        self.kind
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}
