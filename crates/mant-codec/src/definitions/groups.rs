//! Native adjacency witnesses survive normalization without guessing from IR
//! indentation or moving another item's description into a semantic owner.
use super::evidence::head_content;
use mant_ir::{DeclarationGroup, DefinitionItem, Inline, SourceSpan};
use std::collections::{HashMap, HashSet};

struct Witness {
    source: SourceSpan,
    head: Vec<Vec<Inline>>,
    key: usize,
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
    items: HashMap<(u32, u32), Vec<Witness>>,
}

impl GroupEvidence {
    #[cfg(feature = "roff")]
    pub(crate) fn adjacent(&mut self, left: usize, right: usize) {
        self.edges.insert((left, right));
        self.native_predecessors.insert(right);
    }
    #[cfg(feature = "roff")]
    pub(crate) fn record(&mut self, item: &DefinitionItem, key: usize) {
        let Some(source) = item.source else { return };
        self.items
            .entry((source.line, source.column))
            .or_default()
            .push(Witness {
                source,
                head: head_content(&item.terms),
                key,
                last_key: key,
            });
    }
    /// TQ extends one physical owner. Rebind the exact merged head to the
    /// original first node and final continuation, not its new array position.
    #[cfg(feature = "roff")]
    pub(crate) fn continued(&mut self, item: &DefinitionItem, last_key: usize) {
        let Some(source) = item.source else { return };
        let head = head_content(&item.terms);
        let Some(bucket) = self.items.get_mut(&(source.line, source.column)) else {
            return;
        };
        let candidates = bucket
            .iter()
            .filter(|w| w.source == source && head.starts_with(&w.head));
        let Some(key) = candidates
            .map(|w| w.key)
            .reduce(|a, b| if a == b { a } else { 0 })
            .filter(|&key| key != 0)
        else {
            return;
        };
        bucket.push(Witness {
            source,
            head,
            key,
            last_key,
        });
    }
    fn key(&self, item: &DefinitionItem) -> Option<(usize, usize)> {
        let source = item.source?;
        let head = head_content(&item.terms);
        let mut witnesses = self
            .items
            .get(&(source.line, source.column))?
            .iter()
            .filter(|w| w.source == source && w.head == head);
        let first = witnesses.next()?;
        let key = (first.key, first.last_key);
        witnesses.all(|w| (w.key, w.last_key) == key).then_some(key)
    }
    /// One linear pass over final owners; recognizability comes from the same
    /// preparation plan that will allocate their names, never another parser.
    pub(super) fn resolve(
        &self,
        items: &[DefinitionItem],
        heads: &[bool],
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
            let Some(key) = self.key(item) else {
                pending = None;
                previous = None;
                continue;
            };
            if !heads[index] {
                pending = None;
                previous = None;
                blocked_by_unclassified_head = true;
                continue;
            }
            let contiguous = previous
                .zip(Some(key.0))
                .is_some_and(|edge| self.edges.contains(&edge));
            if !contiguous {
                pending = None;
                blocked_by_unclassified_head |= self.native_predecessors.contains(&key.0);
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
