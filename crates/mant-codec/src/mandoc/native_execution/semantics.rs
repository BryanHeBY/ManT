//! Bind native execution origins directly to source-neutral semantic owners.
//!
//! This private K19 route consumes native node, wrapper, atom, reference, and
//! anchor order. Device lines, rendered run boundaries, source coordinates,
//! pointers, slugs, and legacy marker anchors never select an owner.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::path::Path;

use libmandoc_rs::{
    AtomDisposition, AtomKind, AtomRole, ExecutionBoundary, ExecutionFont, ExecutionManBlockKind,
    ExecutionNodeKey, ExecutionReferenceKind, ExecutionReferencePresentation, MacroSet,
    NativeExecutionReport,
};
#[cfg(test)]
use mant_ir::DefinitionItem;
use mant_ir::{
    Block, ContentBlockStep, Document, DocumentMeta, DocumentSource, FragmentAlias, Heading,
    Inline, LayoutHint, LinkTarget, NodeId, ParserInfo, Section, SourceFormat, SourceSpan,
};

use super::{
    NativeAnchor, NativeBodySettlement, NativeHeadingFact, NativeHeadingKind, NativeProjection,
    NativeReference, native_node_wrapper_index,
};

mod bindings;
mod man;
mod materializer;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct NativeSemanticReceipt {
    pub(super) origins: Vec<ExecutionNodeKey>,
    pub(super) id: NodeId,
    pub(super) sections: Vec<u32>,
    pub(super) blocks: Vec<ContentBlockStep>,
    pub(super) item_index: u32,
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct NativeSemanticProjection {
    pub(super) document: Document,
    pub(super) receipts: Vec<NativeSemanticReceipt>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Location {
    Section { top: usize, child: Option<usize> },
}

#[derive(Default)]
struct SectionPlan {
    sections: Vec<Section>,
    authored: HashMap<String, Option<NodeId>>,
    identities: IdentityAllocator,
    authored_titles: HashMap<String, String>,
    fragment_candidates: Vec<(Location, String, ExecutionNodeKey)>,
    heading_nodes: HashSet<ExecutionNodeKey>,
    heading_locations: Vec<Location>,
}

impl SectionPlan {
    fn build(headings: &[NativeHeadingFact], report: &NativeExecutionReport) -> Self {
        let mut plan = Self::default();
        let mut current_top = None;
        for heading in headings {
            let subsection = matches!(
                heading.kind,
                NativeHeadingKind::ManSubsection | NativeHeadingKind::MdocSubsection
            );
            let authored = heading
                .authored_phrase
                .as_deref()
                .unwrap_or(heading.label.as_str());
            let id = plan
                .identities
                .allocate(&crate::producer_identity::section_id_base(authored));
            let source = source_span(report, heading.head);
            let first_top_level = plan.sections.is_empty();
            let section = Section {
                id: NodeId::new(id.clone()),
                fragment_aliases: Vec::new(),
                heading: Heading {
                    content: vec![Inline::Text {
                        value: heading.label.clone(),
                    }],
                    source,
                },
                spacing_before_lines: if subsection || !first_top_level {
                    heading.spacing_before_lines
                } else {
                    0
                },
                blocks: Vec::new(),
                children: Vec::new(),
                source,
            };
            let location = if subsection {
                let top = current_top.unwrap_or_else(|| {
                    plan.sections.push(Section {
                        id: NodeId::new("document"),
                        fragment_aliases: Vec::new(),
                        heading: Heading::from("Document"),
                        spacing_before_lines: 0,
                        blocks: Vec::new(),
                        children: Vec::new(),
                        source: None,
                    });
                    0
                });
                let child = plan.sections[top].children.len();
                plan.sections[top].children.push(section);
                Location::Section {
                    top,
                    child: Some(child),
                }
            } else {
                let top = plan.sections.len();
                plan.sections.push(section);
                current_top = Some(top);
                Location::Section { top, child: None }
            };
            plan.heading_locations.push(location);
            plan.authored_titles.insert(id.clone(), authored.to_owned());
            plan.heading_nodes.insert(heading.section);
            plan.heading_nodes.insert(heading.head);
            if let Some(fragment) = &heading.authored_fragment {
                plan.fragment_candidates
                    .push((location, fragment.clone(), heading.head));
            }
            match plan.authored.entry(authored.to_owned()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(NodeId::new(id)));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
        plan
    }

    fn section_mut(&mut self, location: Location) -> &mut Section {
        match location {
            Location::Section { top, child: None } => &mut self.sections[top],
            Location::Section {
                top,
                child: Some(child),
            } => &mut self.sections[top].children[child],
        }
    }
}

/// Native SH/Sh handlers add a presentation-only bold scope around the whole
/// heading.  Keep nested semantic styling and references, but remove that one
/// structural layer because every IR heading consumer already supplies its
/// own heading presentation.
fn remove_structural_heading_bold(inlines: &mut Vec<Inline>) {
    let mut output = Vec::new();
    for inline in std::mem::take(inlines) {
        match inline {
            Inline::Strong { mut children } => {
                remove_structural_heading_bold(&mut children);
                output.extend(children);
            }
            Inline::Emphasis { mut children } => {
                remove_structural_heading_bold(&mut children);
                output.push(Inline::Emphasis { children });
            }
            Inline::Link {
                target,
                title,
                mut children,
            } => {
                remove_structural_heading_bold(&mut children);
                output.push(Inline::Link {
                    target,
                    title,
                    children,
                });
            }
            other => output.push(other),
        }
    }
    *inlines = output;
}

#[derive(Clone, Default)]
struct IdentityAllocator {
    used: HashSet<String>,
    next_suffix: HashMap<String, u32>,
}

impl IdentityAllocator {
    fn allocate(&mut self, base: &str) -> String {
        let base = if base.is_empty() { "target" } else { base };
        if self.used.insert(base.to_owned()) {
            self.next_suffix.entry(base.to_owned()).or_insert(2);
            return base.to_owned();
        }
        let suffix = self.next_suffix.entry(base.to_owned()).or_insert(2);
        loop {
            let candidate = format!("{base}-{suffix}");
            *suffix = suffix.saturating_add(1);
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
        }
    }
}

fn source_span(report: &NativeExecutionReport, key: ExecutionNodeKey) -> Option<SourceSpan> {
    let node = &report.nodes()[key.0 as usize];
    (node.line > 0).then_some(SourceSpan {
        line: node.line,
        column: node.column.max(1),
        byte_range: None,
        end_line: None,
        end_column: None,
    })
}

fn section_coordinates(location: Location) -> Vec<u32> {
    match location {
        Location::Section { top, child: None } => vec![u32::try_from(top).unwrap()],
        Location::Section {
            top,
            child: Some(child),
        } => vec![u32::try_from(top).unwrap(), u32::try_from(child).unwrap()],
    }
}

fn reference_target(
    reference: &NativeReference,
    authored_sections: &HashMap<String, Option<NodeId>>,
) -> Option<LinkTarget> {
    let primary =
        super::super::inline::link_identity_text(&String::from_utf8_lossy(&reference.primary));
    if primary.is_empty()
        && matches!(
            reference.kind,
            ExecutionReferenceKind::ExternalUri | ExecutionReferenceKind::Email
        )
    {
        return None;
    }
    Some(match reference.kind {
        ExecutionReferenceKind::ExternalUri => LinkTarget::External { uri: primary },
        ExecutionReferenceKind::Email => LinkTarget::Email { address: primary },
        ExecutionReferenceKind::Manual => LinkTarget::Manual {
            name: primary,
            manual_section: reference.secondary.as_deref().map(|value| {
                super::super::inline::link_identity_text(&String::from_utf8_lossy(value))
            }),
        },
        ExecutionReferenceKind::SameDocumentSection => LinkTarget::Section {
            id: authored_sections.get(&primary)?.clone()?,
        },
    })
}

/// Plan the documented ManT presentation of the three portable mdoc `Bx`
/// lifecycle spellings without replaying any formatter state.
///
/// Fixed CVS `mdoc_validate.c::post_bx()` executes the authored operand first
/// and appends a generated `BSD` word in the same node wrapper.  The native
/// report is therefore authoritative for fonts, word-end breaks, zero-width
/// effects, and following-word geometry.  ManT only replaces the visible
/// glyphs when that executed wrapper contains exactly one authored formatter
/// word whose decoded identity is a lifecycle spelling; execution/layout
/// atoms inside the interval remain active.
fn bsd_replacements(
    report: &NativeExecutionReport,
    node_wrappers: &[Option<usize>],
    final_cells: &[Option<(u32, u32, i64, i64)>],
) -> (BTreeMap<u32, BsdReplacement>, Vec<bool>) {
    let mut replacements = BTreeMap::new();
    let mut replaced_atoms = vec![false; report.atoms().len()];
    for node in report
        .nodes()
        .iter()
        .filter(|node| node.macro_name.as_deref() == Some("Bx"))
    {
        let Some(wrapper_index) = node_wrappers[node.key.0 as usize] else {
            continue;
        };
        let wrapper = &report.wrappers()[wrapper_index];
        let authored = report
            .words()
            .iter()
            .filter(|word| {
                word.role == AtomRole::Authored
                    && wrapper.enter_atom <= word.atoms.start
                    && word.atoms.end <= wrapper.leave_atom
            })
            .collect::<Vec<_>>();
        let [word] = authored.as_slice() else {
            continue;
        };
        let identity = super::super::roff_escape::visible_text(&String::from_utf8_lossy(
            report.pool_bytes(word.operand).unwrap_or_default(),
        ));
        let value = match identity.as_str() {
            "-alpha" => "BSD (currently in alpha test)",
            "-beta" => "BSD (currently in beta test)",
            "-devel" => "BSD (currently under development)",
            _ => continue,
        };
        let range = wrapper.enter_atom..wrapper.leave_atom;
        let injection = range
            .clone()
            .find(|cursor| {
                final_cells[*cursor as usize].is_some()
                    || report.atoms()[*cursor as usize].kind == AtomKind::WordEndBreak
            })
            .unwrap_or(range.start);
        let font = range
            .clone()
            .rev()
            .filter_map(|cursor| report.atoms().get(cursor as usize))
            .find(|atom| atom.role == AtomRole::MacroGenerated && atom.kind == AtomKind::Glyph)
            .map_or(ExecutionFont::Roman, |atom| atom.font);
        replaced_atoms[range.start as usize..range.end as usize].fill(true);
        replacements.insert(injection, BsdReplacement { range, font, value });
    }
    (replacements, replaced_atoms)
}

fn native_target_identities(
    anchors: &[NativeAnchor],
    sections: &mut SectionPlan,
) -> HashMap<u32, (NodeId, Vec<FragmentAlias>)> {
    #[derive(Clone, Copy)]
    enum Owner {
        Section(Location),
        Anchor(u32),
    }

    let mut identities = sections.identities.clone();
    let mut output = HashMap::new();
    for anchor in anchors {
        if sections.heading_nodes.contains(&anchor.node) {
            continue;
        }
        let authored = String::from_utf8_lossy(&anchor.target).into_owned();
        let base = crate::definitions::document_id_slug(&authored);
        let base = format!("target-{base}");
        let id = identities.allocate(&base);
        output.insert(anchor.key, (NodeId::new(id), Vec::new()));
    }

    // Mirror html_make_id(): authored fragments share one document-order
    // namespace and repeated spellings receive ~N suffixes. Canonical IR IDs
    // use separate structural prefixes, so case-sensitive authored fragments
    // cannot collide with lower-cased NodeIds.
    let mut candidates = sections
        .fragment_candidates
        .iter()
        .map(|(location, fragment, node)| {
            (node.0, 0_u8, fragment.clone(), Owner::Section(*location))
        })
        .chain(
            anchors
                .iter()
                .filter(|anchor| output.contains_key(&anchor.key))
                .map(|anchor| {
                    (
                        anchor.node.0,
                        1_u8,
                        String::from_utf8_lossy(&anchor.target).into_owned(),
                        Owner::Anchor(anchor.key),
                    )
                }),
        )
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(cursor, order, _, _)| (*cursor, *order));
    let mut ordinals = HashMap::<String, u32>::new();
    for (_, _, authored, owner) in candidates {
        let base = super::heading_fragment(&authored);
        let ordinal = ordinals
            .entry(base.clone())
            .and_modify(|value| *value = value.saturating_add(1))
            .or_insert(1);
        let fragment = if *ordinal == 1 {
            base
        } else {
            format!("{base}~{ordinal}")
        };
        match owner {
            Owner::Section(location) => {
                let section = sections.section_mut(location);
                if fragment != section.id.as_str() {
                    section.fragment_aliases.push(FragmentAlias::from(fragment));
                }
            }
            Owner::Anchor(key) => {
                let (_, aliases) = output
                    .get_mut(&key)
                    .expect("native target candidate has an allocated identity");
                // `.Tg` is an authored fragment identity even when its exact
                // spelling happens to equal the normalized internal NodeId.
                // Keep both namespaces explicit so later allocator changes do
                // not silently remove the source-authored deep link.
                aliases.push(FragmentAlias::from(fragment));
            }
        }
    }
    output
}

struct InlineProjector<'a> {
    report: &'a NativeExecutionReport,
    node_wrappers: Vec<Option<usize>>,
    subtree_ends: Vec<usize>,
    word_nodes: Vec<(u32, usize)>,
    references: &'a [NativeReference],
    reference_ranges: Vec<Range<u32>>,
    reference_children: HashMap<Option<u32>, Vec<usize>>,
    visible_atoms: Vec<bool>,
    visible_implicit_spaces: Vec<bool>,
    final_cells: Vec<Option<(u32, u32, i64, i64)>>,
    /// Native leading cells before the first visible atom of one logical
    /// field.  Unlike a block offset, these cells are part of the field's
    /// executed content relationship (for example the retained separator
    /// after `.mc` flushes an occupied but glyph-free field).
    field_leading_bu: Vec<Option<i64>>,
    /// Origin atom of an invisible native field whose settlement ended a
    /// device line immediately before this visible atom.  Keeping the origin
    /// lets each structural owner decide whether the line event belongs
    /// inside it or between two owners.
    settled_break_origin: Vec<Option<u32>>,
    word_end_break_before: Vec<bool>,
    /// Ordered native `.fi`/`.nf` completion points.  Fill mode belongs to
    /// the formatter execution stream, not to one IR owner: structured man
    /// descriptions and generic prose must split at the same transition.
    fill_transitions: Vec<u64>,
    cell_bu: i64,
    suppressed_reference_atoms: Vec<bool>,
    bsd_replacements: BTreeMap<u32, BsdReplacement>,
    replaced_bsd_atoms: Vec<bool>,
    compatibility_glyphs: BTreeMap<u32, String>,
    anchors: HashMap<u32, &'a NativeAnchor>,
    anchor_ids: &'a HashMap<u32, (NodeId, Vec<FragmentAlias>)>,
    authored_sections: &'a HashMap<String, Option<NodeId>>,
}

#[derive(Clone, Debug)]
struct BsdReplacement {
    range: Range<u32>,
    font: ExecutionFont,
    value: &'static str,
}

/// Resolve the terminal device stream into its final visible cells.
///
/// Fixed CVS emits overstrikes as ordinary glyphs interleaved with backspace
/// device events (`term.c::term_field()` and `encode1()`).  Fragment geometry
/// records the native logical `viscol`, which can deliberately remain ahead
/// of the physical device after a zero-width marker separates a glyph from
/// its backspace.  Replaying the emitted device events is therefore the only
/// authoritative way to decide which later content glyph occupies a cell;
/// re-decoding source operands cannot recover this cross-word relationship.
fn native_final_cells(
    report: &NativeExecutionReport,
) -> (Vec<bool>, Vec<Option<(u32, u32, i64, i64)>>) {
    let default_cell_bu = report
        .fragments()
        .iter()
        .filter_map(|fragment| {
            (fragment.end_bu > fragment.start_bu).then_some(fragment.end_bu - fragment.start_bu)
        })
        .min()
        .unwrap_or(1);
    let mut final_cells = BTreeMap::<(u32, i64), (u32, i64)>::new();
    let mut line = None;
    let mut correction_bu = 0i64;
    let mut emitted_widths = Vec::<i64>::new();

    for fragment in report.fragments() {
        let atom = &report.atoms()[fragment.atom.0 as usize];
        if line != Some(fragment.device_line) {
            line = Some(fragment.device_line);
            correction_bu = 0;
            emitted_widths.clear();
        }
        let actual_start = fragment.start_bu.saturating_add(correction_bu).max(0);
        if atom.kind == AtomKind::Backspace {
            let reported_width = fragment.start_bu.saturating_sub(fragment.end_bu);
            let width = (reported_width > 0)
                .then_some(reported_width)
                .or_else(|| emitted_widths.pop())
                .unwrap_or(default_cell_bu);
            let actual_end = actual_start.saturating_sub(width);
            correction_bu = actual_end.saturating_sub(fragment.end_bu);
            continue;
        }

        let reported_width = fragment.end_bu.saturating_sub(fragment.start_bu);
        let width = reported_width.max(atom.width_bu).max(0);
        let actual_end = actual_start.saturating_add(width);
        correction_bu = actual_end.saturating_sub(fragment.end_bu);
        if width > 0 {
            emitted_widths.push(width);
        }
        if fragment.role == libmandoc_rs::FragmentRole::Content {
            final_cells.insert(
                (fragment.device_line, actual_start),
                (fragment.atom.0, actual_end),
            );
        }
    }

    let mut visible_atoms = vec![false; report.atoms().len()];
    let mut cells = vec![None; report.atoms().len()];
    for ((line, start), (atom, end)) in final_cells {
        visible_atoms[atom as usize] = true;
        cells[atom as usize] = Some((atom, line, start, end));
    }
    (visible_atoms, cells)
}

/// Pair ManT's deliberately narrow character-table extensions with the
/// unknown special-character atoms executed by fixed CVS.
///
/// `term.c::term_word()` asks `mchars_spec2cp()` for a named character and
/// writes `ASCII_NBRZW` when the pinned table does not know it.  Source text
/// alone is not permission to replace native output: a formatter word may
/// also contain real zero-width controls.  Promote an extension only when the
/// exact word supplies a one-to-one correspondence between compatibility
/// glyph events and retained native zero-width atoms; otherwise native output
/// wins unchanged.
fn native_compatibility_glyphs(report: &NativeExecutionReport) -> BTreeMap<u32, String> {
    let mut output = BTreeMap::new();
    for word in report.words() {
        let Some(operand) = report.pool_bytes(word.operand) else {
            continue;
        };
        let glyphs =
            super::super::roff_escape::compatibility_glyphs(&String::from_utf8_lossy(operand));
        if glyphs.is_empty() {
            continue;
        }
        let atoms = word
            .atoms
            .clone()
            .filter(|cursor| {
                let atom = &report.atoms()[*cursor as usize];
                atom.kind == AtomKind::ZeroWidth
                    && atom.role == word.role
                    && matches!(
                        atom.disposition,
                        AtomDisposition::Emitted
                            | AtomDisposition::Buffered
                            | AtomDisposition::Consumed
                    )
            })
            .collect::<Vec<_>>();
        if atoms.len() != glyphs.len() {
            continue;
        }
        output.extend(atoms.into_iter().zip(glyphs));
    }
    output
}

impl<'a> InlineProjector<'a> {
    fn new(
        report: &'a NativeExecutionReport,
        references: &'a [NativeReference],
        anchors: &'a [NativeAnchor],
        anchor_ids: &'a HashMap<u32, (NodeId, Vec<FragmentAlias>)>,
        authored_sections: &'a HashMap<String, Option<NodeId>>,
    ) -> Self {
        let node_wrappers = native_node_wrapper_index(report);
        let mut subtree_ends = vec![report.nodes().len(); report.nodes().len()];
        let mut open = Vec::<usize>::new();
        for (index, node) in report.nodes().iter().enumerate() {
            while open
                .last()
                .is_some_and(|parent| Some(report.nodes()[*parent].key) != node.parent)
            {
                subtree_ends[open.pop().unwrap()] = index;
            }
            open.push(index);
        }
        let (visible_atoms, cells) = native_final_cells(report);
        let compatibility_glyphs = native_compatibility_glyphs(report);
        let mut previous = vec![None; report.atoms().len()];
        let mut current = None;
        for (index, cell) in cells.iter().enumerate() {
            previous[index] = current;
            if cell.is_some() {
                current = *cell;
            }
        }
        let mut next = vec![None; report.atoms().len()];
        current = None;
        for (index, cell) in cells.iter().enumerate().rev() {
            next[index] = current;
            if cell.is_some() {
                current = *cell;
            }
        }
        let mut visible_implicit_spaces = report
            .atoms()
            .iter()
            .enumerate()
            .map(|(index, atom)| {
                atom.kind == AtomKind::BreakableSpace
                    && atom.disposition == AtomDisposition::Emitted
                    && (atom.role != AtomRole::ImplicitSpace
                        || matches!(
                            (previous[index], next[index]),
                            (Some((_, before_line, _, before_end)), Some((_, after_line, after_start, _)))
                                if before_line == after_line && before_end < after_start
                        ))
            })
            .collect::<Vec<_>>();
        // `term_fill_mode()` consumes the logical separator at an automatic
        // device wrap before the next field is emitted.  That cell is absent
        // from device fragments, but compact semantic text must retain the
        // word relationship instead of concatenating the two source words.
        // Restrict restoration to the exact tail range of a native Wrapped
        // flush; other Consumed spaces are layout state, not prose.
        let mut wrapped_tails = HashMap::<u32, Vec<(Range<u32>, u64)>>::new();
        for flush in report
            .flushes()
            .iter()
            .filter(|flush| flush.outcome == libmandoc_rs::FlushOutcome::Wrapped)
        {
            wrapped_tails
                .entry(flush.buffer_generation)
                .or_default()
                .push((flush.tail_discarded.clone(), flush.outcome_sequence));
        }
        for (index, atom) in report.atoms().iter().enumerate() {
            if atom.kind != AtomKind::BreakableSpace
                || atom.disposition != AtomDisposition::Consumed
            {
                continue;
            }
            let Some((generation, slot)) = atom.buffer_generation.zip(atom.slot) else {
                continue;
            };
            if wrapped_tails.get(&generation).is_some_and(|ranges| {
                ranges
                    .iter()
                    .any(|(range, outcome)| range.contains(&slot) && atom.sequence < *outcome)
            }) {
                visible_implicit_spaces[index] = true;
            }
        }
        let cell_bu = cells
            .iter()
            .flatten()
            .filter_map(|(_, _, start, end)| (*end > *start).then_some(*end - *start))
            .min()
            .unwrap_or(1);
        let mut field_leading_bu = vec![None; report.atoms().len()];
        for flush in report.flushes() {
            let first = report.fragments()
                [flush.fragments.start as usize..flush.fragments.end as usize]
                .iter()
                .filter(|fragment| fragment.role == libmandoc_rs::FragmentRole::Content)
                .filter_map(|fragment| Some((fragment.atom, fragment.start_bu)))
                .min_by_key(|(atom, _)| atom.0);
            let Some((atom, start_bu)) = first else {
                continue;
            };
            // `term_field()` folds buffered leading blanks into `vbl` before
            // emitting the first glyph.  The flush's logical origin excludes
            // those cells, so their difference is the exact line-local
            // prefix independently of whether the blanks were authored,
            // implicit, or introduced by native alignment.
            field_leading_bu[atom.0 as usize] =
                Some(start_bu.saturating_sub(flush.logical_origin_bu));
        }
        let mut generation_execution_origin = HashMap::<u32, u32>::new();
        for atom in report.atoms().iter().filter(|atom| {
            atom.node.is_some() && matches!(atom.kind, AtomKind::WordEndBreak | AtomKind::ZeroWidth)
        }) {
            if let Some(generation) = atom.buffer_generation {
                generation_execution_origin
                    .entry(generation)
                    .and_modify(|current| *current = (*current).min(atom.key.0))
                    .or_insert(atom.key.0);
            }
        }
        let boundaries = report
            .boundaries()
            .iter()
            .map(|boundary| (boundary.key, boundary))
            .collect::<HashMap<_, _>>();
        let mut settled_break_origin = vec![None; report.atoms().len()];
        for flush in report.flushes().iter().filter(|flush| {
            flush.fragments.is_empty()
                && (flush.logical_forced_break
                    || flush.boundary.is_some_and(|key| {
                        boundaries.get(&key).is_some_and(|boundary| {
                            boundary.effect == libmandoc_rs::BoundaryEffect::EndedLine
                        })
                    }))
        }) {
            let Some(&origin) = generation_execution_origin.get(&flush.buffer_generation) else {
                continue;
            };
            if let Some((next, _)) = cells.iter().enumerate().find(|(index, cell)| {
                cell.is_some() && report.atoms()[*index].sequence > flush.outcome_sequence
            }) {
                settled_break_origin[next] = Some(origin);
            }
        }
        let mut word_end_break_before = vec![false; report.atoms().len()];
        let mut pending_word_end_break = None::<(Option<u32>, bool, Option<u32>)>;
        let mut previous_content = None::<(Option<u32>, u32)>;
        for (index, atom) in report.atoms().iter().enumerate() {
            if atom.kind == AtomKind::WordEndBreak {
                let seen_content = previous_content
                    .is_some_and(|(generation, _)| generation == atom.buffer_generation);
                pending_word_end_break = Some((
                    atom.buffer_generation,
                    seen_content,
                    seen_content.then(|| previous_content.unwrap().1),
                ));
            }
            let Some((_, device_line, _, _)) = cells[index] else {
                continue;
            };
            if let Some((generation, seen_content, pending_line)) = &mut pending_word_end_break {
                if !*seen_content && atom.buffer_generation == *generation {
                    *seen_content = true;
                    *pending_line = Some(device_line);
                } else if *seen_content && pending_line.is_some_and(|line| line != device_line) {
                    word_end_break_before[index] = true;
                    pending_word_end_break = None;
                }
            }
            previous_content = Some((atom.buffer_generation, device_line));
        }
        let mut suppressed_reference_atoms = vec![false; report.atoms().len()];
        let mut reference_ranges = Vec::with_capacity(references.len());
        for reference in references {
            let label_is_visible = reference.atoms.clone().any(|cursor| {
                (visible_atoms[cursor as usize] || compatibility_glyphs.contains_key(&cursor))
                    && char::from_u32(report.atoms()[cursor as usize].display_scalar).map_or_else(
                        || {
                            compatibility_glyphs
                                .get(&cursor)
                                .is_some_and(|value| !value.chars().all(char::is_whitespace))
                        },
                        |value| !value.is_whitespace(),
                    )
            });
            match reference.presentation {
                ExecutionReferencePresentation::Direct => {
                    reference_ranges.push(reference.atoms.clone());
                }
                ExecutionReferencePresentation::LabelledSupplement if label_is_visible => {
                    reference_ranges.push(reference.atoms.clone());
                    if let Some(supplement) = &reference.supplement_atoms {
                        suppressed_reference_atoms
                            [supplement.start as usize..supplement.end as usize]
                            .fill(true);
                    }
                }
                ExecutionReferencePresentation::LabelledSupplement => {
                    reference_ranges.push(reference.target_atoms.clone());
                    suppressed_reference_atoms
                        [reference.atoms.start as usize..reference.atoms.end as usize]
                        .fill(true);
                    if let Some(supplement) = &reference.supplement_atoms {
                        suppressed_reference_atoms
                            [supplement.start as usize..reference.target_atoms.start as usize]
                            .fill(true);
                        suppressed_reference_atoms
                            [reference.target_atoms.end as usize..supplement.end as usize]
                            .fill(true);
                    }
                }
            }
        }
        let mut reference_children = HashMap::<Option<u32>, Vec<usize>>::new();
        for (index, reference) in references.iter().enumerate() {
            reference_children
                .entry(reference.parent)
                .or_default()
                .push(index);
        }
        for children in reference_children.values_mut() {
            children.sort_unstable_by_key(|index| {
                let range = &reference_ranges[*index];
                (range.start, range.end)
            });
        }
        let (bsd_replacements, replaced_bsd_atoms) =
            bsd_replacements(report, &node_wrappers, &cells);
        let mut fill_transitions = report
            .controls()
            .iter()
            .filter(|control| {
                matches!(
                    control.request,
                    libmandoc_rs::ExecutionControlRequest::Fill
                        | libmandoc_rs::ExecutionControlRequest::NoFill
                )
            })
            .map(|control| control.leave_sequence)
            .collect::<Vec<_>>();
        fill_transitions.sort_unstable();
        fill_transitions.dedup();
        let mut word_nodes = report
            .words()
            .iter()
            .enumerate()
            .filter_map(|(index, word)| word.node.map(|node| (node.0, index)))
            .collect::<Vec<_>>();
        word_nodes.sort_unstable();
        Self {
            report,
            node_wrappers,
            subtree_ends,
            word_nodes,
            references,
            reference_ranges,
            reference_children,
            visible_atoms,
            visible_implicit_spaces,
            final_cells: cells,
            field_leading_bu,
            settled_break_origin,
            word_end_break_before,
            fill_transitions,
            cell_bu,
            suppressed_reference_atoms,
            bsd_replacements,
            replaced_bsd_atoms,
            compatibility_glyphs,
            anchors: anchors.iter().map(|value| (value.key, value)).collect(),
            anchor_ids,
            authored_sections,
        }
    }

