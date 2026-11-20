//! Publish the surviving owners of a proved device overprint in column order.

use std::collections::BTreeMap;

use super::super::{
    INTERNAL_FIELD_PREFIX, INTERNAL_FIELD_WORD, Inline,
    native_passes::{OrderedGlyph, OwnerAcceptance},
};

const ORDER: &str = "\0mant:output-scope:paint-order:";

pub(super) fn mark_survivors(
    nodes: &mut Vec<Inline>,
    limits: &BTreeMap<String, OwnerAcceptance>,
    covered_only: bool,
) {
    let positions: BTreeMap<_, _> = limits
        .iter()
        .filter(|(_, accepted)| !accepted.ordered_glyphs.is_empty())
        .map(|(owner, accepted)| {
            let positions: BTreeMap<_, _> = accepted
                .ordered_glyphs
                .iter()
                .filter_map(|glyph| {
                    let mut scalar = glyph.scalar;
                    for omit in &accepted.omitted_scalars {
                        if covered_only && !omit.covered {
                            continue;
                        }
                        if omit.range.contains(&glyph.scalar) {
                            return None;
                        }
                        if omit.range.end <= glyph.scalar {
                            scalar -= omit.range.len();
                        }
                    }
                    Some((scalar, glyph))
                })
                .collect();
            (owner.as_str(), positions)
        })
        .collect();
    if !positions.is_empty() {
        MarkerCursor {
            positions,
            ..MarkerCursor::default()
        }
        .mark(nodes);
    }
}

#[derive(Default)]
struct MarkerCursor<'a> {
    positions: BTreeMap<&'a str, BTreeMap<usize, &'a OrderedGlyph>>,
    seen: BTreeMap<&'a str, usize>,
    owner: Option<&'a str>,
}

impl MarkerCursor<'_> {
    fn mark(&mut self, nodes: &mut Vec<Inline>) {
        let mut marked = Vec::with_capacity(nodes.len());
        for mut node in std::mem::take(nodes) {
            match &mut node {
                Inline::Anchor { id, .. } => {
                    if id.as_str().starts_with(INTERNAL_FIELD_WORD) {
                        self.owner = self
                            .positions
                            .get_key_value(id.as_str())
                            .map(|(&key, _)| key);
                    } else if id.as_str().starts_with(INTERNAL_FIELD_PREFIX) {
                        self.owner = None;
                    }
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => self.mark(children),
                Inline::Text { .. } | Inline::Code { .. } if self.owner.is_some() => {
                    self.mark_text(node, &mut marked);
                    continue;
                }
                Inline::Equation { value, .. } => {
                    if let Some(owner) = self.owner {
                        *self.seen.entry(owner).or_default() += value.chars().count();
                    }
                }
                _ => (),
            }
            marked.push(node);
        }
        *nodes = marked;
    }

    fn mark_text(&mut self, node: Inline, output: &mut Vec<Inline>) {
        let (value, wrap): (_, fn(String) -> Inline) = match node {
            Inline::Text { value } => (value, |value| Inline::Text { value }),
            Inline::Code { value } => (value, |value| Inline::Code { value }),
            _ => unreachable!(),
        };
        let owner = self.owner.expect("tracked formatter owner");
        let offset = self.seen.entry(owner).or_default();
        let end = *offset + value.chars().count();
        let mut positions = self.positions[owner].range(*offset..end).peekable();
        if positions.peek().is_none() {
            output.push(wrap(value));
        } else {
            let mut start = 0;
            for (index, (byte, character)) in value.char_indices().enumerate() {
                if let Some((_, glyph)) =
                    positions.next_if(|(scalar, _)| **scalar == *offset + index)
                    && character == glyph.character
                {
                    if start < byte {
                        output.push(wrap(value[start..byte].into()));
                    }
                    let serial = owner
                        .strip_prefix(INTERNAL_FIELD_WORD)
                        .expect("word marker");
                    output.push(Inline::anchor(format!("{ORDER}{}:{serial}", glyph.receipt)));
                    output.push(wrap(character.into()));
                    start = byte + character.len_utf8();
                }
            }
            if start < value.len() {
                output.push(wrap(value[start..].into()));
            }
        }
        *offset = end;
    }
}

fn has_marks(nodes: &[Inline]) -> bool {
    nodes.iter().any(|node| match node {
        Inline::Anchor { id, .. } => id.as_str().starts_with(ORDER),
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => has_marks(children),
        _ => false,
    })
}

pub(in crate::mandoc::inline::flow) fn apply(nodes: &mut Vec<Inline>) {
    if !has_marks(nodes) {
        return;
    }
    let mut scopes = Vec::new();
    let mut leaves = Vec::new();
    flatten(
        std::mem::take(nodes),
        &mut scopes,
        &mut Vec::new(),
        &mut leaves,
    );
    let mut groups = BTreeMap::<String, Vec<Position>>::new();
    let mut marker = None;
    let mut row = 0;
    for (index, leaf) in leaves.iter_mut().enumerate() {
        let Some(node) = &leaf.node else { continue };
        if let Inline::Anchor { id, .. } = node
            && let Some(fields) = id.as_str().strip_prefix(ORDER)
        {
            marker = parse_mark(fields);
            leaf.node = None;
        } else if matches!(node, Inline::Text { value } | Inline::Code { value } if value.len() == 1)
        {
            if let Some((group, rank, size, owner)) = marker.take() {
                groups.entry(group).or_default().push(Position {
                    index,
                    row,
                    rank,
                    size,
                    owner,
                });
            }
        } else if !is_transparent(node) {
            marker = None;
            row += 1;
        }
    }
    let mut moved = vec![false; scopes.len()];
    for group in groups.values_mut() {
        reorder(&mut leaves, group, &mut moved);
    }
    let mut open = Vec::<(usize, Vec<Inline>)>::new();
    for leaf in leaves {
        let Some(node) = leaf.node else { continue };
        let shared = open
            .iter()
            .zip(&leaf.path)
            .take_while(|((left, _), right)| left == *right)
            .count();
        close_scopes(&mut open, &scopes, &moved, nodes, shared);
        open.extend(leaf.path[shared..].iter().map(|&id| (id, Vec::new())));
        let target = output(&mut open, nodes);
        if let Some((owner, restore)) = leaf.owner_change {
            target.push(Inline::anchor(format!("{INTERNAL_FIELD_WORD}{owner}")));
            target.push(node);
            target.push(Inline::anchor(format!("{INTERNAL_FIELD_WORD}{restore}")));
        } else {
            target.push(node);
        }
    }
    close_scopes(&mut open, &scopes, &moved, nodes, 0);
}

