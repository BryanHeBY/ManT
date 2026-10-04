//! Project accepted device-row origins at stable source-word scalar positions.

use std::collections::BTreeMap;

use super::{INTERNAL_FIELD_PREFIX, INTERNAL_FIELD_WORD, INTERNAL_ROW_ORIGIN, Inline};

const NEXT_ROW: &str = "\0mant:row-layout:next:";
const CURRENT_ROW: &str = "\0mant:row-layout:current:";

/// A mutation of the active top-level output vector. Native cell/scalar
/// receipts have a different coordinate space and must never use this map.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc::inline::flow) struct OutputNodeEdit {
    pub(super) start: usize,
    pub(super) removed: usize,
    pub(super) inserted: usize,
}

impl OutputNodeEdit {
    pub(in crate::mandoc::inline::flow) fn remap(self, position: &mut usize) {
        if *position >= self.start.saturating_add(self.removed) {
            *position = position
                .saturating_sub(self.removed)
                .saturating_add(self.inserted);
        } else if *position >= self.start {
            *position = self.start.saturating_add(self.inserted);
        }
    }
}

/// Update the carrier owned by this delimiter, rather than inserting a
/// second receipt. An implicit zero origin needs no new node; replacing a
/// previous nonzero receipt with zero still retires that previous origin.
fn set_break_origin(nodes: &mut Vec<Inline>, index: usize, origin: u16) -> Option<OutputNodeEdit> {
    for previous in (0..index).rev() {
        let Inline::Anchor { id, .. } = &nodes[previous] else {
            break;
        };
        if id.as_str().starts_with(NEXT_ROW) {
            nodes[previous] = Inline::anchor(format!("{NEXT_ROW}{origin}"));
            return None;
        }
    }
    if origin == 0 {
        return None;
    }
    nodes.insert(index, Inline::anchor(format!("{NEXT_ROW}{origin}")));
    Some(OutputNodeEdit {
        start: index,
        removed: 0,
        inserted: 1,
    })
}

/// Only a root insertion changes saved root-vector addresses. A carrier
/// inside a transparent annotation changes that annotation's children only.
pub(in crate::mandoc::inline::flow) fn set_last_break_origin(
    nodes: &mut Vec<Inline>,
    origin: u16,
) -> (bool, Option<OutputNodeEdit>) {
    for index in (0..nodes.len()).rev() {
        match &mut nodes[index] {
            Inline::LineBreak {} => return (true, set_break_origin(nodes, index, origin)),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                if set_last_break_origin(children, origin).0 {
                    return (true, None);
                }
            }
            _ => {}
        }
    }
    (false, None)
}

pub(in crate::mandoc::inline::flow) fn is_layout_carrier(node: &Inline) -> bool {
    matches!(node, Inline::Anchor { id, .. }
        if id.as_str().starts_with(NEXT_ROW) || id.as_str().starts_with(CURRENT_ROW))
}

/// Keep layout next to its executed delimiter until the final owner drains.
/// These temporary anchors carry no glyphs and never enter public IR.
pub(in crate::mandoc) fn push_row_break(nodes: &mut Vec<Inline>, origin: u16) {
    if origin != 0 {
        set_next_origin(nodes, origin);
    }
    nodes.push(Inline::line_break());
}

pub(in crate::mandoc::inline::flow) fn set_next_origin(nodes: &mut Vec<Inline>, origin: u16) {
    let marker = Inline::anchor(format!("{NEXT_ROW}{origin}"));
    if nodes.last().is_some_and(
        |node| matches!(node, Inline::Anchor { id, .. } if id.as_str().starts_with(NEXT_ROW)),
    ) {
        *nodes.last_mut().expect("checked tail") = marker;
    } else {
        nodes.push(marker);
    }
}

/// Rebase a removed alternative delimiter onto the new label's first row.
pub(in crate::mandoc) fn split_row_origin(nodes: &mut Vec<Inline>) -> Option<Inline> {
    let index = nodes.iter().rposition(
        |node| matches!(node, Inline::Anchor { id, .. } if id.as_str().starts_with(NEXT_ROW)),
    )?;
    if nodes[index + 1..]
        .iter()
        .any(|node| !matches!(node, Inline::Anchor { .. }))
    {
        return None;
    }
    let Inline::Anchor { id, .. } = nodes.remove(index) else {
        unreachable!()
    };
    Some(Inline::anchor(format!(
        "{CURRENT_ROW}{}",
        id.as_str().strip_prefix(NEXT_ROW)?
    )))
}