    fn atom_visible(&self, cursor: u32) -> bool {
        let atom = &self.report.atoms()[cursor as usize];
        if atom.kind == AtomKind::Tab {
            // A tab is an execution/layout event rather than a compactable
            // label glyph.  Word-end breaks are deliberately not projected
            // here: fixed CVS records `\p` when encountered but only performs
            // it at the later formatter-word boundary, which is represented
            // by the native boundary stream.
            return atom.disposition == AtomDisposition::Emitted;
        }
        if self.suppressed_reference_atoms[cursor as usize] {
            return false;
        }
        self.compatibility_glyphs.contains_key(&cursor)
            || self.visible_atoms[cursor as usize]
            || self.visible_implicit_spaces[cursor as usize]
    }

    fn has_fill_transition_between(&self, before: u64, after: u64) -> bool {
        let transition = self
            .fill_transitions
            .partition_point(|sequence| *sequence <= before);
        self.fill_transitions
            .get(transition)
            .is_some_and(|sequence| *sequence < after)
    }

    fn descendants(&self, root: ExecutionNodeKey) -> &[libmandoc_rs::ExecutionNode] {
        &self.report.nodes()[root.0 as usize + 1..self.subtree_ends[root.0 as usize]]
    }

    fn descendant_words(
        &self,
        root: ExecutionNodeKey,
    ) -> impl Iterator<Item = &libmandoc_rs::ExecutionWord> {
        let subtree_end = u32::try_from(self.subtree_ends[root.0 as usize]).unwrap();
        let start = self.word_nodes.partition_point(|(node, _)| *node < root.0);
        let end = self
            .word_nodes
            .partition_point(|(node, _)| *node < subtree_end);
        self.word_nodes[start..end]
            .iter()
            .map(|(_, index)| &self.report.words()[*index])
    }

