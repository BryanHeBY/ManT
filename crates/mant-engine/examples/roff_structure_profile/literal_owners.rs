//! Match proven native display occurrences to sourced IR literal blocks.

use std::collections::{BTreeMap, BTreeSet};

use libmandoc_rs::{DisplayKind, Node, NodeKind, NormalizedListKind};
use mant_ir::{Block, Document, Section, SourceSpan};

use super::{
    AstStructure,
    native_topology::column_data_items,
    ordinal_labels::{
        SourceContext,
        description::{description, source_evidence},
        skips_source_children,
    },
};

#[path = "literal_owners/matching.rs"]
mod matching;
#[path = "literal_owners/rows.rs"]
mod rows;
use rows::{CellPlan, cell_plan, display_rows};

pub(super) use matching::compare;

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
    ColumnTable,
    ColumnCell { row: usize, cell: usize },
}

impl ScopeKind {
    const fn table_scope(self) -> bool {
        matches!(self, Self::ColumnTable | Self::ColumnCell { .. })
    }
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
    id: u32,
    opening: Point,
    origins: BTreeSet<LiteralOrigin>,
    path: Vec<ScopeOccurrence>,
    rows: usize,
    short: bool,
    pending_rows: BTreeMap<u32, String>,
}

#[derive(Default)]
pub(super) struct SourceDisplays {
    owners: Vec<SourceDisplay>,
    scopes: BTreeSet<Scope>,
    counts: ScopeCounts,
    cells: BTreeMap<Vec<ScopeOccurrence>, CellPlan>,
    source_rows: Vec<(usize, String)>,
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
    if node.kind == NodeKind::Block && node.list_kind == Some(NormalizedListKind::Column) {
        visit_column_list(node, path, profile);
        return;
    }
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
        enter_native_scope(profile, path, Scope { point, kind });
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
                let pending_rows = display_rows(node, &profile.source_context).unwrap_or_default();
                display = Some(profile.literal_owners.owners.len());
                profile.literal_owners.owners.push(SourceDisplay {
                    id: node.id,
                    opening: point,
                    origins: BTreeSet::new(),
                    path: path.clone(),
                    rows: pending_rows.len(),
                    short: node.macro_token.as_deref() != Some("Bd"),
                    pending_rows,
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
        let literals = &mut profile.literal_owners;
        let source = &mut literals.owners[owner];
        source.origins.insert(LiteralOrigin {
            path: path.clone(),
            point,
        });
        if let Some(text) = source.pending_rows.remove(&node.id) {
            literals.source_rows.push((owner, text));
        }
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

fn enter_native_scope(profile: &mut AstStructure, path: &mut Vec<ScopeOccurrence>, scope: Scope) {
    profile.literal_owners.scopes.insert(scope);
    enter_scope(path, scope, &mut profile.literal_owners.counts);
}

fn visit_column_list(node: &Node, path: &mut Vec<ScopeOccurrence>, profile: &mut AstStructure) {
    // CVS mdoc_it_pre() emits one <tr> per It and one <td> per BODY;
    // termp_it_pre() likewise uses the preceding BODY siblings as columns.
    let point = (node.line, node.column);
    enter_native_scope(
        profile,
        path,
        Scope {
            point,
            kind: ScopeKind::ColumnTable,
        },
    );
    for (row, item) in column_data_items(node).enumerate() {
        for (cell, body) in item
            .children
            .iter()
            .filter(|part| part.kind == NodeKind::Body && part.scope_end.is_none())
            .enumerate()
        {
            enter_native_scope(
                profile,
                path,
                Scope {
                    point,
                    kind: ScopeKind::ColumnCell { row, cell },
                },
            );
            let first_owner = profile.literal_owners.owners.len();
            visit_native(body, path, None, profile);
            let owners = &profile.literal_owners.owners[first_owner..];
            if owners.iter().any(|owner| &owner.path == path) {
                let plan = cell_plan(body, &profile.source_context, owners, first_owner, path);
                if !plan.complete {
                    // A verified prefix can disprove a missing row, but
                    // later unclassified text in this same cell could
                    // supply an equal replacement after a deletion.
                    for owner in owners.iter().filter(|owner| &owner.path == path) {
                        profile
                            .literal_display_reviews
                            .push([owner.opening.0, owner.opening.1]);
                    }
                }
                profile.literal_owners.cells.insert(path.clone(), plan);
            }
            path.pop();
        }
    }
    path.pop();
}

fn has_glyph(text: &str) -> bool {
    text.chars()
        .any(|ch| !ch.is_ascii_whitespace() && !ch.is_control())
}

fn normalized_row(text: &str) -> String {
    text.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Default)]
pub(super) struct ObservedDisplays {
    // Parents are registered before descendants. An empty item still
    // occupies its scope occurrence, even if it no longer contains a block.
    scopes: Vec<(Option<usize>, Scope)>,
    blocks: Vec<ProjectedBlock>,
}

struct ProjectedBlock {
    parent: Option<usize>,
    point: Option<Point>,
    literal: bool,
    rows: Vec<String>,
}

impl ObservedDisplays {
    fn enter_scope(
        &mut self,
        parent: Option<usize>,
        source: Option<SourceSpan>,
        kind: ScopeKind,
    ) -> Option<usize> {
        let Some(point) = source
            .map(point)
            .or_else(|| kind.table_scope().then_some((0, 0)))
        else {
            return parent;
        };
        let index = self.scopes.len();
        self.scopes.push((parent, Scope { point, kind }));
        Some(index)
    }

    fn matching_blocks(&self, native_scopes: &BTreeSet<Scope>) -> Vec<ObservedBlock<'_>> {
        fn path(paths: &[Vec<ScopeOccurrence>], index: Option<usize>) -> Vec<ScopeOccurrence> {
            index.map_or_else(Vec::new, |index| paths[index].clone())
        }

        let mut paths = Vec::with_capacity(self.scopes.len());
        let mut counts = ScopeCounts::new();
        for (parent, scope) in &self.scopes {
            let mut path = path(&paths, *parent);
            // Inferred definitions need not be native scopes, but an
            // explicit table or field always remains a barrier.
            if scope.kind.table_scope() || native_scopes.contains(scope) {
                enter_scope(&mut path, *scope, &mut counts);
            }
            paths.push(path);
        }
        self.blocks
            .iter()
            .map(|block| ObservedBlock {
                path: path(&paths, block.parent),
                point: block.point,
                literal: block.literal,
                rows: &block.rows,
            })
            .collect()
    }
}

struct ObservedBlock<'a> {
    path: Vec<ScopeOccurrence>,
    point: Option<Point>,
    literal: bool,
    rows: &'a [String],
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
                children, source, ..
            }
            | Block::Paragraph {
                children, source, ..
            } => {
                let literal = matches!(block, Block::Preformatted { .. });
                if !literal
                    && !parent.is_some_and(|p| {
                        matches!(observations.scopes[p].1.kind, ScopeKind::ColumnCell { .. })
                    })
                {
                    continue;
                }
                let text = mant_ir::inline_plain_text(children);
                let in_cell = parent.is_some_and(|p| {
                    matches!(observations.scopes[p].1.kind, ScopeKind::ColumnCell { .. })
                });
                if has_glyph(&text)
                    || in_cell
                        && (text.contains('\n')
                            || literal && mant_ir::geometry::has_literal_rows(children))
                {
                    observations.blocks.push(ProjectedBlock {
                        parent,
                        point: source.map(point),
                        literal,
                        rows: text.split('\n').map(normalized_row).collect(),
                    });
                }
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
            Block::Table { rows, source, .. } => {
                let anchor = *source;
                let parent = observations.enter_scope(parent, anchor, ScopeKind::ColumnTable);
                for (row, data) in rows.iter().enumerate() {
                    for (cell, field) in data.cells.iter().enumerate() {
                        let parent = observations.enter_scope(
                            parent,
                            anchor,
                            ScopeKind::ColumnCell { row, cell },
                        );
                        visit_blocks(&field.blocks, parent, observations);
                    }
                }
            }
            _ => {}
        }
    }
}
