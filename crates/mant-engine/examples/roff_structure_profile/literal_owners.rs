//! Match proven native display occurrences to sourced IR literal blocks.

use std::collections::{BTreeMap, BTreeSet};

use libmandoc_rs::{DisplayKind, Node, NodeKind};
use mant_ir::{Block, Document, Section, SourceSpan};

use super::{
    AstStructure,
    ir::has_visible_inline,
    ordinal_labels::{description::description, skips_source_children},
};

type Point = (u32, u32);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Scope {
    point: Point,
    kind: ScopeKind,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ScopeKind {
    Section,
    Item,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ScopeOccurrence {
    scope: Scope,
    occurrence: usize,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LiteralOrigin {
    path: Vec<ScopeOccurrence>,
    point: Point,
}

struct SourceDisplay {
    opening: Point,
    origins: BTreeSet<LiteralOrigin>,
}

#[derive(Default)]
pub(super) struct SourceDisplays {
    owners: Vec<SourceDisplay>,
    scopes: BTreeSet<Scope>,
    counts: ScopeCounts,
}

type ScopeCounts = BTreeMap<(Vec<ScopeOccurrence>, Scope), usize>;

fn enter_scope(path: &mut Vec<ScopeOccurrence>, scope: Scope, counts: &mut ScopeCounts) {
    let occurrence = counts.entry((path.clone(), scope)).or_default();
    *occurrence += 1;
    path.push(ScopeOccurrence {
        scope,
        occurrence: *occurrence,
    });
}

pub(super) fn record_source_displays(root: &Node, profile: &mut AstStructure) {
    visit_native(root, &mut Vec::new(), None, profile);
    profile.literal_displays = profile.literal_owners.owners.len();
}

fn visit_native(
    node: &Node,
    path: &mut Vec<ScopeOccurrence>,
    mut display: Option<usize>,
    profile: &mut AstStructure,
) {
    if skips_source_children(node)
        || node.kind == NodeKind::Head && node.macro_token.as_deref() == Some("It")
    {
        // Definition terms project as their own inline roots rather than literal BODYs.
        return;
    }
    let point = (node.line, node.column);
    let kind = if node.kind == NodeKind::Block {
        match node.macro_token.as_deref() {
            Some("Sh" | "Ss") => Some(ScopeKind::Section),
            Some("It") => Some(ScopeKind::Item),
            _ => None,
        }
    } else {
        None
    };
    if let Some(kind) = kind {
        let scope = Scope { point, kind };
        profile.literal_owners.scopes.insert(scope);
        enter_scope(path, scope, &mut profile.literal_owners.counts);
    }
    if node.kind == NodeKind::Block
        && matches!(node.macro_token.as_deref(), Some("D1" | "Dl" | "Bd"))
    {
        // A nested display has its own output owner even when an ancestor
        // literal projects more blocks after the nested scope returns.
        display = None;
        if node.macro_token.as_deref() != Some("Bd")
            || matches!(
                node.display_kind,
                Some(DisplayKind::Literal | DisplayKind::Unfilled)
            )
        {
            let payload = description(node, false, &profile.source_context);
            if payload.has_text() {
                display = Some(profile.literal_owners.owners.len());
                profile.literal_owners.owners.push(SourceDisplay {
                    opening: point,
                    origins: BTreeSet::new(),
                });
            } else if payload.unproved_literal() {
                profile
                    .literal_display_reviews
                    .push([node.line, node.column]);
            }
        }
    }
    if node.line != 0
        && let Some(owner) = display
    {
        // A block can start at a generated macro word or at the first
        // emitted descendant, including after a separately owned block.
        profile.literal_owners.owners[owner]
            .origins
            .insert(LiteralOrigin {
                path: path.clone(),
                point,
            });
    }
    if node.kind != NodeKind::Table {
        for child in &node.children {
            visit_native(child, path, display, profile);
        }
    }
    if kind.is_some() {
        path.pop();
    }
}

#[derive(Default)]
pub(super) struct ObservedDisplays {
    // Parents are registered before descendants. An empty item still
    // occupies its scope occurrence, even if it no longer contains a block.
    scopes: Vec<(Option<usize>, Scope)>,
    blocks: Vec<(Option<usize>, Point)>,
}

impl ObservedDisplays {
    fn enter_scope(
        &mut self,
        parent: Option<usize>,
        source: Option<SourceSpan>,
        kind: ScopeKind,
    ) -> Option<usize> {
        let Some(source) = source else {
            return parent;
        };
        let index = self.scopes.len();
        self.scopes.push((
            parent,
            Scope {
                point: point(source),
                kind,
            },
        ));
        Some(index)
    }

    fn matching_origins(&self, native_scopes: &BTreeSet<Scope>) -> Vec<LiteralOrigin> {
        fn path(paths: &[Vec<ScopeOccurrence>], index: Option<usize>) -> Vec<ScopeOccurrence> {
            index.map_or_else(Vec::new, |index| paths[index].clone())
        }

        let mut paths = Vec::with_capacity(self.scopes.len());
        let mut counts = ScopeCounts::new();
        for (parent, scope) in &self.scopes {
            let mut path = path(&paths, *parent);
            // Semantic inference can group native blocks into an additional
            // definition. It does not change their native display ancestry.
            if native_scopes.contains(scope) {
                enter_scope(&mut path, *scope, &mut counts);
            }
            paths.push(path);
        }
        self.blocks
            .iter()
            .map(|(parent, point)| LiteralOrigin {
                path: path(&paths, *parent),
                point: *point,
            })
            .collect()
    }
}

pub(super) fn record_ir_displays(document: &Document) -> ObservedDisplays {
    let mut observations = ObservedDisplays::default();
    visit_blocks(&document.blocks, None, &mut observations);
    for section in &document.sections {
        visit_section(section, None, &mut observations);
    }
    observations
}

fn point(span: SourceSpan) -> Point {
    (span.line, span.column)
}

fn visit_section(section: &Section, parent: Option<usize>, observations: &mut ObservedDisplays) {
    let parent = observations.enter_scope(parent, section.source, ScopeKind::Section);
    visit_blocks(&section.blocks, parent, observations);
    for child in &section.children {
        visit_section(child, parent, observations);
    }
}

fn visit_blocks(blocks: &[Block], parent: Option<usize>, observations: &mut ObservedDisplays) {
    for block in blocks {
        match block {
            Block::Preformatted {
                children,
                source: Some(source),
                ..
            } if has_visible_inline(children) => {
                observations.blocks.push((parent, point(*source)));
            }
            Block::List { items, .. } => {
                for item in items {
                    let parent = observations.enter_scope(parent, item.source, ScopeKind::Item);
                    visit_blocks(&item.blocks, parent, observations);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    let parent = observations.enter_scope(parent, item.source, ScopeKind::Item);
                    visit_blocks(&item.description, parent, observations);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    visit_blocks(&cell.blocks, parent, observations);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn compare(
    expected: &SourceDisplays,
    observed: &ObservedDisplays,
    violations: &mut Vec<String>,
) {
    let mut blocks = BTreeMap::<_, Blocks>::new();
    for origin in observed.matching_origins(&expected.scopes) {
        blocks.entry(origin).or_default().available += 1;
    }
    let expected = &expected.owners;
    let mut claims = vec![None; expected.len()];
    let mut occurrences = BTreeMap::<Point, usize>::new();
    for (index, owner) in expected.iter().enumerate() {
        let occurrence = occurrences.entry(owner.opening).or_default();
        *occurrence += 1;
        if !match_block(index, expected, &mut blocks, &mut claims) {
            let (line, column) = owner.opening;
            violations.push(format!(
                "literal-displays: missing sourced block for {line}:{column} occurrence {occurrence}"
            ));
        }
    }
}

#[derive(Default)]
struct Blocks {
    available: usize,
    owners: BTreeSet<usize>,
}

fn match_block(
    owner: usize,
    expected: &[SourceDisplay],
    blocks: &mut BTreeMap<LiteralOrigin, Blocks>,
    claims: &mut [Option<LiteralOrigin>],
) -> bool {
    // Equal positions occur in user macro expansions. Each block can pay
    // once. If more than one point fits an owner, look for an alternate
    // assignment without allowing a neighboring display to pay twice.
    let mut pending = vec![owner];
    let mut paths = BTreeMap::new();
    while let Some(owner) = pending.pop() {
        for origin in &expected[owner].origins {
            let Some(block) = blocks.get(origin) else {
                continue;
            };
            let std::collections::btree_map::Entry::Vacant(path) = paths.entry(origin.clone())
            else {
                continue;
            };
            path.insert(owner);
            if block.available == 0 {
                pending.extend(block.owners.iter().copied());
                continue;
            }
            let mut open = Some(origin.clone());
            while let Some(origin) = open {
                let owner = paths[&origin];
                let block = blocks.get_mut(&origin).expect("matched literal origin");
                block.available -= 1;
                block.owners.insert(owner);
                open = claims[owner].replace(origin);
                if let Some(previous) = &open {
                    let old = blocks.get_mut(previous).expect("prior literal origin");
                    old.available += 1;
                    old.owners.remove(&owner);
                }
            }
            return true;
        }
    }
    false
}