    fn segment(
        &self,
        range: Range<u32>,
        anchor_keys: &[u32],
        boundaries: &[ExecutionBoundary],
        source: Option<SourceSpan>,
        policy: FlowProjectionPolicy,
    ) -> Vec<Inline> {
        let mut anchor_cursors = BTreeMap::<u32, Vec<u32>>::new();
        for key in anchor_keys {
            let Some(anchor) = self.anchors.get(key) else {
                continue;
            };
            if (range.start..=range.end).contains(&anchor.atom_cursor) {
                anchor_cursors
                    .entry(anchor.atom_cursor)
                    .or_default()
                    .push(*key);
            }
        }
        let mut break_cursors = physical_break_cursors(self.report, boundaries, &range);
        break_cursors.retain(|cursor, _| {
            // A boundary at either edge belongs between primary content
            // owners.  Fixed CVS `term_newln()` flushes the preceding field
            // before the following word is executed; it does not make the
            // following source-neutral block begin with a LineBreak.
            (range.start < *cursor || policy.include_edge_breaks && range.start == *cursor)
                && *cursor < range.end
        });
        if policy.alignment_owner_start == Some(range.start)
            && let Some(count) = break_cursors.get_mut(&range.start)
            && (!policy.retain_owner_start_break || *count > 1)
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                break_cursors.remove(&range.start);
            }
        }
        if let Some(NativeBodySettlement::Boundary(settlement)) = policy.body_settlement {
            // Consume only the descendant device newline of the exact native
            // event that closed the pending label row.  Later BODY-local
            // newlines remain content even when their geometry is identical.
            if let Some(boundary) = boundaries.iter().find(|boundary| {
                boundary.request == libmandoc_rs::BoundaryRequest::DeviceEndline
                    && boundary_has_ancestor_key(self.report, boundary, settlement)
            }) {
                let cursor = boundary_atom_cursor(self.report, &range, boundary.leave_sequence);
                if let Some(count) = break_cursors.get_mut(&cursor) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        break_cursors.remove(&cursor);
                    }
                }
            }
        }
        self.range(
            range,
            None,
            &anchor_cursors,
            &break_cursors,
            source,
            policy.allow_field_leading,
            policy.body_settlement,
        )
    }

    fn standalone_anchors(&self, keys: &[u32], source: Option<SourceSpan>) -> Vec<Inline> {
        keys.iter()
            .filter_map(|key| self.anchor_ids.get(key))
            .map(|(id, aliases)| Inline::Anchor {
                id: id.clone(),
                fragment_aliases: aliases.clone(),
                owner_source: source,
            })
            .collect()
    }

    fn range(
        &self,
        range: Range<u32>,
        parent_reference: Option<u32>,
        anchors: &BTreeMap<u32, Vec<u32>>,
        breaks: &BTreeMap<u32, usize>,
        source: Option<SourceSpan>,
        allow_field_leading: bool,
        body_settlement: Option<NativeBodySettlement>,
    ) -> Vec<Inline> {
        let mut output = Vec::new();
        let first_content = range
            .clone()
            .find(|cursor| self.final_cells[*cursor as usize].is_some());
        let settlement_cursor = match body_settlement {
            Some(NativeBodySettlement::Boundary(key)) => {
                let sequence = self.report.boundaries()[key as usize].leave_sequence;
                range.clone().find(|cursor| {
                    self.report.atoms()[*cursor as usize].sequence > sequence
                        && self.final_cells[*cursor as usize].is_some()
                })
            }
            Some(NativeBodySettlement::WordEndBreak) | None => None,
        };
        let children = self
            .reference_children
            .get(&parent_reference)
            .map_or(&[][..], Vec::as_slice);
        let child_start =
            children.partition_point(|index| self.reference_ranges[*index].start < range.start);
        let child_end =
            children.partition_point(|index| self.reference_ranges[*index].start < range.end);
        let children = &children[child_start..child_end];
        let mut child = 0;
        let mut cursor = range.start;
        let mut previous_cell = None;
        while cursor <= range.end {
            if let Some(keys) = anchors.get(&cursor) {
                for key in keys {
                    if let Some((id, aliases)) = self.anchor_ids.get(key) {
                        output.push(Inline::Anchor {
                            id: id.clone(),
                            fragment_aliases: aliases.clone(),
                            owner_source: source,
                        });
                    }
                }
            }
            if cursor == range.end {
                break;
            }
            if let Some(count) = breaks.get(&cursor) {
                trim_breakable_space(&mut output);
                output.extend(std::iter::repeat_n(Inline::LineBreak, *count));
                // `term_newln()` flushes the previous field and resets the
                // device-line relationship before the next formatter word.
                // The next atom must therefore project its own field-leading
                // cells rather than measure from the preceding device row.
                previous_cell = None;
            }
            if let Some(replacement) = self.bsd_replacements.get(&cursor)
                && replacement.range.end <= range.end
            {
                let first_cell = replacement
                    .range
                    .clone()
                    .filter_map(|atom| self.final_cells[atom as usize])
                    .next();
                self.push_cell_boundary(
                    &mut output,
                    previous_cell,
                    first_cell,
                    range.start,
                    cursor,
                    allow_field_leading,
                    body_settlement,
                    first_content,
                    settlement_cursor,
                );
                output.push(styled_text(replacement.font, replacement.value.to_owned()));
            }
            if let Some(reference_index) = children.get(child).copied()
                && self.reference_ranges[reference_index].start == cursor
                && self.reference_ranges[reference_index].end <= range.end
            {
                let reference = &self.references[reference_index];
                let reference_range = self.reference_ranges[reference_index].clone();
                let first_cell = reference_range
                    .clone()
                    .filter_map(|atom| {
                        (!self.suppressed_reference_atoms[atom as usize])
                            .then_some(self.final_cells[atom as usize])
                            .flatten()
                    })
                    .next();
                self.push_cell_boundary(
                    &mut output,
                    previous_cell,
                    first_cell,
                    range.start,
                    cursor,
                    allow_field_leading,
                    body_settlement,
                    first_content,
                    settlement_cursor,
                );
                let nested = self.range(
                    reference_range.clone(),
                    Some(reference.key),
                    anchors,
                    breaks,
                    source,
                    false,
                    body_settlement,
                );
                if let Some(target) = reference_target(reference, self.authored_sections) {
                    output.push(Inline::Link {
                        target,
                        title: None,
                        children: nested,
                    });
                } else if matches!(
                    reference.kind,
                    ExecutionReferenceKind::ExternalUri | ExecutionReferenceKind::Email
                ) && super::super::inline::link_identity_text(&String::from_utf8_lossy(
                    &reference.primary,
                ))
                .is_empty()
                {
                    // A control-only URI/mail operand executes natively but
                    // has no semantic destination or visible identity.  Its
                    // terminal state is already reflected in later atoms;
                    // do not manufacture an empty target or leak the hidden
                    // formatter operand into content.  A labelled `.Lk`
                    // still degrades to its executed visible label.
                    if reference.presentation == ExecutionReferencePresentation::LabelledSupplement
                    {
                        output.extend(nested);
                    }
                } else {
                    output.extend(nested);
                }
                previous_cell = reference_range
                    .clone()
                    .filter_map(|atom| {
                        (!self.suppressed_reference_atoms[atom as usize])
                            .then_some(self.final_cells[atom as usize])
                            .flatten()
                    })
                    .next_back()
                    .or(previous_cell);
                cursor = reference_range.end;
                child += 1;
                continue;
            }
            let atom = &self.report.atoms()[cursor as usize];
            if self.replaced_bsd_atoms[cursor as usize]
                && !matches!(atom.kind, AtomKind::WordEndBreak | AtomKind::Tab)
            {
                previous_cell = self.final_cells[cursor as usize].or(previous_cell);
                cursor += 1;
                continue;
            }
            if self.atom_visible(cursor)
                && let Some((font, value)) = self
                    .compatibility_glyphs
                    .get(&cursor)
                    .map(|value| (atom.font, value.clone()))
                    .or_else(|| fragment_atom(atom))
            {
                let cell = self.final_cells[cursor as usize];
                self.push_cell_boundary(
                    &mut output,
                    previous_cell,
                    cell,
                    range.start,
                    cursor,
                    allow_field_leading,
                    body_settlement,
                    first_content,
                    settlement_cursor,
                );
                push_styled_text(&mut output, font, value);
                previous_cell = cell.or(previous_cell);
            }
            cursor += 1;
        }
        trim_breakable_space(&mut output);
        output
    }

    fn push_cell_boundary(
        &self,
        output: &mut Vec<Inline>,
        previous: Option<(u32, u32, i64, i64)>,
        current: Option<(u32, u32, i64, i64)>,
        range_start: u32,
        cursor: u32,
        allow_field_leading: bool,
        body_settlement: Option<NativeBodySettlement>,
        first_content: Option<u32>,
        settlement_cursor: Option<u32>,
    ) {
        let Some((after_atom, after_line, after_start, _)) = current else {
            return;
        };
        if self.settled_break_origin[after_atom as usize]
            .is_some_and(|origin| range_start <= origin)
        {
            let consume = match body_settlement {
                Some(NativeBodySettlement::Boundary(_)) => Some(cursor) == settlement_cursor,
                Some(NativeBodySettlement::WordEndBreak) => Some(cursor) == first_content,
                None => false,
            };
            if consume {
                if allow_field_leading {
                    self.push_field_leading(output, after_atom);
                }
                return;
            }
            trim_breakable_space(output);
            if !matches!(output.last(), Some(Inline::LineBreak)) {
                output.push(Inline::LineBreak);
            }
            if allow_field_leading {
                self.push_field_leading(output, after_atom);
            }
            return;
        }
        let Some((before_atom, before_line, _, before_end)) = previous else {
            // Fixed CVS `term_flushln()` computes `vbl` from the field's
            // logical origin before calling `term_field()`.  Preserve only
            // the extra cells between that origin and the first emitted
            // content atom; the origin itself remains block layout.
            if allow_field_leading {
                self.push_field_leading(output, after_atom);
            }
            return;
        };
        if self.word_end_break_before[after_atom as usize] {
            if body_settlement == Some(NativeBodySettlement::WordEndBreak)
                && Some(cursor) == first_content
            {
                if allow_field_leading {
                    self.push_field_leading(output, after_atom);
                }
                return;
            }
            trim_breakable_space(output);
            if !matches!(output.last(), Some(Inline::LineBreak)) {
                output.push(Inline::LineBreak);
            }
            if allow_field_leading {
                self.push_field_leading(output, after_atom);
            }
            return;
        }
        if before_line == after_line {
            let cells = after_start.saturating_sub(before_end) / self.cell_bu.max(1);
            // Only native geometry between two content cells is projected.
            // Leading offsets remain block layout, and an explicit formatter
            // blank atom already owns its own cell.
            // A gap spanning suppressed atoms belongs to a compacted native
            // presentation envelope (for example `.Lk`'s `: URI`), not to
            // surrounding authored whitespace.  Other gaps are objective
            // formatter cells and retain their exact width.
            let compacted = (before_atom.saturating_add(1)..after_atom).any(|atom| {
                self.suppressed_reference_atoms
                    .get(atom as usize)
                    .copied()
                    .unwrap_or(false)
            });
            let positioned_by_tab = (before_atom.saturating_add(1)..after_atom).any(|atom| {
                self.report.atoms()[atom as usize].kind == AtomKind::Tab
                    && self.report.atoms()[atom as usize].disposition == AtomDisposition::Emitted
            });
            if !compacted && !positioned_by_tab {
                let already_projected =
                    output.last().is_some_and(inline_ends_in_breakable_space) as i64;
                for _ in already_projected..cells {
                    match output.last_mut() {
                        Some(Inline::Text { value }) => value.push(' '),
                        _ => output.push(Inline::Text {
                            value: " ".to_owned(),
                        }),
                    }
                }
            }
        } else if cursor > range_start
            && self.report.atoms()[cursor as usize]
                .node
                .is_some_and(|node| {
                    self.report.nodes()[node.0 as usize]
                        .flags
                        .contains(libmandoc_rs::ExecutionNodeFlags::NO_FILL)
                })
        {
            trim_breakable_space(output);
            for _ in before_line..after_line {
                if !matches!(output.last(), Some(Inline::LineBreak)) {
                    output.push(Inline::LineBreak);
                }
            }
        }
    }

    fn push_field_leading(&self, output: &mut Vec<Inline>, atom: u32) {
        let leading =
            self.field_leading_bu[atom as usize].unwrap_or_default() / self.cell_bu.max(1);
        for _ in 0..leading {
            output.push(Inline::Text {
                value: " ".to_owned(),
            });
        }
    }
}

