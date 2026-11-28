//! Recover a completely surviving relocated definition prefix, never BODY ink.

use super::super::super::output::INTERNAL_FIELD_PREFIX;
use super::INTERNAL_FIELD_WORD;
use crate::mandoc::inline::ownership::{annotations::AnnotationKey, markers::target_owner};
use mant_ir::{
    Block, DefinitionItem, Inline,
    visit::{self, VisitMut},
};
use std::collections::{BTreeMap, BTreeSet};

const MAX_NODES: usize = 128;
const MAX_GLYPHS: usize = 64;

#[derive(Clone)]
pub(super) struct RelocatedHead {
    original: Vec<Inline>,
    cells: Vec<HeadCell>,
    aliases: BTreeMap<String, NativeCell>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct NativeCell {
    owner: u64,
    scalar: char,
}

#[derive(Clone)]
struct HeadCell {
    native: NativeCell,
    link: Option<AnnotationKey>,
}

impl RelocatedHead {
    pub(super) fn new(term: &[Inline]) -> Option<Self> {
        let mut source = SourceCells::default();
        if !source.project(term, None)
            || ![source.cells.first(), source.cells.last()]
                .into_iter()
                .all(|cell| cell.is_some_and(|cell| !cell.native.scalar.is_whitespace()))
        {
            return None;
        }
        Some(Self {
            original: term.to_vec(),
            cells: source.cells,
            aliases: BTreeMap::new(),
        })
    }

    pub(super) fn add(&mut self, owner: &str, alias: &str, scalar: char) -> bool {
        let Some(owner) = native_word(owner) else {
            return false;
        };
        self.aliases.len() < self.cells.len()
            && self
                .aliases
                .insert(alias.to_owned(), NativeCell { owner, scalar })
                .is_none()
    }

    /// All source cells, including interior word spaces, must witness the
    /// first accepted BODY prefix in original order. The detached term
    /// holds the authored link scope even if the BODY drain split its label.
    pub(super) fn restore(self, term: &[Inline], body: &mut Vec<Inline>) -> Option<Vec<Inline>> {
        if self.aliases.len() != self.cells.len() || !has_no_ink_or_break(term) {
            return None;
        }
        let mut cursor = AcceptedInk {
            cells: &self.cells,
            aliases: &self.aliases,
            matched: 0,
            seen: BTreeSet::new(),
            owner: None,
        };
        if !cursor.scan(body, None) || cursor.in_prefix() {
            return None;
        }
        remove_witnessed_ink(body, &self.aliases, &mut false);
        Some(self.original)
    }
}

fn native_word(id: &str) -> Option<u64> {
    id.strip_prefix(INTERNAL_FIELD_WORD)?.parse().ok()
}

/// The accepted BODY drain may erase ordinary field numbers, but these
/// detached aliases still have one definition-seam consumer before IR.
pub(in crate::mandoc::inline::flow) fn is_detached_alias(node: &Inline) -> bool {
    matches!(node, Inline::Anchor { id, .. } if id.as_str()
        .split_once(":relocated:")
        .is_some_and(|(word, cell)| native_word(word).is_some() && cell.parse::<usize>().is_ok()))
}

pub(super) fn strip_aliases(nodes: &mut Vec<Inline>) {
    nodes.retain_mut(|node| match node {
        node if is_detached_alias(node) => false,
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => {
            strip_aliases(children);
            true
        }
        _ => true,
    });
}

pub(super) fn strip_block_aliases(block: &mut Block) {
    struct Scrub;
    impl VisitMut for Scrub {
        fn visit_definition_item_mut(&mut self, item: &mut DefinitionItem) {
            for term in &mut item.terms {
                strip_aliases(&mut term.content);
            }
            for block in &mut item.description {
                self.visit_block_mut(block);
            }
        }

        fn visit_block_mut(&mut self, block: &mut Block) {
            match block {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    strip_aliases(children);
                }
                _ => visit::walk_block_mut(self, block),
            }
        }
    }
    Scrub.visit_block_mut(block);
}

#[derive(Default)]
struct SourceCells {
    owner: Option<u64>,
    cells: Vec<HeadCell>,
    visited: usize,
}

impl SourceCells {
    fn project(&mut self, nodes: &[Inline], link: Option<AnnotationKey>) -> bool {
        for node in nodes {
            self.visited += 1;
            if self.visited > MAX_NODES {
                return false;
            }
            match node {
                Inline::Anchor { id, .. } => {
                    if id.as_str().len() > 256 {
                        return false;
                    }
                    if id.as_str().starts_with(INTERNAL_FIELD_WORD) {
                        self.owner = native_word(id.as_str());
                    } else if id.as_str().starts_with(INTERNAL_FIELD_PREFIX) {
                        self.owner = None;
                    }
                }
                Inline::Text { value } | Inline::Code { value } => {
                    for scalar in value.chars() {
                        let Some(owner) = self.owner else {
                            return false;
                        };
                        // A breakable space must move as a native word
                        // cell too. Tabs and fixed padding have separate
                        // layout receipts and cannot prove this prefix.
                        if scalar.is_whitespace() && scalar != ' ' || self.cells.len() >= MAX_GLYPHS
                        {
                            return false;
                        }
                        self.cells.push(HeadCell {
                            native: NativeCell { owner, scalar },
                            link,
                        });
                    }
                }
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    if !self.project(children, link) {
                        return false;
                    }
                }
                Inline::Link {
                    target, children, ..
                } => {
                    let Some(owner) = target_owner(target) else {
                        return false;
                    };
                    if link.is_some() || !self.project(children, Some(owner)) {
                        return false;
                    }
                }
                Inline::LineBreak {} | Inline::Equation { .. } => return false,
            }
        }
        true
    }
}

