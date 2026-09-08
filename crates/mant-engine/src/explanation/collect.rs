//! One source-order walk assigns real owners and finite literal support.
use super::{Candidate, LocatedNode, ResolvedContent, is_identity};
use super::{plan::Candidates, preview::LiteralHit};
use mant_ir::{Block, EntryOwner, Section};
use mant_protocol::EvidenceBasis;
use std::collections::HashMap;

pub(super) fn collect<'a>(
    content: &'a ResolvedContent,
    query: &str,
    located: &[LocatedNode<'a>],
    _validation: Option<&mant_ir::DocumentValidation<'_>>,
) -> (Candidates<'a>, Vec<usize>) {
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
        candidates: Candidates::default(),
        next_order: 0,
        orders: vec![0; located.len()],
    };
    if let Some(document) = &content.document {
        scan.blocks(&document.blocks, None, None, "root");
        scan.sections(&document.sections, "sections");
    }
    (scan.candidates, scan.orders)
}

struct Scan<'a, 'b> {
    query: &'b str,
    located: &'b [LocatedNode<'a>],
    owners: HashMap<usize, usize>,
    sections: HashMap<usize, usize>,
    candidates: Candidates<'a>,
    next_order: usize,
    orders: Vec<usize>,
}

impl<'a> Scan<'a, '_> {
    fn sections(&mut self, sections: &'a [Section], path: &str) {
        for (ordinal, section) in sections.iter().enumerate() {
            let path = format!("{path}/s{ordinal}");
            let index = self.sections[&(std::ptr::from_ref(section) as usize)];
            self.blocks(&section.blocks, None, Some(index), &path);
            self.sections(&section.children, &path);
        }
    }

    fn enter_owner(&mut self, owner: EntryOwner<'a>, current: Option<usize>) -> Option<usize> {
        let Some(&index) = self.owners.get(&owner_key(owner)) else {
            return current;
        };
        self.orders[index] = self.next_order;
        self.next_order += 1;
        let LocatedNode::Entry { entry, .. } = &self.located[index] else {
            unreachable!("owner location")
        };
        let (matched, mut bases) =
            super::matches::MatchPlan::collect(owner, entry.names, self.query);
        if is_identity(&self.located[index], self.query) {
            let mut fields = Vec::new();
            if self.located[index].id() == self.query {
                fields.push(mant_protocol::ExplanationIdentityField::Id);
            }
            if self
                .query
                .parse::<mant_ir::OutlinePath>()
                .is_ok_and(|path| &path == self.located[index].path())
            {
                fields.push(mant_protocol::ExplanationIdentityField::Path);
            }
            bases.push(EvidenceBasis::Identity { fields });
        }
        if !bases.is_empty() {
            self.add_owner(index, bases, Vec::new(), matched);
        }
        Some(index)
    }

    fn add_owner(
        &mut self,
        index: usize,
        bases: Vec<EvidenceBasis>,
        hits: Vec<LiteralHit<'a>>,
        matched: super::matches::MatchPlan,
    ) {
        self.candidates.insert(Candidate {
            order: self.orders[index],
            located: Some(index),
            ordinary: None,
            section: None,
            block_path: None,
            source: self.located[index].source(),
            bases,
            matched,
            hits,
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
                _ => {
                    let Some(text) = super::literal::block_text(block) else {
                        continue;
                    };
                    let Some(range) = super::literal::first_match(&text, self.query) else {
                        continue;
                    };
                    let hit = LiteralHit {
                        block,
                        path: block_path.clone(),
                        range,
                    };
                    if let Some(owner) = current {
                        self.add_owner(
                            owner,
                            vec![EvidenceBasis::Literal],
                            vec![hit],
                            super::matches::MatchPlan::default(),
                        );
                    } else {
                        self.candidates.insert(Candidate {
                            order,
                            located: None,
                            ordinary: Some(block),
                            section,
                            block_path: Some(block_path),
                            source: crate::block::block_source(block),
                            bases: vec![EvidenceBasis::Literal],
                            matched: super::matches::MatchPlan::default(),
                            hits: vec![hit],
                        });
                    }
                }
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