fn visible_atom(atom: &libmandoc_rs::ExecutionAtom) -> Option<(ExecutionFont, String)> {
    if !matches!(
        atom.disposition,
        AtomDisposition::Emitted | AtomDisposition::Buffered
    ) || matches!(
        atom.role,
        AtomRole::FontDecoration | AtomRole::DeviceGenerated
    ) {
        return None;
    }
    fragment_atom(atom)
}

fn fragment_atom(atom: &libmandoc_rs::ExecutionAtom) -> Option<(ExecutionFont, String)> {
    // Membership in a native `Content` fragment, rather than the atom's
    // origin role, is the visibility authority.  Fixed CVS emits generated
    // punctuation and replacement words as `MacroGenerated` or
    // `DeviceGenerated` atoms, while font overstrike decorations are assigned
    // to non-content fragments.  The report validator also guarantees that a
    // fragment atom has the final `Emitted` disposition.
    let value = match atom.kind {
        AtomKind::Glyph | AtomKind::BreakableHyphen => {
            char::from_u32(atom.display_scalar)?.to_string()
        }
        AtomKind::BreakableSpace => " ".to_owned(),
        AtomKind::NonBreakingSpace => "\u{a0}".to_owned(),
        AtomKind::Tab => "\t".to_owned(),
        AtomKind::WordEndBreak => "\n".to_owned(),
        AtomKind::ZeroWidth
        | AtomKind::TabReference
        | AtomKind::Backspace
        | AtomKind::BreakPoint => return None,
    };
    Some((atom.font, value))
}

