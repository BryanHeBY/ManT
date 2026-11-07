//! Carry already proved native heads through the one publication of their
//! accepted inline roots. Neither source coordinates nor label text can
//! select a witness for another owner.
use super::{NativeHeadEvidence, NativeOperand, head_snapshot, native_owner, publication_matches};
use mant_ir::{
    Block, DefinitionItem, Heading, Inline, Section,
    visit::{self, Visit, VisitMut},
};
use std::collections::HashMap;

#[derive(Default)]
struct Owners(HashMap<usize, usize>);

impl<'ir> Visit<'ir> for Owners {
    fn visit_inline(&mut self, node: &'ir Inline) {
        if let Some(key) = super::super::groups::native_inline_owner(std::slice::from_ref(node)) {
            *self.0.entry(key).or_default() += 1;
        }
        visit::walk_inline(self, node);
    }
}

impl Owners {
    fn unique(&self, owner: usize) -> bool {
        self.0.get(&owner) == Some(&1)
    }
}

impl NativeHeadEvidence {
    pub(crate) fn publish_annotations(
        &mut self,
        blocks: &mut [Block],
        sections: &mut [Section],
        publish: impl FnMut(&mut Vec<Inline>) -> bool,
    ) {
        let mut owners = Owners::default();
        for block in blocks.iter() {
            owners.visit_block(block);
        }
        for section in sections.iter() {
            owners.visit_section(section);
        }
        let mut publisher = Publisher {
            evidence: self,
            owners,
            publish,
        };
        for block in blocks {
            publisher.visit_block_mut(block);
        }
        for section in sections {
            publisher.visit_section_mut(section);
        }
    }
}

struct Publisher<'a, F> {
    evidence: &'a mut NativeHeadEvidence,
    owners: Owners,
    publish: F,
}

impl<F: FnMut(&mut Vec<Inline>) -> bool> VisitMut for Publisher<'_, F> {
    fn visit_heading_mut(&mut self, heading: &mut Heading) {
        (self.publish)(&mut heading.content);
    }

    fn visit_definition_item_mut(&mut self, item: &mut DefinitionItem) {
        if let Some(owner) = native_owner(item) {
            self.publish_head(item, owner);
        } else {
            self.publish_terms(item);
        }
        for block in &mut item.description {
            self.visit_block_mut(block);
        }
    }

    fn visit_block_mut(&mut self, block: &mut Block) {
        use super::super::groups::native_inline_owner;
        let owner = match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                native_inline_owner(children)
            }
            _ => {
                visit::walk_block_mut(self, block);
                return;
            }
        };
        let proved = owner.is_some_and(|key| {
            self.owners.unique(key)
                && self
                    .evidence
                    .hanging
                    .receiver_of_proved_head(key, block)
                    .is_some_and(|body| self.owners.unique(body))
        });
        let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) = block
        else {
            unreachable!("inline publication root")
        };
        let changed = (self.publish)(children);
        if let Some(key) = owner {
            if !proved || native_inline_owner(children) != Some(key) {
                self.evidence.hanging.discard_pair(key);
            } else if changed {
                self.evidence.hanging.rebase_published_head(key, children);
            }
        }
    }
}

impl<F: FnMut(&mut Vec<Inline>) -> bool> Publisher<'_, F> {
    fn publish_terms(&mut self, item: &mut DefinitionItem) -> bool {
        let mut changed = false;
        for term in &mut item.terms {
            changed |= (self.publish)(&mut term.content);
        }
        changed
    }

    fn publish_head(&mut self, item: &mut DefinitionItem, owner: usize) {
        let unique = self.owners.unique(owner);
        let group = unique
            .then(|| self.evidence.groups.publication_witness(owner, item))
            .flatten();
        let role = unique
            && self
                .evidence
                .native_witnesses
                .get(&owner)
                .is_some_and(|witness| {
                    Some(witness.source) == item.source
                        && publication_matches(&item.terms, &witness.terms)
                });
        let operands = unique
            && self
                .evidence
                .operand_witnesses
                .get(&owner)
                .is_some_and(|witness| {
                    Some(witness.source) == item.source
                        && publication_matches(&item.terms, &witness.terms)
                });
        let changed = self.publish_terms(item);
        let same_owner = unique && native_owner(item) == Some(owner);
        let head = (changed && same_owner && (group.is_some() || role || operands))
            .then(|| head_snapshot(&item.terms));

        self.evidence
            .groups
            .rebase_publication(owner, group.filter(|_| same_owner), head.as_ref());
        if !same_owner || !role {
            self.evidence.native_witnesses.remove(&owner);
        } else if let Some(head) = &head
            && let Some(witness) = self.evidence.native_witnesses.get_mut(&owner)
        {
            witness.terms = head.clone();
        }
        if !same_owner || !operands {
            self.evidence.operand_witnesses.remove(&owner);
        } else if let Some(head) = head
            && let Some(witness) = self.evidence.operand_witnesses.get_mut(&owner)
        {
            for ((operands, before), after) in witness
                .operands
                .iter_mut()
                .zip(witness.terms.iter())
                .zip(head.iter())
            {
                retain_unchanged_operands(operands, before, after);
            }
            witness.terms = head;
        }
    }
}

/// Publication can consume a source-proven reference marker. A byte range
/// remains valid only through the unchanged prefix of its verified term.
pub(in crate::definitions) fn retain_unchanged_operands(
    operands: &mut Vec<NativeOperand>,
    before: &[Inline],
    after: &[Inline],
) {
    if operands.is_empty() {
        return;
    }
    let before = mant_ir::inline_plain_text(before);
    let after = mant_ir::inline_plain_text(after);
    let unchanged = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    operands.retain(|operand| operand.bytes.end <= unchanged);
}
