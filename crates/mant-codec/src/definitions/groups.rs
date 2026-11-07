//! Native adjacency witnesses survive normalization without guessing from IR
//! indentation or moving another item's description into a semantic owner.
#[cfg(feature = "roff")]
use super::evidence::head_snapshot;
use super::evidence::{HeadSnapshot, head_matches};
use mant_ir::{Block, DeclarationGroup, DefinitionItem, Inline, Section, SourceSpan};
use std::collections::{HashMap, HashSet};

// This marker is deliberately impossible in sanitized roff input.  It is an
// in-memory hand-off between native lowering and semantic preparation, never
// a document anchor: navigation skips it and preparation removes it before
// the public IR is returned.
const OWNER_MARKER_PREFIX: &str = "\0mant-native-definition-owner:";

#[cfg(feature = "roff")]
pub(crate) fn mark_native_definition_owner(item: &mut DefinitionItem, key: usize) {
    let Some(term) = item.terms.first_mut() else {
        return;
    };
    mark_native_inline_owner(&mut term.content, key);
}

#[cfg(feature = "roff")]
pub(crate) fn mark_native_inline_owner(inlines: &mut Vec<Inline>, key: usize) {
    inlines.insert(0, Inline::anchor(format!("{OWNER_MARKER_PREFIX}{key:x}")));
}

pub(crate) fn native_inline_owner(inlines: &[Inline]) -> Option<usize> {
    inlines.iter().find_map(|inline| {
        let Inline::Anchor { id, .. } = inline else {
            return None;
        };
        id.as_str()
            .strip_prefix(OWNER_MARKER_PREFIX)
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .and_then(|value| usize::from_str_radix(value, 16).ok())
    })
}

