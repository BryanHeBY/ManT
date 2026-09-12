//! Native adjacency witnesses survive normalization without guessing from IR
//! indentation or moving another item's description into a semantic owner.
use super::evidence::head_content;
use mant_ir::{Block, DeclarationGroup, DefinitionItem, Inline, Section, SourceSpan};
use std::collections::{HashMap, HashSet};

struct Witness {
    source: SourceSpan,
    head: Vec<Vec<Inline>>,
    key: usize,
    last_key: usize,
}

/// A native owner allocation plan built after normalization and consumed in
/// that same structural traversal.  Source coordinates are not identities:
/// macro expansion may legitimately duplicate them.  The plan first proves
/// that every duplicate survived, then assigns the recorded native pointers
/// in order without rescanning each final definition list.
#[derive(Default)]
pub(crate) struct GroupMatchingPlan {
    classes: HashMap<(u32, u32), Vec<MatchClass>>,
}

struct MatchClass {
    source: SourceSpan,
    head: Vec<Vec<Inline>>,
    owners: Vec<(usize, usize)>,
    observed: usize,
    next: usize,
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
        for (coordinate, witnesses) in &self.items {
            let classes = plan.classes.entry(*coordinate).or_default();
            for witness in witnesses {
                if let Some(class) = classes
                    .iter_mut()
                    .find(|class| class.source == witness.source && class.head == witness.head)
                {
                    class.owners.push((witness.key, witness.last_key));
                } else {
                    classes.push(MatchClass {
                        source: witness.source,
                        head: witness.head.clone(),
                        owners: vec![(witness.key, witness.last_key)],
                        observed: 0,
                        next: 0,
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
        let head = head_content(&item.terms);
        let coordinate = (source.line, source.column);
        let class = plan
            .classes
            .get_mut(&coordinate)?
            .iter_mut()
            .find(|class| class.source == source && class.head == head)?;
        // Do not let a survivor of an incomplete duplicate stream borrow the
        // identity of a removed sibling.  The count was calculated over the
        // complete normalized document, not merely this definition list.
        if class.observed != class.owners.len() {
            return None;
        }
        let owner = *class.owners.get(class.next)?;
        class.next += 1;
        Some(owner)
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
        let Some(source) = item.source else { return };
        let head = head_content(&item.terms);
        if let Some(class) = self
            .classes
            .get_mut(&(source.line, source.column))
            .and_then(|classes| {
                classes
                    .iter_mut()
                    .find(|class| class.source == source && class.head == head)
            })
        {
            class.observed = class.observed.saturating_add(1);
        }
    }
}

// These witnesses are populated only by the native roff lowering path.  Keep
// their regression matrix out of the Markdown-only build, which deliberately
// omits the roff-only recording API.
#[cfg(all(test, feature = "roff"))]
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

    fn plan(evidence: &GroupEvidence, items: &[DefinitionItem]) -> super::GroupMatchingPlan {
        let blocks = vec![Block::DefinitionList {
            items: items.to_vec(),
            declaration_groups: Vec::new(),
            compact: false,
            source: None,
            layout: LayoutHint::default(),
        }];
        evidence.matching_plan(&blocks, &[])
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

        let mut plan = plan(&evidence, &items);
        assert_eq!(
            evidence.resolve(&items, &[false, true, true], &mut plan),
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

        let mut plan = plan(&evidence, &items);
        assert_eq!(
            evidence.resolve(&items, &[true; 4], &mut plan),
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
        let mut plan = plan(&evidence, &final_items);
        assert!(
            evidence
                .resolve(&final_items, &[true; 2], &mut plan)
                .is_empty()
        );
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

        let mut plan = plan(&evidence, &items);
        assert_eq!(
            evidence.resolve(&items, &[true; 4], &mut plan),
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
    fn an_unclassified_head_with_its_own_body_closes_the_previous_run() {
        let mut evidence = GroupEvidence::default();
        let items = vec![
            item(1, 1, "This is explanatory prose.", true),
            item(2, 1, "--alpha", false),
            item(3, 1, "--beta", true),
        ];
        for (item, key) in items.iter().zip([10, 20, 30]) {
            evidence.record(item, key);
        }
        evidence.adjacent(10, 20, true);
        evidence.adjacent(20, 30, false);

        let mut plan = plan(&evidence, &items);
        assert_eq!(
            evidence.resolve(&items, &[false, true, true], &mut plan),
            vec![mant_ir::DeclarationGroup {
                start_item: 1,
                end_item: 3,
            }]
        );
    }

    #[test]
    fn an_empty_unclassified_head_still_blocks_a_following_suffix() {
        let mut evidence = GroupEvidence::default();
        let items = vec![
            item(1, 1, "unclassified", false),
            item(2, 1, "--alpha", false),
            item(3, 1, "--beta", true),
        ];
        for (item, key) in items.iter().zip([10, 20, 30]) {
            evidence.record(item, key);
        }
        evidence.adjacent(10, 20, false);
        evidence.adjacent(20, 30, false);

        let mut plan = plan(&evidence, &items);
        assert!(
            evidence
                .resolve(&items, &[false, true, true], &mut plan)
                .is_empty()
        );
    }

    #[test]
    fn macro_expansion_can_span_definition_lists_without_losing_owner_identity() {
        let mut evidence = GroupEvidence::default();
        let first = vec![item(12, 2, "--alpha", false), item(12, 2, "--beta", true)];
        let second = first.clone();
        for (item, key) in first.iter().chain(&second).zip([10, 20, 30, 40]) {
            evidence.record(item, key);
        }
        evidence.adjacent(10, 20, false);
        evidence.adjacent(30, 40, false);
        let blocks = vec![
            Block::DefinitionList {
                items: first.clone(),
                declaration_groups: Vec::new(),
                compact: false,
                source: None,
                layout: LayoutHint::default(),
            },
            Block::Paragraph {
                children: vec![Inline::Text {
                    value: "ordinary separator".to_owned(),
                }],
                source: None,
                layout: LayoutHint::default(),
            },
            Block::DefinitionList {
                items: second.clone(),
                declaration_groups: Vec::new(),
                compact: false,
                source: None,
                layout: LayoutHint::default(),
            },
        ];
        let mut plan = evidence.matching_plan(&blocks, &[]);
        let expected = vec![mant_ir::DeclarationGroup {
            start_item: 0,
            end_item: 2,
        }];
        assert_eq!(evidence.resolve(&first, &[true, true], &mut plan), expected);
        assert_eq!(
            evidence.resolve(&second, &[true, true], &mut plan),
            vec![mant_ir::DeclarationGroup {
                start_item: 0,
                end_item: 2,
            }]
        );
    }
}
