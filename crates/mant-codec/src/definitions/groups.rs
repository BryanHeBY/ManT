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
    items: HashMap<(u32, u32), Vec<Witness>>,
}

impl GroupEvidence {
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
    fn key(
        &self,
        item: &DefinitionItem,
        items: &[DefinitionItem],
        consumed: &mut HashMap<(u32, u32), Vec<bool>>,
    ) -> Option<(usize, usize)> {
        let source = item.source?;
        let head = head_content(&item.terms);
        let coordinate = (source.line, source.column);
        let witnesses = self.items.get(&coordinate)?;
        let source_occurrences = items
            .iter()
            .filter(|candidate| {
                candidate.source == Some(source) && head_content(&candidate.terms) == head
            })
            .count();
        let witness_occurrences = witnesses
            .iter()
            .filter(|witness| witness.source == source && witness.head == head)
            .count();
        // If normalization removed or rewrote one member of an otherwise
        // indistinguishable macro-expanded stream, source coordinates cannot
        // safely identify the survivors. Decline the whole ambiguous class
        // rather than binding a later owner to an earlier native pointer.
        if source_occurrences != witness_occurrences {
            return None;
        }
        // Macro expansion may create several independent native owners with
        // the exact same source coordinate and head.  Their pointer identity
        // is the only reliable edge witness, so consume matching witnesses in
        // lowering order rather than declaring the whole coordinate ambiguous.
        let used = consumed
            .entry(coordinate)
            .or_insert_with(|| vec![false; witnesses.len()]);
        witnesses
            .iter()
            .enumerate()
            .find(|(index, witness)| {
                !used[*index] && witness.source == source && witness.head == head
            })
            .map(|(index, witness)| {
                used[index] = true;
                (witness.key, witness.last_key)
            })
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
        let mut consumed = HashMap::new();
        // Once a native declaration run contains a head that semantic
        // preparation cannot retain, it cannot prove a later suffix shares a
        // description.  Keep the block until a readable body closes that
        // physical run instead of restarting inference at the next named
        // owner.
        let mut blocked_by_unclassified_head = false;
        for (index, item) in items.iter().enumerate() {
            let Some(key) = self.key(item, items, &mut consumed) else {
                pending = None;
                previous = None;
                continue;
            };
            if !heads[index] {
                pending = None;
                previous = None;
                if !self.presentation_heads.contains(&key.0) {
                    blocked_by_unclassified_head = true;
                }
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

#[cfg(test)]
mod tests {
    use super::GroupEvidence;
    use mant_ir::{Block, DefinitionItem, DefinitionLayout, Inline, LayoutHint, SourceSpan};

    fn item(line: u32, column: u32, name: &str, description: bool) -> DefinitionItem {
        DefinitionItem {
            source: Some(SourceSpan {
                line,
                column,
                byte_range: None,
                end_line: None,
                end_column: None,
            }),
            entry: None,
            terms: vec![vec![Inline::Text {
                value: name.to_owned(),
            }]],
            description: if description {
                vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: "description".to_owned(),
                    }],
                    source: None,
                    layout: LayoutHint::default(),
                }]
            } else {
                Vec::new()
            },
            layout: DefinitionLayout::default(),
        }
    }

    #[test]
    fn presentation_heads_split_but_do_not_poison_a_named_suffix_group() {
        let mut evidence = GroupEvidence::default();
        let items = vec![
            item(1, 1, "/RE", false),
            item(2, 1, "?RE", false),
            item(3, 1, "n", true),
        ];
        evidence.record(&items[0], 10);
        evidence.record(&items[1], 20);
        evidence.record(&items[2], 30);
        evidence.presentation_head(10);
        evidence.adjacent(10, 20, false);
        evidence.adjacent(20, 30, false);

        assert_eq!(
            evidence.resolve(&items, &[false, true, true]),
            vec![mant_ir::DeclarationGroup {
                start_item: 1,
                end_item: 3,
            }]
        );
    }

    #[test]
    fn macro_expansion_keeps_same_coordinate_runs_distinct() {
        let mut evidence = GroupEvidence::default();
        let items = vec![
            item(12, 2, "-a", false),
            item(12, 2, "-b", true),
            item(12, 2, "-a", false),
            item(12, 2, "-b", true),
        ];
        for (item, key) in items.iter().zip([10, 20, 30, 40]) {
            evidence.record(item, key);
        }
        evidence.adjacent(10, 20, false);
        evidence.adjacent(30, 40, false);

        assert_eq!(
            evidence.resolve(&items, &[true; 4]),
            vec![
                mant_ir::DeclarationGroup {
                    start_item: 0,
                    end_item: 2,
                },
                mant_ir::DeclarationGroup {
                    start_item: 2,
                    end_item: 4,
                },
            ]
        );
    }

    #[test]
    fn a_lost_same_coordinate_owner_does_not_rebind_a_later_macro_expansion() {
        let mut evidence = GroupEvidence::default();
        let native_items = [
            item(12, 2, "-a", false),
            item(12, 2, "-b", true),
            item(12, 2, "-a", false),
            item(12, 2, "-b", true),
        ];
        for (item, key) in native_items.iter().zip([10, 20, 30, 40]) {
            evidence.record(item, key);
        }
        evidence.adjacent(10, 20, false);
        evidence.adjacent(30, 40, false);

        // A final rewrite retained only the latter source occurrence. The
        // remaining coordinate/head pair is indistinguishable, so it must
        // not be silently attached to the first native run.
        let final_items = vec![item(12, 2, "-a", false), item(12, 2, "-b", true)];
        assert!(evidence.resolve(&final_items, &[true; 2]).is_empty());
    }

    #[test]
    fn a_body_closed_native_owner_allows_the_next_physical_run() {
        let mut evidence = GroupEvidence::default();
        let items = vec![
            item(1, 1, "--first", false),
            item(2, 1, "--second", true),
            item(4, 1, "--third", false),
            item(5, 1, "--fourth", true),
        ];
        for (item, key) in items.iter().zip([10, 20, 40, 50]) {
            evidence.record(item, key);
        }
        evidence.adjacent(10, 20, false);
        // Key 30 is a native ordinal/bullet owner converted to Block::List.
        // Its readable body closes that run before --third starts.
        evidence.adjacent(30, 40, true);
        evidence.adjacent(40, 50, false);

        assert_eq!(
            evidence.resolve(&items, &[true; 4]),
            vec![
                mant_ir::DeclarationGroup {
                    start_item: 0,
                    end_item: 2,
                },
                mant_ir::DeclarationGroup {
                    start_item: 2,
                    end_item: 4,
                },
            ]
        );
    }
}