/// Resolve alternative boundary addresses only after native retirement/drain.
pub(in crate::mandoc) fn take_definition_term_breaks(nodes: &mut Vec<Inline>) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut index = 0;
    let mut previous_break = None;
    nodes.retain(|node| {
        if super::is_term_alternative(node) {
            if let Some(index) = previous_break.take() {
                positions.push(index);
            }
            return false;
        }
        if !matches!(node, Inline::Anchor { .. }) {
            previous_break = matches!(node, Inline::LineBreak {}).then_some(index);
        }
        index += 1;
        true
    });
    positions
}

pub(in crate::mandoc) fn take_inline_layout(nodes: &mut Vec<Inline>) -> mant_ir::InlineLayout {
    fn take(
        nodes: &mut Vec<Inline>,
        row: &mut u32,
        pending: &mut Option<i32>,
        hints: &mut BTreeMap<u32, i32>,
    ) {
        nodes.retain_mut(|node| {
            if super::is_term_alternative(node) {
                return false;
            }
            if let Inline::Anchor { id, .. } = node {
                if let Some(value) = id.as_str().strip_prefix(NEXT_ROW) {
                    *pending = value.parse().ok();
                    return false;
                }
                if let Some(value) = id.as_str().strip_prefix(CURRENT_ROW) {
                    if let Ok(value) = value.parse() {
                        hints.insert(*row, value);
                    }
                    return false;
                }
            }
            match node {
                Inline::LineBreak {} => {
                    *row = row.saturating_add(1);
                    if let Some(value) = pending.take() {
                        hints.insert(*row, value);
                    }
                }
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    if !value.is_empty() {
                        *pending = None;
                    }
                    *row = row.saturating_add(
                        u32::try_from(value.bytes().filter(|c| *c == b'\n').count())
                            .unwrap_or(u32::MAX),
                    );
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => take(children, row, pending, hints),
                Inline::Anchor { .. } => {}
            }
            true
        });
    }
    let mut hints = BTreeMap::new();
    take(nodes, &mut 0, &mut None, &mut hints);
    mant_ir::InlineLayout {
        row_hints: hints
            .into_iter()
            .filter(|(_, indent)| *indent != 0)
            .map(|(row, indent_columns)| mant_ir::RowLayoutHint {
                row,
                indent_columns,
            })
            .collect(),
    }
}

pub(in crate::mandoc::inline::flow) fn project_native_positions(
    nodes: &mut Vec<Inline>,
    origins: &[(String, usize, usize, bool)],
    padding: &[(String, usize, usize, bool)],
    output_start: usize,
    materialize_line_origins: bool,
) -> Option<OutputNodeEdit> {
    if origins.is_empty() && padding.is_empty() {
        return None;
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
    let mut edit = None;
    if materialize_line_origins {
        let mut origin = None;
        materialize_origins(&mut projected, &mut origin);
        if let Some(origin) = origin {
            let (applied, insertion) = apply_tail_origin(nodes, origin);
            edit = insertion;
            if applied.is_none() && origin > 0 {
                // The first represented HEAD row can start at a nonzero
                // restored native offset without a prior row delimiter.
                // Its accepted print owns this positioning; an empty
                // earlier owner cannot represent it through LineBreak.
                projected.insert(0, Inline::anchor(format!("{CURRENT_ROW}{origin}")));
            }
        }
    }
    nodes.extend(projected);
    edit
}

fn apply_tail_origin(
    nodes: &mut Vec<Inline>,
    origin: u16,
) -> (Option<bool>, Option<OutputNodeEdit>) {
    for index in (0..nodes.len()).rev() {
        match &mut nodes[index] {
            Inline::LineBreak {} => {
                return (Some(true), set_break_origin(nodes, index, origin));
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                if let (Some(applied), _) = apply_tail_origin(children, origin) {
                    return (Some(applied), None);
                }
            }
            Inline::Anchor { .. } => {}
            _ => return (Some(false), None),
        }
    }
    (None, None)
}

/// Origins sit immediately before their accepted graph. Walking backwards
/// reaches that graph's actual hard row boundary, even through annotations.
/// A first-row origin has no preceding hard boundary; its pending receipt is
/// handled by the owner projection after this boundary search completes.
fn materialize_origins(nodes: &mut Vec<Inline>, origin: &mut Option<u16>) {
    let mut reversed = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes).into_iter().rev() {
        if let Some(columns) = super::native_row_origin(&node) {
            *origin = Some(u16::try_from(columns).unwrap_or(u16::MAX));
            continue;
        }
        match &mut node {
            Inline::LineBreak {} => {
                if let Some(columns) = origin.take() {
                    reversed.push(node);
                    reversed.push(Inline::anchor(format!("{NEXT_ROW}{columns}")));
                    continue;
                }
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => materialize_origins(children, origin),
            Inline::Text { value } | Inline::Code { value } if !value.is_empty() => *origin = None,
            _ => {}
        }
        reversed.push(node);
    }
    reversed.reverse();
    *nodes = reversed;
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