fn is_word_metadata(node: &Inline) -> bool {
    matches!(node, Inline::Anchor { id, .. } if id.as_str().starts_with(INTERNAL_FIELD_WORD)
        || id.as_str().starts_with(INTERNAL_FIELD_PREFIX))
}

fn is_transparent(node: &Inline) -> bool {
    match node {
        // Keep anchors at their existing boundaries when glyphs change
        // places. A layout carrier or private scope boundary is different.
        Inline::Anchor { id, .. } => !id.as_str().starts_with('\0') || is_word_metadata(node),
        Inline::Text { value } | Inline::Code { value } => value.is_empty(),
        _ => false,
    }
}

#[derive(Debug)]
struct Position {
    index: usize,
    row: usize,
    rank: usize,
    size: usize,
    owner: u64,
}

fn parse_mark(mark: &str) -> Option<(String, usize, usize, u64)> {
    let (group, fields) = mark.split_once(':')?;
    let (serial, values) = fields.split_once(':')?;
    let mut values = values.split(':');
    let rank = values.next()?.parse().ok()?;
    let size = values.next()?.parse().ok()?;
    let owner = values.next()?.parse().ok()?;
    Some((format!("{group}:{serial}"), rank, size, owner))
}

fn reorder(leaves: &mut [Leaf], group: &mut [Position], moved: &mut [bool]) {
    let first = &group[0];
    let last = &group[group.len() - 1];
    if group.len() != first.size || first.row != last.row {
        return;
    }
    let ink = leaves[first.index..=last.index]
        .iter()
        .filter_map(|leaf| leaf.node.as_ref())
        .filter(|node| !is_transparent(node));
    // Unmatched characters between marked glyphs cannot be assigned a
    // device column just because two siblings happen to have receipts.
    if ink.count() != group.len() {
        return;
    }
    let destinations: Vec<_> = group.iter().map(|pos| (pos.index, pos.owner)).collect();
    group.sort_unstable_by_key(|pos| pos.rank);
    if !group.iter().enumerate().all(|(rank, pos)| rank == pos.rank) {
        return;
    }
    let payload: Vec<_> = group
        .iter()
        .map(|pos| std::mem::take(&mut leaves[pos.index]))
        .collect();
    for (((index, owner), position), mut leaf) in destinations.into_iter().zip(group).zip(payload) {
        if index != position.index {
            for &scope in &leaf.path {
                moved[scope] = true;
            }
        }
        if owner != position.owner {
            // Head verification runs before private metadata is stripped.
            // Associate the moved glyph with its own source word there too.
            leaf.owner_change = Some((position.owner, owner));
        }
        leaves[index] = leaf;
    }
}

#[derive(Debug, Default)]
struct Leaf {
    node: Option<Inline>,
    path: Vec<usize>,
    owner_change: Option<(u64, u64)>,
}

fn children(node: &mut Inline) -> Option<&mut Vec<Inline>> {
    match node {
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => Some(children),
        _ => None,
    }
}

fn flatten(
    nodes: Vec<Inline>,
    scopes: &mut Vec<Inline>,
    path: &mut Vec<usize>,
    leaves: &mut Vec<Leaf>,
) {
    for mut node in nodes {
        if let Some(children) = children(&mut node).filter(|children| !children.is_empty()) {
            let contents = std::mem::take(children);
            path.push(scopes.len());
            scopes.push(node);
            flatten(contents, scopes, path, leaves);
            path.pop();
        } else {
            leaves.push(Leaf {
                node: Some(node),
                path: path.clone(),
                owner_change: None,
            });
        }
    }
}

fn output<'a>(
    open: &'a mut [(usize, Vec<Inline>)],
    roots: &'a mut Vec<Inline>,
) -> &'a mut Vec<Inline> {
    open.last_mut().map_or(roots, |(_, children)| children)
}

fn close_scopes(
    open: &mut Vec<(usize, Vec<Inline>)>,
    scopes: &[Inline],
    moved: &[bool],
    roots: &mut Vec<Inline>,
    shared: usize,
) {
    while open.len() > shared {
        let (scope, contents) = open.pop().expect("open inline wrapper");
        let target = output(open, roots);
        if moved[scope] && contents.iter().all(is_transparent) {
            // The scope's actual ink went with the author. Keep private
            // markers on the timeline without manufacturing an empty link.
            target.extend(contents);
        } else {
            let mut node = scopes[scope].clone();
            *children(&mut node).expect("inline wrapper") = contents;
            target.push(node);
        }
    }
}

#[cfg(test)]
mod tests;
