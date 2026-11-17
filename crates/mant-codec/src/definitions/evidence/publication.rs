//! Carry already proved native heads through the one publication of their
//! accepted inline roots. Neither source coordinates nor label text can
//! select a witness for another owner.
use super::{
    NativeHeadEvidence, NativeOperand, NativeOperandRole, TermOperands, head_snapshot,
    native_owner, publication_matches,
};
use mant_ir::{
    Block, DefinitionItem, Diagnostic, DiagnosticImpact, DiagnosticLevel, Heading, Inline, Section,
    SourceSpan, inline_plain_text,
    visit::{self, Visit, VisitMut},
};
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

/// Changes from publishing one inline root. Offsets belong to the accepted
/// text before publication, including bytes in nested style and link nodes.
#[derive(Default)]
pub(crate) struct InlinePublication {
    pub(crate) changed: bool,
    deletions: Vec<Deletion>,
}

struct Deletion {
    bytes: Range<usize>,
    removed_before: usize,
}

impl InlinePublication {
    pub(crate) fn remove(&mut self, bytes: Range<usize>) {
        let removed_before = self
            .deletions
            .last()
            .map_or(0, |last| last.removed_before + last.bytes.len());
        self.deletions.push(Deletion {
            bytes,
            removed_before,
        });
        self.changed = true;
    }

    /// Check each recorded byte deletion against the original and the final
    /// text, including every intervening span. Matching text elsewhere in
    /// the root cannot supply evidence for an unknown edit.
    fn matches(&self, before: &str, mut after: &str) -> bool {
        let mut offset = 0;
        for deletion in &self.deletions {
            let Some(retained) = before.get(offset..deletion.bytes.start) else {
                return false;
            };
            if deletion.bytes.is_empty() || before.get(deletion.bytes.clone()).is_none() {
                return false;
            }
            let Some(rest) = after.strip_prefix(retained) else {
                return false;
            };
            after = rest;
            offset = deletion.bytes.end;
        }
        before.get(offset..) == Some(after)
    }

    fn offset(&self, original: usize) -> usize {
        let index = self
            .deletions
            .partition_point(|deletion| deletion.bytes.start < original);
        self.deletions[..index].last().map_or(original, |deletion| {
            let removed = original.min(deletion.bytes.end) - deletion.bytes.start;
            original - deletion.removed_before - removed
        })
    }

    /// Return whether publication clipped an already qualified option key.
    /// Parameter bounds survive a deletion so the remaining text stays
    /// excluded from lexical matching. A clipped typed subject or complete
    /// key no longer proves a name.
    pub(in crate::definitions) fn rebase_operands(
        &self,
        operands: &mut Vec<NativeOperand>,
        before: &[Inline],
        after: &[Inline],
    ) -> Option<bool> {
        if !self.matches(&inline_plain_text(before), &inline_plain_text(after)) {
            return None;
        }
        let mut lost_key = false;
        if !self.deletions.is_empty() {
            operands.retain_mut(|operand| {
                let projected = self.offset(operand.bytes.start)..self.offset(operand.bytes.end);
                if projected.len() != operand.bytes.len() {
                    lost_key |= operand.role == NativeOperandRole::ExplicitOption;
                    operand.role = match operand.role {
                        NativeOperandRole::Subject(subject, scope) => {
                            NativeOperandRole::ClippedSubject(subject, scope)
                        }
                        NativeOperandRole::ExplicitOption | NativeOperandRole::Placeholder => {
                            NativeOperandRole::Argument
                        }
                        role => role,
                    };
                }
                operand.bytes = projected;
                !operand.bytes.is_empty()
            });
        }
        Some(lost_key)
    }
}

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
        publish: impl FnMut(&mut Vec<Inline>) -> InlinePublication,
    ) -> Vec<Diagnostic> {
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
            diagnostics: Vec::new(),
        };
        for block in blocks {
            publisher.visit_block_mut(block);
        }
        for section in sections {
            publisher.visit_section_mut(section);
        }
        publisher.diagnostics
    }
}

struct Publisher<'a, F> {
    evidence: &'a mut NativeHeadEvidence,
    owners: Owners,
    publish: F,
    diagnostics: Vec<Diagnostic>,
}

impl<F: FnMut(&mut Vec<Inline>) -> InlinePublication> VisitMut for Publisher<'_, F> {
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
        let (Block::Paragraph {
            children, source, ..
        }
        | Block::Preformatted {
            children, source, ..
        }) = block
        else {
            unreachable!("inline publication root")
        };
        let publication = (self.publish)(children);
        if let Some(key) = owner {
            if !proved || native_inline_owner(children) != Some(key) {
                self.evidence.hanging.discard_pair(key);
            } else if publication.changed
                && self
                    .evidence
                    .hanging
                    .rebase_published_head(key, children, &publication)
            {
                self.diagnostics.push(operand_coverage(*source));
            }
        }
    }
}

impl<F: FnMut(&mut Vec<Inline>) -> InlinePublication> Publisher<'_, F> {
    fn publish_terms(&mut self, item: &mut DefinitionItem) {
        for term in &mut item.terms {
            (self.publish)(&mut term.content);
        }
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
            .then(|| self.evidence.matching_operand_witness(item, owner))
            .flatten()
            .map(|(index, _)| index);
        let mut witness = self
            .evidence
            .operand_witnesses
            .get_mut(&owner)
            .and_then(|witnesses| {
                let matched = operands.map(|index| witnesses.swap_remove(index));
                // Preserve an explicit rejection if no complete witness matches.
                witnesses.clear();
                matched
            });
        let mut changed = false;
        let mut lost_operands = false;
        for (index, term) in item.terms.iter_mut().enumerate() {
            let publication = (self.publish)(&mut term.content);
            changed |= publication.changed;
            if publication.changed
                && let Some(witness) = &mut witness
                && let TermOperands::Accepted(operands) = &mut witness.operands[index]
            {
                if let Some(lost_key) = publication.rebase_operands(
                    Rc::make_mut(operands),
                    &witness.terms[index],
                    &term.content,
                ) {
                    lost_operands |= lost_key;
                } else {
                    lost_operands |= !operands.is_empty();
                    witness.operands[index] = TermOperands::Unavailable;
                }
            }
        }
        let same_owner = unique && native_owner(item) == Some(owner);
        let head = (changed && same_owner && (group.is_some() || role || operands.is_some()))
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
        if let Some(mut witness) = witness {
            if same_owner {
                if let Some(head) = head {
                    witness.terms = head;
                }
                self.evidence
                    .operand_witnesses
                    .get_mut(&owner)
                    .expect("the matched operand witness had an owner")
                    .push(witness);
            } else {
                lost_operands = true;
            }
        }
        if lost_operands {
            self.diagnostics.push(operand_coverage(item.source));
        }
    }
}

fn operand_coverage(source: Option<SourceSpan>) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warning,
        impact: DiagnosticImpact::SemanticCoverage,
        code: Some("manual.semantic-entry.operand-coverage".into()),
        message:
            "could not publish all executed declaration evidence. The readable form is retained"
                .into(),
        source,
    }
}
