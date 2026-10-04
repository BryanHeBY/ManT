//! Direct regression guards for one cell layout, then measurement and placement.

use super::{BlockRenderer, Flow, LayoutText, visits};
use crate::presentation::TextPresentation;
use mant_ir::{Block, Inline, LayoutHint, TableCell, TableCellKind, TableRow, TableRowKind};

mod decorations;
mod fallback_rows;
mod fixtures;
mod row_boundaries;
mod traversal;

use fixtures::{
    ansi, cell, literal, matched_evidence, named_item, nested, paragraph, plain, renderer, table,
    text, undecorated,
};
