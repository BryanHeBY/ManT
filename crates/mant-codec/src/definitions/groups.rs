//! Native adjacency witnesses survive normalization without guessing from IR
//! indentation or moving another item's description into a semantic owner.
use super::evidence::head_content;
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
    term.insert(0, Inline::anchor(format!("{OWNER_MARKER_PREFIX}{key:x}")));
}

pub(crate) fn is_internal_definition_owner_marker(id: &str) -> bool {
    id.strip_prefix(OWNER_MARKER_PREFIX).is_some_and(|value| {
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn owner_marker(item: &DefinitionItem) -> Option<usize> {
    item.terms
        .iter()
        .flat_map(|term| term.iter())
        .find_map(|inline| {
            let Inline::Anchor { id, .. } = inline else {
                return None;
            };
            id.as_str()
                .strip_prefix(OWNER_MARKER_PREFIX)
                .filter(|value| {
                    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                .and_then(|value| usize::from_str_radix(value, 16).ok())
        })
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
            | Inline::Anchor { .. }
            | Inline::LineBreak => {}
        }
        retained.push(inline);
    }
    *inlines = retained;
}

pub(crate) fn remove_native_definition_owner_markers_from_items(items: &mut [DefinitionItem]) {
    for item in items {
        for term in &mut item.terms {
            remove_inlines(term);
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
    head: Vec<Vec<Inline>>,
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
    head: Vec<Vec<Inline>>,
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
        let head = head_content(&item.terms);
        self.items
            .entry((source.line, source.column))
            .or_default()
            .push(Witness {
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
        let head = head_content(&item.terms);
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
            .find(|binding| binding.source == source && binding.head == head)
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
mod tests {
    use super::{GroupEvidence, mark_native_definition_owner};
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

    fn record(evidence: &mut GroupEvidence, items: &mut [DefinitionItem], keys: &[usize]) {
        for (item, key) in items.iter_mut().zip(keys) {
            mark_native_definition_owner(item, *key);
            evidence.record(item, *key);
        }
    }

    #[test]
    fn presentation_heads_split_but_do_not_poison_a_named_suffix_group() {
        let mut evidence = GroupEvidence::default();
        let mut items = vec![
            item(1, 1, "/RE", false),
            item(2, 1, "?RE", false),
            item(3, 1, "n", true),
        ];
        record(&mut evidence, &mut items, &[10, 20, 30]);
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
        let mut items = vec![
            item(12, 2, "-a", false),
            item(12, 2, "-b", true),
            item(12, 2, "-a", false),
            item(12, 2, "-b", true),
        ];
        record(&mut evidence, &mut items, &[10, 20, 30, 40]);
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
    fn a_retained_same_coordinate_owner_keeps_its_own_macro_expansion() {
        let mut evidence = GroupEvidence::default();
        let mut native_items = [
            item(12, 2, "-a", false),
            item(12, 2, "-b", true),
            item(12, 2, "-a", false),
            item(12, 2, "-b", true),
        ];
        record(&mut evidence, &mut native_items, &[10, 20, 30, 40]);
        evidence.adjacent(10, 20, false);
        evidence.adjacent(30, 40, false);

        // A final rewrite retained only the latter source occurrence. The
        // coordinate/head pair is indistinguishable, but its carried owner
        // marker still selects the latter native run rather than borrowing
        // the first one.
        // Keep only the latter invocation's own markers. Same source
        // coordinates and heads must never make it borrow the first run.
        let mut final_items = vec![item(12, 2, "-a", false), item(12, 2, "-b", true)];
        mark_native_definition_owner(&mut final_items[0], 30);
        mark_native_definition_owner(&mut final_items[1], 40);
        let mut plan = plan(&evidence, &final_items);
        assert_eq!(
            evidence.resolve(&final_items, &[true; 2], &mut plan),
            vec![mant_ir::DeclarationGroup {
                start_item: 0,
                end_item: 2,
            }]
        );
    }

    #[test]
    fn duplicated_owner_marker_refuses_to_allocate_a_group() {
        let mut evidence = GroupEvidence::default();
        let mut items = vec![item(12, 2, "--alpha", false), item(12, 2, "--beta", true)];
        record(&mut evidence, &mut items, &[10, 20]);
        evidence.adjacent(10, 20, false);

        let mut duplicate = items.clone();
        // A normalization bug that cloned the same owner must remain safe:
        // no duplicated marker may claim one native declaration witness.
        let mut blocks = vec![Block::DefinitionList {
            items: duplicate.clone(),
            declaration_groups: Vec::new(),
            compact: false,
            source: None,
            layout: LayoutHint::default(),
        }];
        blocks.push(Block::DefinitionList {
            items: std::mem::take(&mut duplicate),
            declaration_groups: Vec::new(),
            compact: false,
            source: None,
            layout: LayoutHint::default(),
        });
        let mut plan = evidence.matching_plan(&blocks, &[]);
        let Block::DefinitionList { items, .. } = &blocks[0] else {
            unreachable!();
        };
        assert!(evidence.resolve(items, &[true; 2], &mut plan).is_empty());
    }

    #[test]
    fn a_body_closed_native_owner_allows_the_next_physical_run() {
        let mut evidence = GroupEvidence::default();
        let mut items = vec![
            item(1, 1, "--first", false),
            item(2, 1, "--second", true),
            item(4, 1, "--third", false),
            item(5, 1, "--fourth", true),
        ];
        record(&mut evidence, &mut items, &[10, 20, 40, 50]);
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
        let mut items = vec![
            item(1, 1, "This is explanatory prose.", true),
            item(2, 1, "--alpha", false),
            item(3, 1, "--beta", true),
        ];
        record(&mut evidence, &mut items, &[10, 20, 30]);
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
        let mut items = vec![
            item(1, 1, "unclassified", false),
            item(2, 1, "--alpha", false),
            item(3, 1, "--beta", true),
        ];
        record(&mut evidence, &mut items, &[10, 20, 30]);
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
        let mut first = vec![item(12, 2, "--alpha", false), item(12, 2, "--beta", true)];
        let mut second = first.clone();
        record(&mut evidence, &mut first, &[10, 20]);
        record(&mut evidence, &mut second, &[30, 40]);
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

    #[test]
    fn nested_preparation_cannot_swap_same_source_macro_owners() {
        let mut evidence = GroupEvidence::default();
        let mut outer = vec![item(12, 2, "--alpha", false), item(13, 2, "--beta", false)];
        let mut inner = vec![item(12, 2, "--alpha", false), item(13, 2, "--beta", true)];
        record(&mut evidence, &mut outer, &[10, 20]);
        record(&mut evidence, &mut inner, &[30, 40]);
        // The outer expansion has an executed paragraph boundary between its
        // heads. Only the nested expansion owns the shared description.
        evidence.adjacent(30, 40, false);

        outer[1].description.push(Block::DefinitionList {
            items: inner.clone(),
            declaration_groups: Vec::new(),
            compact: false,
            source: None,
            layout: LayoutHint::default(),
        });
        let blocks = vec![Block::DefinitionList {
            items: outer.clone(),
            declaration_groups: Vec::new(),
            compact: false,
            source: None,
            layout: LayoutHint::default(),
        }];
        let mut plan = evidence.matching_plan(&blocks, &[]);

        // Preparation descends into a definition body before resolving its
        // containing list. A FIFO allocation would give this inner pair the
        // outer witnesses because both expansions have identical coordinates
        // and heads. Parse-local markers make traversal order irrelevant.
        assert_eq!(
            evidence.resolve(&inner, &[true, true], &mut plan),
            vec![mant_ir::DeclarationGroup {
                start_item: 0,
                end_item: 2,
            }]
        );
        assert!(
            evidence
                .resolve(&outer, &[true, true], &mut plan)
                .is_empty()
        );
    }
}
