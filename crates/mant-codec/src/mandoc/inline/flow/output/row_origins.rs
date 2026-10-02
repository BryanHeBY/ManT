//! Project accepted device-row origins at stable source-word scalar positions.

use std::collections::BTreeMap;

use super::{INTERNAL_FIELD_PREFIX, INTERNAL_FIELD_WORD, INTERNAL_ROW_ORIGIN, Inline};

pub(in crate::mandoc::inline::flow) fn project_native_positions(
    nodes: &mut Vec<Inline>,
    origins: &[(String, usize, usize, bool)],
    padding: &[(String, usize, usize, bool)],
    output_start: usize,
    materialize_line_origins: bool,
) {
    if origins.is_empty() && padding.is_empty() {
        return;
    }
    let mut positions = BTreeMap::<String, Vec<(usize, NativePosition, bool)>>::new();
    for (owner, scalar, origin, hidden_graph) in origins {
        positions.entry(owner.clone()).or_default().push((
            *scalar,
            NativePosition::RowOrigin(*origin),
            *hidden_graph,
        ));
    }
    for (owner, scalar, cells, hidden_graph) in padding {
        positions.entry(owner.clone()).or_default().push((
            *scalar,
            NativePosition::FieldPadding(*cells),
            *hidden_graph,
        ));
    }
    for values in positions.values_mut() {
        values.sort_by_key(|(scalar, _, _)| *scalar);
    }
    let mut cursor = OriginCursor {
        positions: &positions,
        owner: None,
        scalar: 0,
        next: 0,
        previous: BTreeMap::new(),
    };
    // Earlier fields are already committed. The captured current-owner
    // start predates its retirement; only that suffix is projected, using
    // stable words/scalars rather than indices for the receipt positions.
    let pending = nodes.split_off(output_start.min(nodes.len()));
    let mut projected = cursor.project(pending);
    if materialize_line_origins {
        let mut origin = None;
        materialize_origins(&mut projected, &mut origin);
        if let Some(origin) = origin
            && apply_tail_origin(nodes, origin).is_none()
            && origin > 0
        {
            // The first represented HEAD row can start at a nonzero
            // restored native offset without a prior row delimiter.
            // Its accepted print owns this positioning; an empty
            // earlier owner cannot represent it through LineBreak.
            projected.insert(
                0,
                Inline::Text {
                    value: " ".repeat(usize::from(origin)),
                },
            );
        }
    }
    nodes.extend(projected);
}

fn apply_tail_origin(nodes: &mut [Inline], origin: u16) -> Option<bool> {
    for node in nodes.iter_mut().rev() {
        match node {
            Inline::LineBreak { indent_columns } => {
                *indent_columns = origin;
                return Some(true);
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                if let Some(applied) = apply_tail_origin(children, origin) {
                    return Some(applied);
                }
            }
            Inline::Anchor { .. } => {}
            _ => return Some(false),
        }
    }
    None
}

/// Origins sit immediately before their accepted graph. Walking backwards
/// reaches that graph's actual hard row boundary, even through annotations.
/// A first-row origin has no preceding hard boundary; its pending receipt is
/// handled by the owner projection after this boundary search completes.
fn materialize_origins(nodes: &mut Vec<Inline>, origin: &mut Option<u16>) {
    for node in nodes.iter_mut().rev() {
        if let Some(columns) = super::native_row_origin(node) {
            *origin = Some(u16::try_from(columns).unwrap_or(u16::MAX));
            continue;
        }
        match node {
            Inline::LineBreak { indent_columns } => {
                if let Some(columns) = origin.take() {
                    *indent_columns = columns;
                }
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => materialize_origins(children, origin),
            Inline::Text { value } | Inline::Code { value } if !value.is_empty() => *origin = None,
            _ => {}
        }
    }
    nodes.retain(|node| super::native_row_origin(node).is_none());
}

enum NativePosition {
    RowOrigin(usize),
    FieldPadding(usize),
}

struct OriginCursor<'a> {
    positions: &'a BTreeMap<String, Vec<(usize, NativePosition, bool)>>,
    owner: Option<String>,
    scalar: usize,
    next: usize,
    previous: BTreeMap<String, (usize, usize)>,
}