pub(crate) fn is_internal_definition_owner_marker(id: &str) -> bool {
    id.strip_prefix(OWNER_MARKER_PREFIX).is_some_and(|value| {
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn owner_marker(item: &DefinitionItem) -> Option<usize> {
    item.terms.iter().find_map(|term| native_inline_owner(term))
}

fn remove_inlines(inlines: &mut Vec<Inline>) {
    let mut retained = Vec::with_capacity(inlines.len());
    for mut inline in std::mem::take(inlines) {
        match &mut inline {
            Inline::Anchor { id, .. } if is_internal_definition_owner_marker(id.as_str()) => {
                continue;
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => remove_inlines(children),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Equation { .. }
            | Inline::Anchor { .. }
            | Inline::LineBreak { .. } => {}
        }
        retained.push(inline);
    }
    *inlines = retained;
}

pub(crate) fn remove_native_definition_owner_markers_from_items(items: &mut [DefinitionItem]) {
    for item in items {
        for term in &mut item.terms {
            remove_inlines(&mut term.content);
        }
    }
}

pub(crate) fn remove_native_definition_owner_markers(
    blocks: &mut [Block],
    sections: &mut [Section],
) {
    fn remove_blocks(blocks: &mut [Block]) {
        for block in blocks {
            match block {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    remove_inlines(children);
                }
                Block::DefinitionList { items, .. } => {
                    remove_native_definition_owner_markers_from_items(items);
                    for item in items {
                        remove_blocks(&mut item.description);
                    }
                }
                Block::List { items, .. } => {
                    for item in items {
                        remove_blocks(&mut item.blocks);
                    }
                }
                Block::Table { rows, .. } => {
                    for row in rows {
                        for cell in &mut row.cells {
                            remove_blocks(&mut cell.blocks);
                        }
                    }
                }
                Block::Equation { .. }
                | Block::VerticalSpace { .. }
                | Block::ThematicBreak { .. }
                | Block::Unsupported { .. } => {}
            }
        }
    }
    remove_blocks(blocks);
    for section in sections {
        remove_inlines(&mut section.heading.content);
        remove_blocks(&mut section.blocks);
        remove_native_definition_owner_markers(&mut [], &mut section.children);
    }
}

struct Witness {
    source: SourceSpan,
    head: HeadSnapshot,
    key: usize,
    last_key: usize,
}

/// A native owner allocation plan built after normalization and consumed in
/// that same structural traversal. Source coordinates are not identities:
/// macro expansion may legitimately duplicate them. Each definition instead
/// carries its parse-local native owner marker until semantic preparation has
/// consumed it, so nested normalization cannot reassign an identical head to
/// a different expanded macro invocation.
#[derive(Default)]
pub(crate) struct GroupMatchingPlan {
    owners: HashMap<usize, Vec<OwnerBinding>>,
    occurrences: HashMap<usize, usize>,
}

struct OwnerBinding {
    source: SourceSpan,
    head: HeadSnapshot,
    last_key: usize,
}

#[derive(Default)]
pub(crate) struct GroupEvidence {
    edges: HashSet<(usize, usize)>,
    // A semantic declaration group may start only at the beginning of a
    // native declaration run, or after a source owner with its own readable
    // body.  Keep the raw sibling fact separately from `edges`: semantic
    // preparation deliberately drops unaddressable heads, and must not make
    // the next recognizable head look like a fresh adjacent run.
    native_predecessors: HashSet<usize>,
    // Some native list heads are formatter-visible templates rather than
    // addressable declarations: `/RE`, `-min-len`, a bracketed parameter,
    // and unsigned numeric labels are representative examples. They still
    // split a physical run, but must not make a later independently named
    // suffix borrow their unclassifiable status.
    presentation_heads: HashSet<usize>,
    presentation_predecessors: HashSet<usize>,
    // A native owner with an actual body closes its physical run even if
    // lowering later converts it to an ordinary/ordered list. The following
    // declaration must not inherit an earlier unclassified-head barrier.
    body_closed_predecessors: HashSet<usize>,
    items: HashMap<usize, Vec<Witness>>,
}

impl GroupEvidence {
    #[cfg(feature = "roff")]
    pub(super) fn publication_witness(&self, owner: usize, item: &DefinitionItem) -> Option<usize> {
        let source = item.source?;
        let mut matches = self
            .items
            .get(&owner)?
            .iter()
            .enumerate()
            .filter(|(_, witness)| {
                witness.source == source
                    && super::evidence::publication_matches(&item.terms, &witness.head)
            });
        let (index, first) = matches.next()?;
        matches
            .all(|(_, witness)| witness.last_key == first.last_key)
            .then_some(index)
    }

    #[cfg(feature = "roff")]
    pub(super) fn rebase_publication(
        &mut self,
        owner: usize,
        index: Option<usize>,
        head: Option<&HeadSnapshot>,
    ) {
        if let (Some(index), Some(witnesses)) = (index, self.items.get_mut(&owner)) {
            let mut witness = witnesses.swap_remove(index);
            if let Some(head) = head {
                witness.head = head.clone();
            }
            witnesses.clear();
            witnesses.push(witness);
        } else {
            self.items.remove(&owner);
        }
    }

    /// Only the current complete source/head proof can share storage. Older
    /// continuation versions remain immutable for matching and rejection.
    pub(super) fn shared_head(&self, item: &DefinitionItem) -> Option<HeadSnapshot> {
        let source = item.source?;
        let witness = self.items.get(&owner_marker(item)?)?.last()?;
        (witness.source == source && head_matches(&item.terms, &witness.head))
            .then(|| witness.head.clone())
    }

    #[cfg(feature = "roff")]
    pub(crate) fn adjacent(&mut self, left: usize, right: usize, left_has_body: bool) {
        self.edges.insert((left, right));
        self.native_predecessors.insert(right);
        if self.presentation_heads.contains(&left) {
            self.presentation_predecessors.insert(right);
        }
        if left_has_body {
            self.body_closed_predecessors.insert(right);
        }
    }
    #[cfg(feature = "roff")]
    pub(crate) fn presentation_head(&mut self, key: usize) {
        self.presentation_heads.insert(key);
    }
    #[cfg(feature = "roff")]
    pub(crate) fn record(&mut self, item: &DefinitionItem, key: usize) {
        let Some(source) = item.source else { return };
        self.items.entry(key).or_default().push(Witness {
            source,
            head: head_snapshot(&item.terms),
            key,
            last_key: key,
        });
    }
    /// TQ extends one physical owner. Rebind the exact merged head to the
    /// original first node and final continuation, not its new array position
    /// or a source-coordinate lookalike. Macro expansion can duplicate both
    /// a definition's source span and its visible head, so only the native
    /// owner marker survives as an unambiguous identity here.
    #[cfg(feature = "roff")]
    pub(crate) fn continued(&mut self, item: &DefinitionItem, last_key: usize) {
        let Some(source) = item.source else { return };
        let Some(key) = owner_marker(item) else {
            return;
        };
        let head = head_snapshot(&item.terms);
        self.items.entry(key).or_default().push(Witness {
            source,
            head,
            key,
            last_key,
        });
    }
    /// Build a complete owner allocation plan only after all normalization has
    /// finished.  This rejects a damaged repeated macro stream globally, but
    /// does not confuse it with the valid case where the same expansion is
    /// distributed across multiple physical definition lists.
    pub(crate) fn matching_plan(
        &self,
        blocks: &[Block],
        sections: &[Section],
    ) -> GroupMatchingPlan {
        let mut plan = GroupMatchingPlan::default();
        for witnesses in self.items.values() {
            for witness in witnesses {
                let bindings = plan.owners.entry(witness.key).or_default();
                if !bindings.iter().any(|binding| {
                    binding.source == witness.source
                        && binding.head == witness.head
                        && binding.last_key == witness.last_key
                }) {
                    bindings.push(OwnerBinding {
                        source: witness.source,
                        head: witness.head.clone(),
                        last_key: witness.last_key,
                    });
                }
            }
        }
        plan.count_blocks(blocks);
        for section in sections {
            plan.count_blocks(&section.blocks);
            plan.count_sections(&section.children);
        }
        plan
    }

    fn key(item: &DefinitionItem, plan: &mut GroupMatchingPlan) -> Option<(usize, usize)> {
        let source = item.source?;
        let owner = owner_marker(item)?;
        // A normalized rewrite must retain exactly one native owner marker.
        // Duplicate/collapsed owners are unsafe just like a missing owner;
        // neither may borrow another macro expansion's witness.
        if plan.occurrences.get(&owner) != Some(&1) {
            return None;
        }
        plan.owners
            .get(&owner)?
            .iter()
            .find(|binding| binding.source == source && head_matches(&item.terms, &binding.head))
            .map(|binding| (owner, binding.last_key))
    }
    /// One linear pass over final owners; recognizability comes from the same
    /// preparation plan that will allocate their names, never another parser.
    pub(super) fn resolve(
        &self,
        items: &[DefinitionItem],
        heads: &[bool],
        plan: &mut GroupMatchingPlan,
    ) -> Vec<DeclarationGroup> {
        let mut result = Vec::new();
        let mut pending = None;
        let mut previous = None;
        // Once a native declaration run contains a head that semantic
        // preparation cannot retain, it cannot prove a later suffix shares a
        // description.  Keep the block until a readable body closes that
        // physical run instead of restarting inference at the next named
        // owner.
        let mut blocked_by_unclassified_head = false;
        for (index, item) in items.iter().enumerate() {
            let Some(key) = Self::key(item, plan) else {
                pending = None;
                previous = None;
                continue;
            };
            if !heads[index] {
                pending = None;
                // A readable, unclassified owner has an independent body:
                // it closes the physical declaration run just like a
                // recognized item.  Empty unknown heads still block a later
                // suffix from borrowing an unrelated description.
                if mant_ir::blocks_have_readable_content(&item.description) {
                    blocked_by_unclassified_head = false;
                } else if !self.presentation_heads.contains(&key.0) {
                    blocked_by_unclassified_head = true;
                }
                previous = Some(key.1);
                continue;
            }
            let contiguous = previous
                .zip(Some(key.0))
                .is_some_and(|edge| self.edges.contains(&edge));
            if !contiguous {
                pending = None;
                blocked_by_unclassified_head |= self.native_predecessors.contains(&key.0)
                    && !self.presentation_predecessors.contains(&key.0)
                    && !self.body_closed_predecessors.contains(&key.0);
            }
            if mant_ir::blocks_have_readable_content(&item.description) {
                let start_item = pending.take();
                if !blocked_by_unclassified_head && let Some(start_item) = start_item {
                    result.push(DeclarationGroup {
                        start_item,
                        end_item: index + 1,
                    });
                }
                blocked_by_unclassified_head = false;
            } else if !blocked_by_unclassified_head {
                // Do not reconstruct a run after semantic preparation has
                // rejected an intervening native definition head.  That
                // pattern is commonly a command/example transcript followed
                // by prose for its final line, not several declarations with
                // one shared description.
                pending.get_or_insert(index);
            }
            previous = Some(key.1);
        }
        result
    }
}

impl GroupMatchingPlan {
    fn count_sections(&mut self, sections: &[Section]) {
        for section in sections {
            self.count_blocks(&section.blocks);
            self.count_sections(&section.children);
        }
    }

    fn count_blocks(&mut self, blocks: &[Block]) {
        for block in blocks {
            match block {
                Block::DefinitionList { items, .. } => {
                    for item in items {
                        self.count(item);
                        self.count_blocks(&item.description);
                    }
                }
                Block::List { items, .. } => {
                    for item in items {
                        self.count_blocks(&item.blocks);
                    }
                }
                Block::Table { rows, .. } => {
                    for row in rows {
                        for cell in &row.cells {
                            self.count_blocks(&cell.blocks);
                        }
                    }
                }
                Block::Paragraph { .. }
                | Block::Preformatted { .. }
                | Block::Equation { .. }
                | Block::VerticalSpace { .. }
                | Block::ThematicBreak { .. }
                | Block::Unsupported { .. } => {}
            }
        }
    }

    fn count(&mut self, item: &DefinitionItem) {
        if let Some(owner) = owner_marker(item) {
            *self.occurrences.entry(owner).or_default() += 1;
        }
    }
}

// These witnesses are populated only by the native roff lowering path.  Keep
// their regression matrix out of the Markdown-only build, which deliberately
// omits the roff-only recording API.
#[cfg(all(test, feature = "roff"))]
mod tests;