fn push_styled_text(output: &mut Vec<Inline>, font: ExecutionFont, value: String) {
    if value == "\n" {
        trim_breakable_space(output);
        if !matches!(output.last(), Some(Inline::LineBreak)) {
            output.push(Inline::LineBreak);
        }
        return;
    }
    if value == " " {
        if output.is_empty() || inline_ends_in_breakable_space(output.last().unwrap()) {
            return;
        }
        // `term_field()` emits a retained buffered space under the same
        // effective font as its neighbouring glyphs.  Preserve that native
        // span instead of splitting styled phrases at every word boundary.
    }
    match (font, output.last_mut()) {
        (ExecutionFont::Roman, Some(Inline::Text { value: current }))
            if !(current.chars().all(char::is_whitespace)
                && !value.chars().all(char::is_whitespace)) =>
        {
            current.push_str(&value);
        }
        (ExecutionFont::Bold, Some(Inline::Strong { children }))
        | (ExecutionFont::Underline, Some(Inline::Emphasis { children }))
            if matches!(children.as_slice(), [Inline::Text { .. }]) =>
        {
            let Inline::Text { value: current } = &mut children[0] else {
                unreachable!()
            };
            current.push_str(&value);
        }
        _ => output.push(styled_text(font, value)),
    }
}

fn styled_text(font: ExecutionFont, value: String) -> Inline {
    match font {
        ExecutionFont::Roman => Inline::Text { value },
        ExecutionFont::Bold => Inline::Strong {
            children: vec![Inline::Text { value }],
        },
        ExecutionFont::Underline => Inline::Emphasis {
            children: vec![Inline::Text { value }],
        },
        ExecutionFont::BoldUnderline => Inline::Strong {
            children: vec![Inline::Emphasis {
                children: vec![Inline::Text { value }],
            }],
        },
    }
}

