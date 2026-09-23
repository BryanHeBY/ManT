//! Native table identities and ownership over the one logical content body.

use super::{
    ContentPointKey, NativeBlockKey, NativeFixedViewKey, NativeTableCellKey, NativeTableKey,
    NativeTableRowKey, OwnerKey, ProvenanceKey,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeTable {
    pub(crate) key: NativeTableKey,
    pub(crate) block: NativeBlockKey,
    pub(crate) fixed_view: Option<NativeFixedViewKey>,
    pub(crate) provenance: ProvenanceKey,
}

impl NativeTable {
    #[must_use]
    pub const fn key(&self) -> NativeTableKey {
        self.key
    }
    #[must_use]
    pub const fn block(&self) -> NativeBlockKey {
        self.block
    }
    #[must_use]
    pub const fn fixed_view(&self) -> Option<NativeFixedViewKey> {
        self.fixed_view
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeTableRowKind {
    Data,
    HorizontalRule,
    DoubleHorizontalRule,
    LayoutRule,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeTableRow {
    pub(crate) key: NativeTableRowKey,
    pub(crate) table: NativeTableKey,
    pub(crate) ordinal: u32,
    pub(crate) kind: NativeTableRowKind,
    pub(crate) point: Option<ContentPointKey>,
    pub(crate) provenance: ProvenanceKey,
}

impl NativeTableRow {
    #[must_use]
    pub const fn key(&self) -> NativeTableRowKey {
        self.key
    }
    #[must_use]
    pub const fn table(&self) -> NativeTableKey {
        self.table
    }
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }
    #[must_use]
    pub const fn kind(&self) -> NativeTableRowKind {
        self.kind
    }
    #[must_use]
    pub const fn point(&self) -> Option<ContentPointKey> {
        self.point
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeTableCellKind {
    Text,
    HorizontalRule,
    DoubleHorizontalRule,
    IsolatedHorizontalRule,
    IsolatedDoubleHorizontalRule,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeTableAlignment {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeTableCell {
    pub(crate) key: NativeTableCellKey,
    pub(crate) row: NativeTableRowKey,
    pub(crate) column: u32,
    pub(crate) owner: OwnerKey,
    pub(crate) kind: NativeTableCellKind,
    pub(crate) alignment: NativeTableAlignment,
    pub(crate) row_span: u32,
    pub(crate) column_span: u32,
    pub(crate) point: Option<ContentPointKey>,
    pub(crate) provenance: ProvenanceKey,
}

impl NativeTableCell {
    #[must_use]
    pub const fn key(&self) -> NativeTableCellKey {
        self.key
    }
    #[must_use]
    pub const fn row(&self) -> NativeTableRowKey {
        self.row
    }
    #[must_use]
    pub const fn column(&self) -> u32 {
        self.column
    }
    #[must_use]
    pub const fn owner(&self) -> OwnerKey {
        self.owner
    }
    #[must_use]
    pub const fn kind(&self) -> NativeTableCellKind {
        self.kind
    }
    #[must_use]
    pub const fn alignment(&self) -> NativeTableAlignment {
        self.alignment
    }
    #[must_use]
    pub const fn row_span(&self) -> u32 {
        self.row_span
    }
    #[must_use]
    pub const fn column_span(&self) -> u32 {
        self.column_span
    }
    #[must_use]
    pub const fn point(&self) -> Option<ContentPointKey> {
        self.point
    }
    #[must_use]
    pub const fn provenance(&self) -> ProvenanceKey {
        self.provenance
    }
}
