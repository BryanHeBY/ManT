//! One source-order walk assigns real owners and finite literal support.
use super::{Candidate, LocatedNode, ResolvedContent, is_identity, same};
use mant_ir::{Block, EntryOwner, Section};
use mant_protocol::{EvidenceBasis, MAX_EXPLANATION_CANDIDATES};
use std::collections::HashMap;

pub(super) fn collect<'a>(
    content: &'a ResolvedContent,
    query: &str,
    located: &[LocatedNode<'a>],
) -> (Vec<Candidate<'a>>, bool, Vec<usize>) {
    let mut scan = Scan {
        query,
        located,
        owners: located
            .iter()
            .enumerate()
            .filter_map(|(i, node)| super::owner(node).map(|o| (owner_key(o), i)))
            .collect(),
        sections: located
            .iter()
            .enumerate()
            .filter_map(|(i, node)| match node {
                LocatedNode::Section { section, .. } => {
                    Some((std::ptr::from_ref(*section) as usize, i))
                }
                LocatedNode::Entry { .. } => None,
            })
            .collect(),
        candidates: Vec::new(),
        records: HashMap::new(),
        truncated: false,
        next_order: 0,
        orders: vec![0; located.len()],
    };
    if let Some(document) = &content.document {
        scan.blocks(&document.blocks, None, None, "root");
        scan.sections(&document.sections);
    }
    (scan.candidates, scan.truncated, scan.orders)
}

struct Scan<'a, 'b> {
    query: &'b str,
    located: &'b [LocatedNode<'a>],
    owners: HashMap<usize, usize>,
    sections: HashMap<usize, usize>,
    candidates: Vec<Candidate<'a>>,
    records: HashMap<usize, usize>,
    truncated: bool,
    next_order: usize,
    orders: Vec<usize>,
}

impl<'a> Scan<'a, '_> {
    fn sections(&mut self, sections: &'a [Section]) {
        for section in sections {
            let index = self.sections[&(std::ptr::from_ref(section) as usize)];
            self.blocks(&section.blocks, None, Some(index), "blocks");
            self.sections(&section.children);
        }
    }

    fn enter_owner(&mut self, owner: EntryOwner<'a>, current: Option<usize>) -> Option<usize> {
        let Some(&index) = self.owners.get(&owner_key(owner)) else {
            return current;
        };
        self.orders[index] = self.next_order;
        self.next_order += 1;
        let facts = owner.facts().expect("indexed semantic owner");
        let mut bases = Vec::new();
        if facts
            .names
            .iter()
            .any(|name| same(name, self.query, facts.case))
        {
            bases.push(EvidenceBasis::Name);
        }
        if owner.forms().is_some_and(|forms| {
            forms
                .iter()
                .any(|form| same(&crate::inline::plain_text(form), self.query, facts.case))
        }) {
            bases.push(EvidenceBasis::Form);
        }
        if is_identity(&self.located[index], self.query) {
            bases.push(EvidenceBasis::Identity);
        }
        if !bases.is_empty() {
            self.add_owner(index, bases);
        }
        Some(index)
    }

    fn add_owner(&mut self, index: usize, bases: Vec<EvidenceBasis>) {
        if let Some(&record) = self.records.get(&index) {
            for basis in bases {
                if !self.candidates[record].bases.contains(&basis) {
                    self.candidates[record].bases.push(basis);
                }
            }
            return;
        }
        if self.candidates.len() == MAX_EXPLANATION_CANDIDATES {
            self.truncated = true;
            return;
        }
        self.records.insert(index, self.candidates.len());
        self.candidates.push(Candidate {
            order: self.orders[index],
            located: Some(index),
            ordinary: None,
            section: None,
            block_path: None,
            source: self.located[index].source(),
            bases,
        });
    }

    fn blocks(
        &mut self,
        blocks: &'a [Block],
        current: Option<usize>,
        section: Option<usize>,
        path: &str,
    ) {
        for (index, block) in blocks.iter().enumerate() {
            let block_path = format!("{path}/b{index}");
            let order = self.next_order;
            self.next_order += 1;
            match block {
                Block::List { items, .. } => {
                    for (i, item) in items.iter().enumerate() {
                        let owner = self.enter_owner(EntryOwner::List(item), current);
                        self.blocks(&item.blocks, owner, section, &format!("{block_path}/i{i}"));
                    }
                }
                Block::DefinitionList { items, .. } => {
                    for (i, item) in items.iter().enumerate() {
                        let owner = self.enter_owner(EntryOwner::Definition(item), current);
                        // Terms are already matched as complete forms; literal
                        // support must not turn parameter substrings into names.
                        self.blocks(
                            &item.description,
                            owner,
                            section,
                            &format!("{block_path}/d{i}"),
                        );
                    }
                }
                Block::Table { rows, .. } => {
                    for (r, row) in rows.iter().enumerate() {
                        for (c, cell) in row.cells.iter().enumerate() {
                            self.blocks(
                                &cell.blocks,
                                current,
                                section,
                                &format!("{block_path}/r{r}/c{c}"),
                            );
                        }
                    }
                }
                _ if block_matches(block, self.query) => {
                    if let Some(owner) = current {
                        self.add_owner(owner, vec![EvidenceBasis::Literal]);
                    } else if self.candidates.len() < MAX_EXPLANATION_CANDIDATES {
                        self.candidates.push(Candidate {
                            order,
                            located: None,
                            ordinary: Some(block),
                            section,
                            block_path: Some(block_path),
                            source: crate::block::block_source(block),
                            bases: vec![EvidenceBasis::Literal],
                        });
                    } else {
                        self.truncated = true;
                    }
                }
                _ => {}
            }
        }
    }
}

fn owner_key(owner: EntryOwner<'_>) -> usize {
    match owner {
        EntryOwner::List(item) => std::ptr::from_ref(item) as usize,
        EntryOwner::Definition(item) => std::ptr::from_ref(item) as usize,
    }
}

fn block_matches(block: &Block, query: &str) -> bool {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            super::literal::first_match(&crate::inline::plain_text(children), query).is_some()
        }
        Block::Equation { value, .. } | Block::Unsupported { text: value, .. } => {
            super::literal::first_match(value, query).is_some()
        }
        _ => false,
    }
}