fn inline_ends_in_breakable_space(inline: &Inline) -> bool {
    match inline {
        Inline::Text { value } => value.ends_with(' '),
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => {
            children.last().is_some_and(inline_ends_in_breakable_space)
        }
        Inline::LineBreak => true,
        _ => false,
    }
}

fn trim_breakable_space(output: &mut Vec<Inline>) {
    loop {
        let remove = match output.last_mut() {
            Some(Inline::Text { value }) if value.ends_with(' ') => {
                value.pop();
                value.is_empty()
            }
            Some(
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. },
            ) if children.last().is_some_and(inline_ends_in_breakable_space) => {
                trim_breakable_space(children);
                children.is_empty()
            }
            _ => break,
        };
        if remove {
            output.pop();
        }
    }
}

fn paragraph(inlines: Vec<Inline>, source: Option<SourceSpan>) -> Vec<Block> {
    (!inlines.is_empty())
        .then_some(Block::Paragraph {
            children: inlines,
            layout: LayoutHint::default(),
            source,
        })
        .into_iter()
        .collect()
}

fn native_content_block(
    children: Vec<Inline>,
    preformatted: bool,
    source: Option<SourceSpan>,
) -> Option<Block> {
    if children.is_empty() {
        return None;
    }
    let layout = LayoutHint::default();
    Some(if preformatted {
        Block::Preformatted {
            children,
            language: None,
            layout,
            source,
        }
    } else {
        Block::Paragraph {
            children,
            layout,
            source,
        }
    })
}

fn collapse_inline_flow(
    blocks: Vec<Block>,
    preformatted: bool,
    source: Option<SourceSpan>,
) -> Vec<Block> {
    let mut children = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph {
                children: mut next, ..
            }
            | Block::Preformatted {
                children: mut next, ..
            } => {
                if !children.is_empty() && !matches!(children.last(), Some(Inline::LineBreak)) {
                    children.push(Inline::LineBreak);
                }
                children.append(&mut next);
            }
            Block::VerticalSpace { lines, .. } => {
                if !children.is_empty() && !matches!(children.last(), Some(Inline::LineBreak)) {
                    children.push(Inline::LineBreak);
                }
                children.extend((0..lines).map(|_| Inline::LineBreak));
            }
            _ => unreachable!("native inline flow contains only text and vertical-space blocks"),
        }
    }
    native_content_block(children, preformatted, source)
        .into_iter()
        .collect()
}

fn boundary_atom_cursor(report: &NativeExecutionReport, range: &Range<u32>, sequence: u64) -> u32 {
    let atoms = report
        .atoms()
        .get(range.start as usize..range.end as usize)
        .unwrap_or_default();
    let index = atoms.partition_point(|atom| atom.sequence <= sequence);
    atoms.get(index).map_or(range.end, |atom| atom.key.0)
}

fn boundary_has_ancestor_request(
    report: &NativeExecutionReport,
    boundary: &ExecutionBoundary,
    request: libmandoc_rs::BoundaryRequest,
) -> bool {
    let mut current = Some(boundary.key);
    while let Some(key) = current {
        let value = &report.boundaries()[key as usize];
        if value.request == request {
            return true;
        }
        current = value.parent;
    }
    false
}

fn boundary_has_ancestor_key(
    report: &NativeExecutionReport,
    boundary: &ExecutionBoundary,
    ancestor: u32,
) -> bool {
    let mut current = Some(boundary.key);
    while let Some(key) = current {
        if key == ancestor {
            return true;
        }
        current = report.boundaries()[key as usize].parent;
    }
    false
}

fn physical_break_cursors(
    report: &NativeExecutionReport,
    boundaries: &[ExecutionBoundary],
    range: &Range<u32>,
) -> BTreeMap<u32, usize> {
    let atoms = report
        .atoms()
        .get(range.start as usize..range.end as usize)
        .unwrap_or_default();
    let mut output = BTreeMap::new();
    for boundary in boundaries.iter().filter(|boundary| {
        boundary.request == libmandoc_rs::BoundaryRequest::DeviceEndline
            && boundary.effect == libmandoc_rs::BoundaryEffect::EndedLine
            && !boundary_has_ancestor_request(
                report,
                boundary,
                libmandoc_rs::BoundaryRequest::VerticalSpace,
            )
    }) {
        let index = atoms.partition_point(|atom| atom.sequence <= boundary.leave_sequence);
        let cursor = atoms.get(index).map_or(range.end, |atom| atom.key.0);
        if (range.start..=range.end).contains(&cursor) {
            *output.entry(cursor).or_default() += 1;
        }
    }
    output
}

#[derive(Clone, Copy)]
struct FlowProjectionPolicy {
    include_edge_vertical: bool,
    include_edge_breaks: bool,
    body_settlement: Option<NativeBodySettlement>,
    allow_field_leading: bool,
    alignment_owner_start: Option<u32>,
    retain_owner_start_break: bool,
}

impl FlowProjectionPolicy {
    const fn content(include_edge_vertical: bool) -> Self {
        Self {
            include_edge_vertical,
            include_edge_breaks: false,
            body_settlement: None,
            allow_field_leading: true,
            alignment_owner_start: None,
            retain_owner_start_break: false,
        }
    }
}

/// Project one formatter-owned flow without confusing an IR block boundary
/// with a native line flush.
///
/// Fixed CVS `roff_term_pre_sp()` executes `term_vspace()` once per requested
/// row and only then executes the conditional break.  Consequently a vertical
/// request is a structural blank row, while an ordinary `term_newln()` inside
/// the same owner is an inline hard break.  Both are already present in the
/// owned execution report; this function merely partitions that ordered fact
/// stream and never replays a roff request.
fn flow_blocks(
    projector: &InlineProjector<'_>,
    range: Range<u32>,
    anchor_keys: &[u32],
    boundaries: &[ExecutionBoundary],
    preformatted: bool,
    source: Option<SourceSpan>,
    policy: FlowProjectionPolicy,
) -> Vec<Block> {
    let sequence_window = range
        .clone()
        .filter_map(|cursor| projector.report.atoms().get(cursor as usize))
        .map(|atom| atom.sequence)
        .fold(None::<(u64, u64)>, |window, sequence| {
            Some(match window {
                Some((first, last)) => (first.min(sequence), last.max(sequence)),
                None => (sequence, sequence),
            })
        });
    let mut vertical = boundaries
        .iter()
        .filter(|boundary| {
            boundary.request == libmandoc_rs::BoundaryRequest::VerticalSpace
                && boundary.effect == libmandoc_rs::BoundaryEffect::AddedVerticalSpace
                && (policy.include_edge_vertical
                    || sequence_window.is_some_and(|(first, last)| {
                        first <= boundary.enter_sequence && boundary.leave_sequence <= last
                    }))
        })
        .map(|boundary| {
            let cursor = boundary_atom_cursor(projector.report, &range, boundary.leave_sequence);
            // Fixed CVS `term_vspace()` first calls `term_newln()` and then
            // emits its requested empty device row.  Count the row committed
            // directly by the vertical request plus nested endlines that
            // committed a zero-visible field.  The enclosing line delta also
            // includes soft wraps of visible prose and is not blank-space
            // multiplicity.
            let nested_empty_rows = boundaries
                .iter()
                .filter(|candidate| {
                    candidate.key != boundary.key
                        && candidate.request == libmandoc_rs::BoundaryRequest::Endline
                        && candidate.visual_before == 0
                        && boundary_has_ancestor_key(projector.report, candidate, boundary.key)
                })
                .map(|candidate| candidate.direct_device_lines)
                .sum::<u32>();
            let lines = boundary
                .direct_device_lines
                .saturating_add(nested_empty_rows)
                .max(1);
            (cursor, u16::try_from(lines).unwrap_or(u16::MAX))
        })
        .filter(|(cursor, _)| (range.start..=range.end).contains(cursor))
        .collect::<Vec<_>>();
    vertical.sort_unstable_by_key(|(cursor, _)| *cursor);
    let physical_breaks = physical_break_cursors(projector.report, boundaries, &range);

    let mut blocks = Vec::new();
    let mut start = range.start;
    for (end, vertical_lines) in vertical.into_iter().chain(std::iter::once((range.end, 0))) {
        let mut segment_policy = policy;
        if start != range.start {
            segment_policy.include_edge_breaks = false;
        }
        let inlines =
            projector.segment(start..end, anchor_keys, boundaries, source, segment_policy);
        if !inlines.is_empty() {
            if inlines
                .iter()
                .all(|inline| matches!(inline, Inline::LineBreak))
            {
                for _ in 0..inlines.len() {
                    blocks.push(Block::VerticalSpace { lines: 1, source });
                }
            } else if let Some(block) = native_content_block(inlines, preformatted, source) {
                blocks.push(block);
            }
        }
        if vertical_lines != 0 {
            let lines = vertical_lines.saturating_add(
                u16::try_from(physical_breaks.get(&end).copied().unwrap_or_default())
                    .unwrap_or(u16::MAX),
            );
            blocks.push(Block::VerticalSpace { lines, source });
        }
        start = end;
    }
    blocks
}

