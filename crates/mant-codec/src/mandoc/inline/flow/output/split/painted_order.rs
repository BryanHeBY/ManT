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
                    for receipt in &glyph.receipts {
                        output.push(Inline::anchor(format!("{ORDER}{receipt}:{serial}")));
                    }
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

pub(in crate::mandoc::inline::flow) fn is_shared_mark(node: &Inline) -> bool {
    matches!(node, Inline::Anchor { id, .. } if id.as_str().strip_prefix(ORDER).is_some_and(|fields| fields.starts_with('s')))
}

pub(in crate::mandoc::inline::flow) fn discard_shared(nodes: &mut Vec<Inline>) {
    nodes.retain_mut(|node| {
        if is_shared_mark(node) {
            return false;
        }
        if let Some(children) = children(node) {
            discard_shared(children);
        }
        true
    });
}

fn has_marks(nodes: &[Inline], shared: bool) -> bool {
    nodes.iter().any(|node| match node {
        Inline::Anchor { id, .. } => {
            id.as_str().starts_with(ORDER) && is_shared_mark(node) == shared
        }
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => has_marks(children, shared),
        _ => false,
    })
}

pub(in crate::mandoc::inline::flow) fn apply(nodes: &mut Vec<Inline>) {
    if has_marks(nodes, false) {
        apply_partitions(nodes, None);
    }
}

pub(in crate::mandoc::inline::flow) fn apply_shared(
    head: &mut Vec<Inline>,
    body: &mut Vec<Inline>,
) {
    if has_marks(head, true) && has_marks(body, true) {
        apply_partitions(head, Some(body));
    }
}

fn apply_partitions(nodes: &mut Vec<Inline>, mut body: Option<&mut Vec<Inline>>) {
    let mut scopes = Vec::new();
    let mut leaves = Vec::new();
    flatten(
        std::mem::take(nodes),
        &mut scopes,
        &mut Vec::new(),
        &mut leaves,
    );
    let shared = body.is_some();
    if let Some(body) = body.as_deref_mut() {
        let first_body_leaf = leaves.len();
        flatten(
            std::mem::take(body),
            &mut scopes,
            &mut Vec::new(),
            &mut leaves,
        );
        for leaf in &mut leaves[first_body_leaf..] {
            leaf.body = true;
        }
    }
    let mut groups = BTreeMap::<String, Vec<Position>>::new();
    let mut markers = Vec::new();
    let mut row = 0;
    for leaf in std::mem::take(&mut leaves) {
        let Some(node) = &leaf.node else { continue };
        if let Inline::Anchor { id, .. } = node
            && let Some(fields) = id.as_str().strip_prefix(ORDER)
        {
            let shared_fields = fields.strip_prefix('s');
            if shared_fields.is_some() == shared {
                markers.extend(parse_mark(shared_fields.unwrap_or(fields)));
            } else {
                leaves.push(leaf);
            }
            continue;
        }
        let index = leaves.len();
        let single_glyph = matches!(
            node,
            Inline::Text { value } | Inline::Code { value }
                if value.chars().take(2).count() == 1
        );
        if single_glyph {
            leaves.push(leaf);
            for (copy, (group, rank, size, owner)) in markers.drain(..).enumerate() {
                if copy != 0 {
                    // An accepted scalar can paint at multiple device
                    // columns. Republish its original style and scope
                    // only now, after all native owner counts are settled.
                    leaves.push(leaves[index].clone());
                }
                groups.entry(group).or_default().push(Position {
                    index: index + copy,
                    device_copy: copy != 0,
                    row,
                    rank,
                    size,
                    owner,
                });
            }
            continue;
        }
        if !is_transparent(node) {
            markers.clear();
            row += 1;
        }
        leaves.push(leaf);
    }
    let mut moved = vec![false; scopes.len()];
    for group in groups.values_mut() {
        if !reorder(&mut leaves, group, &mut moved) {
            for copy in group.iter().filter(|position| position.device_copy) {
                // Speculative copy leaves cannot outlive the placement
                // proof that would publish their actual device positions.
                leaves[copy.index].node = None;
            }
        }
    }
    rebuild_partitions(leaves, &scopes, &moved, nodes, body);
}

fn rebuild_partitions(
    leaves: Vec<Leaf>,
    scopes: &[Inline],
    moved: &[bool],
    nodes: &mut Vec<Inline>,
    mut body: Option<&mut Vec<Inline>>,
) {
    let mut open = Vec::<(usize, Vec<Inline>)>::new();
    let mut roots = nodes;
    let mut left_head = false;
    for leaf in leaves {
        if leaf.body && !left_head {
            close_scopes(&mut open, scopes, moved, roots, 0);
            roots = body.take().expect("BODY projection destination");
            left_head = true;
        }
        let Some(node) = leaf.node else { continue };
        let shared = open
            .iter()
            .zip(&leaf.path)
            .take_while(|((left, _), right)| left == *right)
            .count();
        close_scopes(&mut open, scopes, moved, roots, shared);
        open.extend(leaf.path[shared..].iter().map(|&id| (id, Vec::new())));
        let target = output(&mut open, roots);
        if let Some((owner, restore)) = leaf.owner_change {
            target.push(Inline::anchor(format!("{INTERNAL_FIELD_WORD}{owner}")));
            target.push(node);
            target.push(Inline::anchor(format!("{INTERNAL_FIELD_WORD}{restore}")));
        } else {
            target.push(node);
        }
    }
    close_scopes(&mut open, scopes, moved, roots, 0);
}

fn is_word_metadata(node: &Inline) -> bool {
    matches!(node, Inline::Anchor { id, .. } if id.as_str().starts_with(INTERNAL_FIELD_WORD)
        || id.as_str().starts_with(INTERNAL_FIELD_PREFIX)
        || id.as_str().starts_with(super::super::NAVIGATION_WORD)
        || is_shared_mark(node)
        // Unprinted deferred-blank anchors and retired placeholders emit zero
        // columns and stay on the same physical row (`term.c:389-436`).
        || id.as_str().starts_with("\0mant:output-scope:deferred-blank:")
        || id.as_str() == "\0mant:output-scope:retired-blank")
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
    device_copy: bool,
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

fn reorder(leaves: &mut [Leaf], group: &mut [Position], moved: &mut [bool]) -> bool {
    let first = &group[0];
    let last = &group[group.len() - 1];
    if group.len() != first.size || first.row != last.row {
        return false;
    }
    let ink = leaves[first.index..=last.index]
        .iter()
        .filter_map(|leaf| leaf.node.as_ref())
        .filter(|node| !is_transparent(node));
    // Unmatched characters between marked glyphs cannot be assigned a
    // device column just because two siblings happen to have receipts.
    if ink.count() != group.len() {
        return false;
    }
    let destinations: Vec<_> = group
        .iter()
        .map(|pos| (pos.index, pos.owner, leaves[pos.index].body))
        .collect();
    group.sort_unstable_by_key(|pos| pos.rank);
    if !group.iter().enumerate().all(|(rank, pos)| rank == pos.rank) {
        return false;
    }
    let payload: Vec<_> = group
        .iter()
        .map(|pos| std::mem::take(&mut leaves[pos.index]))
        .collect();
    for (((index, owner, body), position), mut leaf) in
        destinations.into_iter().zip(group).zip(payload)
    {
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
        leaf.body = body;
        leaves[index] = leaf;
    }
    true
}

#[derive(Clone, Debug, Default)]
struct Leaf {
    node: Option<Inline>,
    path: Vec<usize>,
    owner_change: Option<(u64, u64)>,
    body: bool,
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
                body: false,
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
