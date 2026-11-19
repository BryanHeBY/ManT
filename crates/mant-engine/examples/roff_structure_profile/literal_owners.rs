//! Match proven native display occurrences to sourced IR literal blocks.

use std::collections::{BTreeMap, BTreeSet};

use libmandoc_rs::{DisplayKind, Node, NodeKind, NormalizedListKind};
use mant_ir::{Block, Document, Section, SourceSpan};

use super::{
    AstStructure,
    native_topology::column_data_items,
    ordinal_labels::{
        SourceContext,
        description::{description, source_evidence, word_payload},
        skips_source_children,
    },
};

#[path = "literal_owners/matching.rs"]
mod matching;

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
    opening: Point,
    origins: BTreeSet<LiteralOrigin>,
    path: Vec<ScopeOccurrence>,
    rows: usize,
    pending_rows: BTreeMap<u32, String>,
}

#[derive(Default)]
pub(super) struct SourceDisplays {
    owners: Vec<SourceDisplay>,
    scopes: BTreeSet<Scope>,
    counts: ScopeCounts,
    cell_prefixes: BTreeMap<Vec<ScopeOccurrence>, (Point, String)>,
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
                let pending_rows = plain_display_rows(node, &profile.source_context);
                display = Some(profile.literal_owners.owners.len());
                profile.literal_owners.owners.push(SourceDisplay {
                    opening: point,
                    origins: BTreeSet::new(),
                    path: path.clone(),
                    rows: pending_rows.len(),
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
            if let Some(first) = body.children.first()
                && let Some(prefix) = plain_source_row(first)
            {
                profile
                    .literal_owners
                    .cell_prefixes
                    .insert(path.clone(), ((first.line, first.column), prefix));
            }
            visit_native(body, path, None, profile);
            path.pop();
        }
    }
    path.pop();
}

fn plain_display_rows(node: &Node, source: &SourceContext) -> BTreeMap<u32, String> {
    // A literal row of plain authored bytes has an independent reading
    // witness. Keep field rewrites and generated macro words under the
    // separate source execution proof. Do not guess their IR spelling.
    let mut rows = BTreeMap::new();
    for child in node
        .children
        .iter()
        .filter(|part| part.kind == NodeKind::Body)
        .flat_map(|body| &body.children)
    {
        if let Some(row) = plain_source_row(child).or_else(|| inline_source_row(child, source)) {
            if !rows.is_empty() && !child.flags.line_start {
                return BTreeMap::new();
            }
            rows.insert(child.id, row);
        } else if child.kind == NodeKind::Block
            && matches!(
                child.macro_token.as_deref(),
                Some("D1" | "Dl" | "Bd" | "Bl")
            )
            || matches!(child.macro_token.as_deref(), Some("br" | "Pp"))
        {
            // These scopes or requests terminate the active text field.
        } else {
            return BTreeMap::new();
        }
    }
    // mdoc_macro.c puts trailing delimiters after the short display BODY,
    // and print_mdoc_node() glues them to the preceding word before post.
    if matches!(node.macro_token.as_deref(), Some("D1" | "Dl"))
        && let Some(mut row) = rows.last_entry()
    {
        let tail = node
            .children
            .iter()
            .skip_while(|part| part.kind != NodeKind::Body)
            .skip(1);
        for child in tail {
            if !child.flags.delimiter_close {
                return BTreeMap::new();
            }
            let Some(word) = plain_source_row(child) else {
                return BTreeMap::new();
            };
            row.get_mut().push_str(&word);
        }
    }
    rows
}

fn inline_source_row(node: &Node, source: &SourceContext) -> Option<String> {
    use mant_codec::RoffWordEffect::{EmittedBreak, Unclassified, WordEndBreak};
    // These pre handlers either select a font or generate known words.
    // Device-specific glyphs, inline geometry, and pretty synopsis
    // formatting stay on the source-position proof path.
    if node.kind != NodeKind::Element
        || node.flags.synopsis_pretty
        || !matches!(
            node.macro_token.as_deref(),
            Some("Em" | "Sy" | "Li" | "Ar" | "Cm" | "Ic" | "Fl" | "Fn" | "Xr")
        )
        || !node.children.iter().all(|child| {
            !child.flags.line_start
                && (plain_source_row(child).is_some()
                    || child.kind == NodeKind::Text && child.text.as_deref() == Some(r"\&"))
        })
    {
        return None;
    }
    let (prefix, word) = source_evidence(node, false, source, true);
    let word = word?;
    (prefix.absent()
        && !word.has(Unclassified)
        && !word.has(WordEndBreak)
        && !word.has(EmittedBreak)
        && word_payload(&word, true).has_text())
    .then(|| normalized_row(&word.text))
}

fn plain_source_row(node: &Node) -> Option<String> {
    let text = node.text.as_deref()?;
    (node.kind == NodeKind::Text
        && text
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'\\' || byte == b' ' || byte == b'\t')
        && has_glyph(text))
    .then(|| normalized_row(text))
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
    blocks: Vec<(Option<usize>, Option<Point>, Vec<String>)>,
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
            .map(|(parent, point, rows)| ObservedBlock {
                path: path(&paths, *parent),
                point: *point,
                rows,
            })
            .collect()
    }
}

struct ObservedBlock<'a> {
    path: Vec<ScopeOccurrence>,
    point: Option<Point>,
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
            } => {
                let text = mant_ir::inline_plain_text(children);
                if has_glyph(&text) {
                    let rows = text.split('\n').map(normalized_row).collect();
                    observations.blocks.push((parent, source.map(point), rows));
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