fn flow_blocks_by_fill_with_default(
    projector: &InlineProjector<'_>,
    range: Range<u32>,
    anchor_keys: &[u32],
    boundaries: &[ExecutionBoundary],
    source: Option<SourceSpan>,
    policy: FlowProjectionPolicy,
    initial_preformatted: Option<bool>,
) -> Vec<Block> {
    let report = projector.report;
    let mut output = Vec::new();
    let range_sequence = range
        .start
        .checked_sub(1)
        .map_or(0, |cursor| report.atoms()[cursor as usize].sequence);
    let mut cursor = range.start;
    while cursor < range.end {
        let atom_preformatted = |cursor: u32| {
            let atom = &report.atoms()[cursor as usize];
            if !projector.has_fill_transition_between(range_sequence, atom.sequence)
                && let Some(initial) = initial_preformatted
            {
                initial
            } else {
                atom.flags
                    .contains(libmandoc_rs::ExecutionAtomFlags::NO_FILL)
            }
        };
        let preformatted = atom_preformatted(cursor);
        let mut end = cursor + 1;
        while end < range.end {
            let candidate = atom_preformatted(end);
            if candidate != preformatted {
                break;
            }
            if projector.has_fill_transition_between(
                report.atoms()[end as usize - 1].sequence,
                report.atoms()[end as usize].sequence,
            ) {
                break;
            }
            end += 1;
        }
        let mut blocks = flow_blocks(
            projector,
            cursor..end,
            anchor_keys,
            boundaries,
            preformatted,
            source,
            policy,
        );
        if preformatted {
            blocks = collapse_inline_flow(blocks, true, source);
        }
        output.extend(blocks);
        cursor = end;
    }
    output
}

enum NativeHeadRoleEvidence {
    None,
    Presentation,
    Semantic(crate::definitions::NativeHeadRole),
}

fn first_native_head_role(
    roots: &[ExecutionNodeKey],
    projector: &InlineProjector<'_>,
) -> NativeHeadRoleEvidence {
    for node in roots.iter().flat_map(|root| projector.descendants(*root)) {
        let role = match node.macro_name.as_deref() {
            Some("Fl") => Some(NativeHeadRoleEvidence::Semantic(
                crate::definitions::NativeHeadRole::Option,
            )),
            Some("Ev") => Some(NativeHeadRoleEvidence::Semantic(
                crate::definitions::NativeHeadRole::Environment,
            )),
            Some("Cm" | "Ic") => Some(NativeHeadRoleEvidence::Semantic(
                crate::definitions::NativeHeadRole::Literal,
            )),
            Some("Ar" | "Em" | "Sy") => Some(NativeHeadRoleEvidence::Presentation),
            Some(_) => None,
            None => projector.node_wrappers[node.key.0 as usize].and_then(|wrapper| {
                let wrapper = &projector.report.wrappers()[wrapper];
                projector.report.atoms()[wrapper.enter_atom as usize..wrapper.leave_atom as usize]
                    .iter()
                    .any(|atom| visible_atom(atom).is_some())
                    .then_some(NativeHeadRoleEvidence::Presentation)
            }),
        };
        if let Some(role) = role {
            return role;
        }
    }
    NativeHeadRoleEvidence::None
}

fn markup_evidence(
    roots: &[ExecutionNodeKey],
    report: &NativeExecutionReport,
    projector: &InlineProjector<'_>,
) -> crate::definitions::ExactNativeDefinitionEvidence {
    let mut evidence = crate::definitions::ExactNativeDefinitionEvidence::default();
    let root_start = roots
        .iter()
        .filter_map(|root| projector.node_wrappers[root.0 as usize])
        .map(|wrapper| report.wrappers()[wrapper].enter_atom)
        .min()
        .unwrap_or_default();
    let root_end = roots
        .iter()
        .filter_map(|root| projector.node_wrappers[root.0 as usize])
        .map(|wrapper| report.wrappers()[wrapper].leave_atom)
        .max()
        .unwrap_or(root_start);
    let mut plain_offsets = Vec::with_capacity((root_end - root_start) as usize + 1);
    let mut plain_offset = 0;
    plain_offsets.push(plain_offset);
    for atom in &report.atoms()[root_start as usize..root_end as usize] {
        if let Some((_, value)) = visible_atom(atom) {
            plain_offset += value.len();
        }
        plain_offsets.push(plain_offset);
    }
    evidence.role = match first_native_head_role(roots, projector) {
        NativeHeadRoleEvidence::None => return evidence,
        NativeHeadRoleEvidence::Presentation => None,
        NativeHeadRoleEvidence::Semantic(role) => Some(role),
    };
    let allowed = match evidence.role {
        Some(crate::definitions::NativeHeadRole::Option) => ["Fl"].as_slice(),
        Some(crate::definitions::NativeHeadRole::Environment) => ["Ev"].as_slice(),
        Some(crate::definitions::NativeHeadRole::Literal) => ["Cm", "Ic"].as_slice(),
        _ => return evidence,
    };
    for node in roots.iter().flat_map(|root| projector.descendants(*root)) {
        let Some(macro_name) = node
            .macro_name
            .as_deref()
            .filter(|name| allowed.contains(name))
        else {
            continue;
        };
        let Some(wrapper) = projector.node_wrappers[node.key.0 as usize] else {
            continue;
        };
        let wrapper = &report.wrappers()[wrapper];
        let start = usize::try_from(wrapper.enter_atom - root_start).unwrap();
        let end = usize::try_from(wrapper.leave_atom - root_start).unwrap();
        if plain_offsets[start] < plain_offsets[end] {
            evidence
                .markup
                .push(crate::definitions::ExactNativeNameEvidence {
                    term_index: 0,
                    parts: std::iter::once(plain_offsets[start]..plain_offsets[end]).collect(),
                });
        }
        if macro_name == "Ev" {
            for word in projector
                .descendant_words(node.key)
                .filter(|word| word.atoms.start >= root_start && word.atoms.end <= root_end)
            {
                let start = usize::try_from(word.atoms.start - root_start).unwrap();
                let end = usize::try_from(word.atoms.end - root_start).unwrap();
                if plain_offsets[start] < plain_offsets[end] {
                    evidence
                        .native_arguments
                        .push(crate::definitions::ExactNativeNameEvidence {
                            term_index: 0,
                            parts: std::iter::once(plain_offsets[start]..plain_offsets[end])
                                .collect(),
                        });
                }
            }
        }
    }
    evidence
}

fn wholly_styled(inlines: &[Inline], in_style: bool) -> bool {
    inlines.iter().all(|inline| match inline {
        Inline::Anchor { .. } | Inline::Code { .. } => true,
        Inline::Text { value } => value.trim().is_empty() || in_style,
        Inline::Strong { children } => wholly_styled(children, true),
        Inline::Emphasis { children } | Inline::Link { children, .. } => {
            wholly_styled(children, in_style)
        }
        Inline::LineBreak => false,
    })
}

fn man_head_evidence(
    native: &super::NativeManBlock,
    term: &[Inline],
    report: &NativeExecutionReport,
    projector: &InlineProjector<'_>,
) -> crate::definitions::ExactNativeDefinitionEvidence {
    let mut evidence = markup_evidence(&[native.head.node], report, projector);
    if evidence.role.is_some() || native.kind != ExecutionManBlockKind::IndentedParagraph {
        return evidence;
    }
    let text = mant_ir::inline_plain_text(term);
    let mut chars = text.trim().chars();
    let Some(mark) = chars.next() else {
        return evidence;
    };
    if chars.next().is_some() || !mark.is_ascii() || (mark.is_ascii_alphanumeric() && mark != 'o') {
        return evidence;
    }
    evidence.role = Some(if wholly_styled(term, false) {
        crate::definitions::NativeHeadRole::LiteralTerm
    } else {
        crate::definitions::NativeHeadRole::Presentation
    });
    evidence
}