fn has_no_ink_or_break(nodes: &[Inline]) -> bool {
    nodes.iter().all(|node| match node {
        Inline::Anchor { .. } => true,
        Inline::Text { value } | Inline::Code { value } => value.is_empty(),
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => has_no_ink_or_break(children),
        _ => false,
    })
}

struct AcceptedInk<'a> {
    cells: &'a [HeadCell],
    aliases: &'a BTreeMap<String, NativeCell>,
    matched: usize,
    seen: BTreeSet<&'a str>,
    owner: Option<(&'a str, NativeCell)>,
}

impl AcceptedInk<'_> {
    fn in_prefix(&self) -> bool {
        self.matched < self.cells.len()
    }

    fn scan(&mut self, nodes: &[Inline], link: Option<AnnotationKey>) -> bool {
        for node in nodes {
            match node {
                Inline::Anchor { id, .. } => {
                    if id.as_str().starts_with(INTERNAL_FIELD_WORD) {
                        self.owner = self
                            .aliases
                            .get_key_value(id.as_str())
                            .map(|(key, &ink)| (key.as_str(), ink));
                    } else if !id.as_str().starts_with('\0') && (self.in_prefix() || link.is_some())
                    {
                        // An authored anchor makes this a different
                        // leading BODY surface, not a contiguous HEAD.
                        return false;
                    }
                }
                Inline::Text { value } | Inline::Code { value } if !value.is_empty() => {
                    if let Some((key, native)) = self.owner {
                        let mut chars = value.chars();
                        if chars.next() != Some(native.scalar)
                            || chars.next().is_some()
                            || !self
                                .cells
                                .get(self.matched)
                                .is_some_and(|cell| cell.native == native && cell.link == link)
                            || !self.seen.insert(key)
                        {
                            return false;
                        }
                        self.matched += 1;
                        self.owner = None;
                    } else if self.in_prefix() || link.is_some() {
                        return false;
                    }
                }
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    if !self.scan(children, link) {
                        return false;
                    }
                }
                Inline::Link {
                    target, children, ..
                } => {
                    // A BODY link can give back a fragment of the same
                    // authored occurrence. Its entire label must move.
                    if link.is_some() {
                        return false;
                    }
                    if self.in_prefix() {
                        let Some(owner) = target_owner(target) else {
                            return false;
                        };
                        let before = self.matched;
                        if !self.scan(children, Some(owner)) || self.matched == before {
                            return false;
                        }
                    } else if !self.scan(children, None) {
                        return false;
                    }
                }
                Inline::LineBreak {} | Inline::Equation { .. }
                    if self.in_prefix() || link.is_some() =>
                {
                    return false;
                }
                _ => (),
            }
        }
        true
    }
}

fn remove_witnessed_ink(
    nodes: &mut Vec<Inline>,
    aliases: &BTreeMap<String, NativeCell>,
    owned: &mut bool,
) -> bool {
    let mut removed = false;
    let mut remaining = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        let is_link = matches!(node, Inline::Link { .. });
        match &mut node {
            Inline::Anchor { id, .. } if id.as_str().starts_with(INTERNAL_FIELD_WORD) => {
                *owned = aliases.contains_key(id.as_str());
                if *owned {
                    continue;
                }
            }
            Inline::Text { value } | Inline::Code { value }
                if !value.is_empty() && std::mem::take(owned) =>
            {
                removed = true;
                continue;
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                if remove_witnessed_ink(children, aliases, owned) {
                    removed = true;
                    if is_link {
                        // Its authored scope stays in the restored term.
                        remaining.append(children);
                        continue;
                    }
                }
            }
            _ => {}
        }
        remaining.push(node);
    }
    *nodes = remaining;
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relocated_head_requires_distinct_receipts_before_any_other_output() {
        let text = |value: &str| Inline::Text {
            value: value.into(),
        };
        let owner = format!("{INTERNAL_FIELD_WORD}1");
        let alias = format!("{INTERNAL_FIELD_WORD}2:relocated:3");
        let original = vec![Inline::anchor(owner.as_str()), text("H")];
        let mut proof = RelocatedHead::new(&original).unwrap();
        assert!(proof.add(&owner, &alias, 'H'));
        let witness = Inline::anchor(alias.as_str());
        let mut beside_body = vec![witness.clone(), text("H"), text("  R<x>")];
        assert_eq!(
            proof.clone().restore(&[], &mut beside_body),
            Some(original.clone())
        );
        assert_eq!(beside_body, [text("  R<x>")]);

        for mut wrong in [
            vec![text("H")],                               // same glyph, BODY source
            vec![text("END"), witness.clone(), text("H")], // foreign prefix
            vec![Inline::line_break(), witness.clone(), text("H")],
            vec![Inline::anchor("authored-body"), witness.clone(), text("H")],
            vec![witness.clone(), text("H"), witness.clone(), text("H")],
        ] {
            let before = wrong.clone();
            assert!(proof.clone().restore(&[], &mut wrong).is_none());
            assert_eq!(wrong, before);
        }
        assert!(
            proof
                .restore(&[text("H")], &mut vec![witness, text("H")])
                .is_none()
        );
    }
}
