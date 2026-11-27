//! Move a covered first word's target only to surviving authored ink in its macro.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    INTERNAL_FIELD_PREFIX, INTERNAL_FIELD_WORD, Inline, NAVIGATION_FOLLOWUP, NAVIGATION_WORD,
};

type Word = u64;
type Scope = u32;
type Position = (usize, usize);

#[derive(Default)]
struct Receipts {
    first: BTreeMap<Scope, (Word, usize, Inline)>,
    followups: Vec<(Word, Scope)>,
    ordinal: usize,
}

impl Receipts {
    fn observe(&mut self, nodes: &[Inline]) {
        let mut first = None;
        for node in nodes {
            self.ordinal += 1;
            match node {
                Inline::Anchor { id, .. } => {
                    let id = id.as_str();
                    if let Some((word, scope)) = first.take()
                        && !id.starts_with('\0')
                    {
                        self.first.insert(scope, (word, self.ordinal, node.clone()));
                    }
                    first = receipt(id, NAVIGATION_WORD);
                    if let Some(next) = receipt(id, NAVIGATION_FOLLOWUP) {
                        self.followups.push(next);
                    }
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => self.observe(children),
                _ => {}
            }
            if !matches!(node, Inline::Anchor { .. }) {
                first = None;
            }
        }
    }
}

fn receipt(id: &str, prefix: &str) -> Option<(Word, Scope)> {
    let (word, scope) = id.strip_prefix(prefix)?.split_once(':')?;
    Some((word.parse().ok()?, scope.parse().ok()?))
}

fn find_ink(
    nodes: &[Inline],
    tracked: &BTreeSet<Word>,
    ink: &mut BTreeMap<Word, Position>,
    owner: &mut Option<Word>,
    ordinal: &mut usize,
) {
    for node in nodes {
        *ordinal += 1;
        match node {
            Inline::Anchor { id, .. } => {
                let id = id.as_str();
                if let Some(word) = id.strip_prefix(INTERNAL_FIELD_WORD) {
                    *owner = word.parse().ok().filter(|word| tracked.contains(word));
                } else if id.starts_with(INTERNAL_FIELD_PREFIX) {
                    *owner = None;
                }
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => find_ink(children, tracked, ink, owner, ordinal),
            Inline::Text { value } | Inline::Code { value } => {
                if let Some(word) = *owner
                    && let Some(offset) = value.find(|ch| !matches!(ch, ' ' | '\t' | '\n' | '\r'))
                {
                    ink.entry(word).or_insert((*ordinal, offset));
                }
            }
            Inline::Equation { .. } | Inline::LineBreak { .. } => *owner = None,
        }
    }
}

pub(super) fn resolve(nodes: &mut Vec<Inline>) {
    let mut receipts = Receipts::default();
    receipts.observe(nodes);
    if receipts.followups.is_empty() {
        return;
    }
    let tracked = receipts
        .followups
        .iter()
        .map(|&(word, _)| word)
        .chain(receipts.first.values().map(|&(word, _, _)| word))
        .collect();
    let mut ink = BTreeMap::new();
    find_ink(nodes, &tracked, &mut ink, &mut None, &mut 0);
    let mut winners = BTreeMap::<Scope, Position>::new();
    for (word, scope) in receipts.followups {
        if let Some(&(first, _, _)) = receipts.first.get(&scope)
            && !ink.contains_key(&first)
            && let Some(&position) = ink.get(&word)
        {
            let winner = winners.entry(scope).or_insert(position);
            *winner = (*winner).min(position);
        }
    }
    if winners.is_empty() {
        // No scoped ink survives, or the normal first-word placement is
        // already right. In either case keep its explicit original boundary.
        return;
    }
    let mut edits = BTreeMap::<usize, Edit>::new();
    for (scope, (ordinal, offset)) in winners {
        let (_, original, anchor) = receipts
            .first
            .remove(&scope)
            .expect("known navigation scope");
        edits.entry(original).or_default().remove = true;
        let edit = edits.entry(ordinal).or_default();
        edit.offset = offset;
        edit.anchors.push(anchor);
    }
    apply(nodes, &mut 0, &mut edits);
}

#[derive(Default)]
struct Edit {
    remove: bool,
    offset: usize,
    anchors: Vec<Inline>,
}

fn apply(nodes: &mut Vec<Inline>, ordinal: &mut usize, edits: &mut BTreeMap<usize, Edit>) {
    let mut output = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        *ordinal += 1;
        let edit = edits.remove(ordinal);
        match &mut node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => apply(children, ordinal, edits),
            _ => {}
        }
        if let Some(edit) = edit {
            if edit.remove {
                continue;
            }
            if edit.offset != 0 {
                let prefix = match &mut node {
                    Inline::Text { value } => Inline::Text {
                        value: split_prefix(value, edit.offset),
                    },
                    Inline::Code { value } => Inline::Code {
                        value: split_prefix(value, edit.offset),
                    },
                    _ => unreachable!("navigation ink is text"),
                };
                output.push(prefix);
            }
            output.extend(edit.anchors);
        }
        output.push(node);
    }
    *nodes = output;
}

fn split_prefix(value: &mut String, offset: usize) -> String {
    let suffix = value.split_off(offset);
    std::mem::replace(value, suffix)
}
