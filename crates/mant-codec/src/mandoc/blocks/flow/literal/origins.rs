//! Accepted physical-row origins become owner-local row layout, not blocks.

use mant_ir::{Block, Inline, LayoutHint, RowLayoutHint, SourceSpan};
use std::collections::BTreeMap;

pub(super) fn literal_blocks(
    mut nodes: Vec<Inline>,
    layout: LayoutHint,
    source: Option<SourceSpan>,
) -> Vec<Block> {
    let mut rows = Origins::default();
    rows.visit(&mut nodes);
    crate::mandoc::inline::strip_native_projection_markers(&mut nodes);
    let mut inline_layout = crate::mandoc::inline::take_inline_layout(&mut nodes);
    if nodes.is_empty() {
        return Vec::new();
    }
    let mut hints: BTreeMap<_, _> = inline_layout
        .row_hints
        .drain(..)
        .map(|hint| (hint.row, hint))
        .collect();
    for (row, origin) in rows.hints {
        let hint = hints.entry(row).or_insert_with(|| RowLayoutHint {
            row,
            ..RowLayoutHint::default()
        });
        hint.indent_columns = hint.indent_columns.saturating_add(origin);
    }
    inline_layout.row_hints = hints
        .into_values()
        .filter(|hint| !hint.is_empty())
        .collect();
    vec![Block::Preformatted {
        inline_layout,
        children: nodes,
        language: None,
        layout,
        source,
    }]
}

#[derive(Default)]
struct Origins {
    row: u32,
    origin: i32,
    occupied: bool,
    hints: BTreeMap<u32, i32>,
}

impl Origins {
    fn advance(&mut self) {
        self.row = self.row.saturating_add(1);
        self.occupied = false;
        if self.origin != 0 {
            self.hints.insert(self.row, self.origin);
        }
    }

    fn visit(&mut self, nodes: &mut Vec<Inline>) {
        let mut output = Vec::with_capacity(nodes.len());
        for mut node in std::mem::take(nodes) {
            if let Some(origin) = crate::mandoc::inline::native_row_origin(&node) {
                // term_field() can accept a later native physical row without
                // another authored newline. Its changed accepted origin owns
                // that delimiter exactly once, inside the original annotation.
                if origin != self.origin && self.occupied {
                    output.push(Inline::line_break());
                    self.advance();
                }
                self.origin = origin;
                self.hints.insert(self.row, origin);
                continue;
            }
            match &mut node {
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => self.visit(children),
                Inline::LineBreak {} => self.advance(),
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    self.occupied = true;
                    for _ in value.bytes().filter(|byte| *byte == b'\n') {
                        self.advance();
                    }
                    if !value.ends_with('\n') {
                        self.occupied = true;
                    }
                }
                Inline::Anchor { .. } => {}
            }
            output.push(node);
        }
        *nodes = output;
    }
}