impl OriginCursor<'_> {
    fn project(&mut self, nodes: Vec<Inline>) -> Vec<Inline> {
        let mut output = Vec::with_capacity(nodes.len());
        for mut node in nodes {
            if let Inline::Anchor { id, .. } = &node
                && id.as_str().starts_with(INTERNAL_FIELD_PREFIX)
            {
                // Position the actually written field prefix at its accepted
                // word's origin. Do not advance the content owner's scalar
                // cursor: these cells are device padding, not input glyphs.
                if let Some(positions) = self.positions.get(id.as_str()) {
                    for (scalar, position, _) in positions {
                        if *scalar == 0
                            && let NativePosition::RowOrigin(origin) = position
                        {
                            output.push(Inline::anchor(format!("{INTERNAL_ROW_ORIGIN}{origin}")));
                        }
                    }
                }
                output.push(node);
                continue;
            }
            if let Inline::Anchor { id, .. } = &node
                && id.as_str().starts_with(INTERNAL_FIELD_WORD)
            {
                // encode1() can write an accepted graph that the next word
                // overstrikes before it acquires an IR scalar (term.c:901-
                // 927). Its term_field() padding nevertheless printed. The
                // receipt owns that positioning even for an empty projected
                // owner; discharge it before advancing to the replacement.
                if self.has_unprojected_graph_here() {
                    self.emit_here(&mut output);
                }
                if let Some(owner) = self.owner.take() {
                    #[cfg(test)]
                    OWNER_HISTORY_STORES.with(|stores| stores.set(stores.get().saturating_add(1)));
                    self.previous.insert(owner, (self.scalar, self.next));
                }
                // Positions are complete and immutable for this receipt.
                // An unselected word can never need a cursor on re-entry;
                // preserve its IR without rebuilding glyph strings.
                self.owner = self
                    .positions
                    .contains_key(id.as_str())
                    .then(|| id.as_str().to_owned());
                (self.scalar, self.next) = self
                    .owner
                    .as_ref()
                    .and_then(|owner| self.previous.get(owner))
                    .copied()
                    .unwrap_or_default();
                output.push(node);
                continue;
            }
            match &mut node {
                Inline::Text { value } | Inline::Code { value }
                    if !value.is_empty() && self.owner.is_some() =>
                {
                    self.project_text(&node, &mut output);
                    continue;
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => {
                    *children = self.project(std::mem::take(children));
                }
                _ => {}
            }
            output.push(node);
        }
        output
    }

    fn has_unprojected_graph_here(&self) -> bool {
        self.owner
            .as_ref()
            .and_then(|owner| self.positions.get(owner))
            .is_some_and(|positions| {
                positions[self.next..]
                    .iter()
                    .take_while(|(scalar, _, _)| *scalar == self.scalar)
                    .any(|(_, _, hidden_graph)| *hidden_graph)
            })
    }

    fn emit_here(&mut self, output: &mut Vec<Inline>) {
        let Some(positions) = self
            .owner
            .as_ref()
            .and_then(|owner| self.positions.get(owner))
        else {
            return;
        };
        while let Some((scalar, position, _)) = positions.get(self.next) {
            if *scalar != self.scalar {
                break;
            }
            output.push(match position {
                NativePosition::RowOrigin(origin) => {
                    Inline::anchor(format!("{INTERNAL_ROW_ORIGIN}{origin}"))
                }
                NativePosition::FieldPadding(cells) => Inline::Text {
                    value: " ".repeat(*cells),
                },
            });
            self.next += 1;
        }
    }

    fn project_text(&mut self, node: &Inline, output: &mut Vec<Inline>) {
        let (Inline::Text { value } | Inline::Code { value }) = node else {
            unreachable!("only word glyph projections advance the scalar cursor");
        };
        #[cfg(test)]
        OWNER_TEXT_REBUILDS.with(|texts| texts.set(texts.get().saturating_add(1)));
        let mut piece = String::new();
        for character in value.chars() {
            #[cfg(test)]
            OWNER_CHARS_PROJECTED.with(|chars| chars.set(chars.get().saturating_add(1)));
            if self
                .owner
                .as_ref()
                .and_then(|owner| self.positions.get(owner))
                .and_then(|positions| positions.get(self.next))
                .is_some_and(|(scalar, _, _)| *scalar == self.scalar)
            {
                append_piece(node, &mut piece, output);
                self.emit_here(output);
            }
            piece.push(character);
            self.scalar += 1;
        }
        append_piece(node, &mut piece, output);
    }
}

fn append_piece(node: &Inline, piece: &mut String, output: &mut Vec<Inline>) {
    if piece.is_empty() {
        return;
    }
    let value = std::mem::take(piece);
    output.push(match node {
        Inline::Text { .. } => Inline::Text { value },
        Inline::Code { .. } => Inline::Code { value },
        _ => unreachable!("only Text and Code have scalar pieces"),
    });
}

#[cfg(test)]
std::thread_local! {
    static OWNER_HISTORY_STORES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNER_TEXT_REBUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNER_CHARS_PROJECTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[path = "row_origins/tests.rs"]
mod tests;
