//! Resolve executed row receipts at the final content-owner drain.

use std::collections::BTreeMap;

use mant_ir::{InlineLayout, RowLayoutHint, TextAlignment};

use super::row_origins::{CURRENT_ROW, NEXT_ROW, PRESENTATION};
use super::{Inline, is_term_alternative};

#[derive(Default)]
struct RowMetadata {
    origin: i32,
    correction: i32,
    first: i32,
    alignment: TextAlignment,
}

#[derive(Default)]
struct Collector {
    row: u32,
    next_origin: Option<i32>,
    rows: BTreeMap<u32, RowMetadata>,
}

impl Collector {
    fn carrier(&mut self, id: &str) -> bool {
        if let Some(value) = id.strip_prefix(PRESENTATION) {
            let mut values = value.split(':');
            let row = self.rows.entry(self.row).or_default();
            row.alignment = match values.next() {
                Some("1") => TextAlignment::Center,
                Some("2") => TextAlignment::Right,
                _ => TextAlignment::Left,
            };
            row.first = values
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            row.correction = values
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            return true;
        }
        if let Some(value) = id.strip_prefix(NEXT_ROW) {
            self.next_origin = value.parse().ok();
            return true;
        }
        if let Some(value) = id.strip_prefix(CURRENT_ROW) {
            if let Ok(origin) = value.parse() {
                self.rows.entry(self.row).or_default().origin = origin;
            }
            return true;
        }
        false
    }

    fn advance(&mut self) {
        self.row = self.row.saturating_add(1);
        if let Some(origin) = self.next_origin.take() {
            self.rows.entry(self.row).or_default().origin = origin;
        }
        // Each actual accepted row has its own receipt. Neither alignment
        // nor a temporary first-visual offset is inherited from an earlier
        // output row merely because both rows share this content owner.
    }

    fn visit(&mut self, nodes: &mut Vec<Inline>) {
        nodes.retain_mut(|node| {
            if is_term_alternative(node) {
                return false;
            }
            if let Inline::Anchor { id, .. } = node
                && self.carrier(id.as_str())
            {
                return false;
            }
            match node {
                Inline::LineBreak {} => self.advance(),
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    if !value.is_empty() {
                        self.next_origin = None;
                    }
                    for _ in value.bytes().filter(|byte| *byte == b'\n') {
                        self.advance();
                    }
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => self.visit(children),
                Inline::Anchor { .. } => {}
            }
            true
        });
    }
}

pub(super) fn take(nodes: &mut Vec<Inline>) -> InlineLayout {
    let mut collector = Collector::default();
    collector.visit(nodes);
    InlineLayout {
        row_hints: collector
            .rows
            .into_iter()
            .map(|(row, metadata)| RowLayoutHint {
                row,
                indent_columns: metadata.origin.saturating_add(metadata.correction),
                first_line_indent_columns: metadata.first,
                alignment: metadata.alignment,
            })
            .filter(|hint| !hint.is_empty())
            .collect(),
    }
}