fn document_origin_columns(report: &NativeExecutionReport) -> i32 {
    i32::try_from(report.content_indent_columns()).unwrap_or(i32::MAX)
}

fn retain_semantic_groups(blocks: &mut [Block], groupable: &HashSet<String>) {
    for block in blocks {
        match block {
            Block::DefinitionList {
                items,
                declaration_groups,
                ..
            } => {
                declaration_groups.retain(|group| {
                    group.start_item < group.end_item
                        && group.end_item <= items.len()
                        && items[group.start_item..group.end_item].iter().all(|item| {
                            item.entry
                                .as_ref()
                                .is_some_and(|facts| groupable.contains(facts.id.as_str()))
                        })
                });
                for item in items {
                    retain_semantic_groups(&mut item.description, groupable);
                }
            }
            Block::List { items, .. } => {
                for item in items {
                    retain_semantic_groups(&mut item.blocks, groupable);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                    retain_semantic_groups(&mut cell.blocks, groupable);
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

fn finish_projection(
    path: &Path,
    native: &libmandoc_rs::Document,
    native_diagnostics: &[libmandoc_rs::Diagnostic],
    root_blocks: Vec<Block>,
    section_plan: SectionPlan,
    receipts: Vec<NativeSemanticReceipt>,
    table_equation_budget_line: Option<u32>,
) -> NativeSemanticProjection {
    let mut document = Document {
        parser: Some(ParserInfo {
            name: "libmandoc".to_owned(),
            version: libmandoc_rs::LIBMANDOC_VERSION.to_owned(),
        }),
        source: DocumentSource {
            format: match native.macro_set {
                MacroSet::Mdoc => SourceFormat::Mdoc,
                MacroSet::Man | MacroSet::None => SourceFormat::Man,
            },
            path: Some(path.to_string_lossy().into_owned()),
        },
        meta: DocumentMeta {
            title: super::super::normalize_metadata(native.metadata.title.as_deref()),
            manual_section: super::super::normalize_metadata(native.metadata.section.as_deref()),
            date: super::super::normalize_metadata(native.metadata.date.as_deref()),
            volume: super::super::normalize_metadata(native.metadata.volume.as_deref()),
            os: super::super::normalize_metadata(native.metadata.os.as_deref()),
            arch: super::super::normalize_metadata(native.metadata.arch.as_deref()),
            names: super::super::normalize_metadata(native.metadata.name.as_deref())
                .into_iter()
                .collect(),
            alias_target: native.metadata.alias_target.clone(),
        },
        heading: None,
        fragment_aliases: Vec::new(),
        diagnostics: super::super::diagnostics::lower_diagnostics(native_diagnostics),
        blocks: root_blocks,
        sections: section_plan.sections,
    };
    super::super::navigation::promote_manual_references_in_document(
        &mut document.blocks,
        &mut document.sections,
    );
    if let Some(line) = table_equation_budget_line {
        document.diagnostics.push(mant_ir::Diagnostic {
            impact: mant_ir::DiagnosticImpact::None,
            level: mant_ir::DiagnosticLevel::Unsupported,
            code: Some("manual.inline-equation-budget".to_owned()),
            message: format!(
                "more than {} distinct inline table equations; later source spellings were retained without normalization",
                super::super::MAX_INLINE_EQUATION_NORMALIZATIONS
            ),
            source: Some(SourceSpan {
                byte_range: None,
                line,
                column: 1,
                end_line: None,
                end_column: None,
            }),
        });
    }
    document
        .diagnostics
        .extend(crate::producer_identity::outline_identity_diagnostics(
            &document.blocks,
            &document.sections,
            "roff",
        ));
    document
        .diagnostics
        .extend(crate::definitions::manual_discovery_diagnostics(
            &document.sections,
        ));
    document
        .diagnostics
        .extend(mant_ir::validate_document(&document));
    NativeSemanticProjection { document, receipts }
}

pub(super) fn project_semantics(
    path: &Path,
    native: &libmandoc_rs::Document,
    native_diagnostics: &[libmandoc_rs::Diagnostic],
    report: &NativeExecutionReport,
    projection: &NativeProjection,
) -> Result<NativeSemanticProjection, crate::mandoc::RoffProjectionError> {
    use crate::mandoc::{RoffProjectionError, RoffProjectionStage};
    let profile = std::env::var_os("MANT_NATIVE_PROFILE").is_some();
    let mut phase = std::time::Instant::now();
    let mark = |name: &str, phase: &mut std::time::Instant| {
        if profile {
            eprintln!("native-profile {name} {:?}", phase.elapsed());
        }
        *phase = std::time::Instant::now();
    };

    let mut section_plan = SectionPlan::build(&projection.headings, report);
    let anchor_ids = native_target_identities(&projection.anchors, &mut section_plan);
    let authored_sections = section_plan.authored.clone();
    let projector = InlineProjector::new(
        report,
        &projection.references,
        &projection.anchors,
        &anchor_ids,
        &authored_sections,
    );
    let (section_index, section_locations) =
        materializer::section_index(report, projection, &section_plan).map_err(|error| {
            RoffProjectionError::with_source(path, RoffProjectionStage::Ownership, error)
        })?;
    mark("semantic-setup", &mut phase);
    let arena =
        super::ownership::plan_native(report, projection, &section_index).map_err(|error| {
            RoffProjectionError::with_source(path, RoffProjectionStage::Ownership, error)
        })?;
    mark("ownership", &mut phase);
    let mut emitter = materializer::NativeRecipeEmitter::new(
        report, projection, &projector, &arena,
    )
    .map_err(|error| {
        RoffProjectionError::with_source(path, RoffProjectionStage::Materialization, error)
    })?;
    let mut materialized = materializer::materialize(&arena, &mut emitter).map_err(|error| {
        RoffProjectionError::with_source(path, RoffProjectionStage::Materialization, error)
    })?;
    mark("materialize", &mut phase);

    for (key, location) in &section_locations {
        let heading = materialized.headings.remove(key).ok_or_else(|| {
            RoffProjectionError::new(
                path,
                RoffProjectionStage::Materialization,
                format!("section {key:?} has no materialized heading"),
            )
        })?;
        section_plan.section_mut(*location).heading.content = heading;
        section_plan.section_mut(*location).blocks =
            materialized.sections.remove(key).unwrap_or_default();
        materialized
            .bindings
            .register_section_address(*key, section_coordinates(*location))
            .map_err(|error| {
                RoffProjectionError::with_source(path, RoffProjectionStage::Validation, error)
            })?;
    }
    if !materialized.headings.is_empty() || !materialized.sections.is_empty() {
        return Err(RoffProjectionError::new(
            path,
            RoffProjectionStage::Materialization,
            "materializer returned an unknown section destination",
        ));
    }
    let mut root_blocks = materialized.root;
    let binding_plan = materialized.bindings.seal().map_err(|error| {
        RoffProjectionError::with_source(path, RoffProjectionStage::Validation, error)
    })?;
    mark("section-bind", &mut phase);
    let target_aliases = anchor_ids
        .values()
        .flat_map(|(id, aliases)| {
            aliases
                .iter()
                .map(move |alias| (alias.as_str().to_owned(), id.to_string()))
        })
        .collect::<HashMap<_, _>>();
    let reserved_targets = anchor_ids
        .values()
        .flat_map(|(id, aliases)| {
            std::iter::once(id.to_string())
                .chain(aliases.iter().map(|alias| alias.as_str().to_owned()))
        })
        .chain(section_plan.sections.iter().flat_map(|section| {
            std::iter::once(section.id.to_string())
                .chain(
                    section
                        .fragment_aliases
                        .iter()
                        .map(|alias| alias.as_str().to_owned()),
                )
                .chain(section.children.iter().flat_map(|child| {
                    std::iter::once(child.id.to_string()).chain(
                        child
                            .fragment_aliases
                            .iter()
                            .map(|alias| alias.as_str().to_owned()),
                    )
                }))
        }))
        .collect::<HashSet<_>>();
    let authored_titles = section_plan.authored_titles.clone();
    let identities = binding_plan
        .identify(
            &mut root_blocks,
            &mut section_plan.sections,
            &reserved_targets,
            native.metadata.name.as_deref(),
            &target_aliases,
            &authored_titles,
        )
        .map_err(|error| {
            RoffProjectionError::with_source(path, RoffProjectionStage::Validation, error)
        })?;
    mark("identify", &mut phase);
    retain_semantic_groups(&mut root_blocks, &identities.groupable);
    for section in &mut section_plan.sections {
        retain_semantic_groups(&mut section.blocks, &identities.groupable);
        for child in &mut section.children {
            retain_semantic_groups(&mut child.blocks, &identities.groupable);
        }
    }
    let receipts = binding_plan.receipts(&identities).map_err(|error| {
        RoffProjectionError::with_source(path, RoffProjectionStage::Validation, error)
    })?;
    mark("receipts", &mut phase);
    let _retained_targets = identities.retained;

    let projection = finish_projection(
        path,
        native,
        native_diagnostics,
        root_blocks,
        section_plan,
        receipts,
        projection.table_equation_budget_line,
    );
    mark("finish", &mut phase);
    Ok(projection)
}

#[cfg(test)]
mod tests;
