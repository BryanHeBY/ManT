//! Bind native execution origins directly to source-neutral semantic owners.
//!
//! This private K19 route consumes native node, wrapper, atom, reference, and
//! anchor order. Device lines, rendered run boundaries, source coordinates,
//! pointers, slugs, and legacy marker anchors never select an owner.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::path::Path;

use libmandoc_rs::{
    AtomDisposition, AtomKind, AtomRole, ExecutionBoundary, ExecutionFont, ExecutionManBlockKind,
    ExecutionMdocListKind, ExecutionNodeKey, ExecutionReferenceKind, MacroSet,
    NativeExecutionReport, NodeKind,
};
use mant_ir::{
    Block, ContentBlockStep, DefinitionItem, DefinitionLayout, Document, DocumentMeta,
    DocumentSource, FragmentAlias, Heading, Inline, LayoutHint, LinkTarget, ListItem,
    ListItemLayout, ListKind, NodeId, ParserInfo, Section, SourceFormat, SourceSpan,
};

use super::{
    NativeAnchor, NativeHeadingFact, NativeHeadingKind, NativeMdocListItem, NativeProjection,
    NativeReference, native_node_wrapper_index,
};

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
    Root,
    Section { top: usize, child: Option<usize> },
}

#[derive(Clone, Debug)]
struct OwnerSlot {
    origins: Vec<ExecutionNodeKey>,
    location: Location,
    blocks: Vec<ContentBlockStep>,
    item_index: usize,
    evidence: crate::definitions::ExactNativeDefinitionEvidence,
}

#[derive(Clone, Debug)]
struct GroupCandidate {
    location: Location,
    blocks: Vec<ContentBlockStep>,
    start: usize,
    end: usize,
}

#[derive(Default)]
struct SectionPlan {
    sections: Vec<Section>,
    bodies: HashMap<ExecutionNodeKey, Location>,
    authored: HashMap<String, Option<NodeId>>,
    identities: IdentityAllocator,
    authored_titles: HashMap<String, String>,
    fragment_candidates: Vec<(Location, String, ExecutionNodeKey)>,
    heading_nodes: HashSet<ExecutionNodeKey>,
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
            let id = plan.identities.allocate(&format!(
                "section-{}",
                crate::producer_identity::section_id_base(authored)
            ));
            let source = source_span(report, heading.head);
            let section = Section {
                id: NodeId::new(id.clone()),
                fragment_aliases: Vec::new(),
                heading: Heading {
                    content: vec![Inline::Text {
                        value: heading.label.clone(),
                    }],
                    source,
                },
                spacing_before_lines: 0,
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
            plan.authored_titles.insert(id.clone(), authored.to_owned());
            plan.bodies.insert(heading.body, location);
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

    fn location_for(&self, report: &NativeExecutionReport, mut node: ExecutionNodeKey) -> Location {
        loop {
            if let Some(location) = self.bodies.get(&node) {
                return *location;
            }
            let Some(parent) = report.nodes()[node.0 as usize].parent else {
                return Location::Root;
            };
            node = parent;
        }
    }

    fn blocks(&self, location: Location) -> &[Block] {
        match location {
            Location::Root => &[],
            Location::Section { top, child: None } => &self.sections[top].blocks,
            Location::Section {
                top,
                child: Some(child),
            } => &self.sections[top].children[child].blocks,
        }
    }

    fn blocks_mut(&mut self, location: Location) -> &mut Vec<Block> {
        match location {
            Location::Root => unreachable!("document root is stored separately"),
            Location::Section { top, child: None } => &mut self.sections[top].blocks,
            Location::Section {
                top,
                child: Some(child),
            } => &mut self.sections[top].children[child].blocks,
        }
    }

    fn section_mut(&mut self, location: Location) -> &mut Section {
        match location {
            Location::Root => unreachable!("document root has no section identity"),
            Location::Section { top, child: None } => &mut self.sections[top],
            Location::Section {
                top,
                child: Some(child),
            } => &mut self.sections[top].children[child],
        }
    }
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

fn blocks_mut<'a>(
    root: &'a mut Vec<Block>,
    sections: &'a mut SectionPlan,
    location: Location,
) -> &'a mut Vec<Block> {
    match location {
        Location::Root => root,
        Location::Section { .. } => sections.blocks_mut(location),
    }
}

fn blocks<'a>(root: &'a [Block], sections: &'a SectionPlan, location: Location) -> &'a [Block] {
    match location {
        Location::Root => root,
        Location::Section { .. } => sections.blocks(location),
    }
}

fn resolve_location_block<'a>(
    root: &'a [Block],
    sections: &'a SectionPlan,
    location: Location,
    path: &[ContentBlockStep],
) -> Option<&'a Block> {
    mant_ir::resolve_content_block(blocks(root, sections, location), path)
}

fn resolve_block_mut<'a>(
    blocks: &'a mut [Block],
    path: &[ContentBlockStep],
) -> Option<&'a mut Block> {
    let (ContentBlockStep::Block { index }, rest) = path.split_first()? else {
        return None;
    };
    resolve_block_descendant_mut(blocks.get_mut(*index as usize)?, rest)
}

fn resolve_block_descendant_mut<'a>(
    block: &'a mut Block,
    path: &[ContentBlockStep],
) -> Option<&'a mut Block> {
    let Some((owner, rest)) = path.split_first() else {
        return Some(block);
    };
    let (ContentBlockStep::Block { index }, tail) = rest.split_first()? else {
        return None;
    };
    let children = match (block, *owner) {
        (Block::List { items, .. }, ContentBlockStep::ListItem { index }) => {
            &mut items.get_mut(index as usize)?.blocks
        }
        (Block::DefinitionList { items, .. }, ContentBlockStep::DefinitionItem { index }) => {
            &mut items.get_mut(index as usize)?.description
        }
        (Block::Table { rows, .. }, ContentBlockStep::TableCell { row, column }) => {
            &mut rows
                .get_mut(row as usize)?
                .cells
                .get_mut(column as usize)?
                .blocks
        }
        _ => return None,
    };
    resolve_block_descendant_mut(children.get_mut(*index as usize)?, tail)
}

fn section_coordinates(location: Location) -> Vec<u32> {
    match location {
        Location::Root => Vec::new(),
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
    let primary = String::from_utf8_lossy(&reference.primary).into_owned();
    Some(match reference.kind {
        ExecutionReferenceKind::ExternalUri => LinkTarget::External { uri: primary },
        ExecutionReferenceKind::Email => LinkTarget::Email { address: primary },
        ExecutionReferenceKind::Manual => LinkTarget::Manual {
            name: primary,
            manual_section: reference
                .secondary
                .as_deref()
                .map(|value| String::from_utf8_lossy(value).into_owned()),
        },
        ExecutionReferenceKind::SameDocumentSection => LinkTarget::Section {
            id: authored_sections.get(&primary)?.clone()?,
        },
    })
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
        let id = identities.allocate(&format!(
            "target-{}",
            crate::definitions::document_id_slug(&authored)
        ));
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
                let (id, aliases) = output
                    .get_mut(&key)
                    .expect("native target candidate has an allocated identity");
                if fragment != id.as_str() {
                    aliases.push(FragmentAlias::from(fragment));
                }
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
    reference_children: HashMap<Option<u32>, Vec<usize>>,
    anchors: HashMap<u32, &'a NativeAnchor>,
    anchor_ids: &'a HashMap<u32, (NodeId, Vec<FragmentAlias>)>,
    authored_sections: &'a HashMap<String, Option<NodeId>>,
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
        let mut reference_children = HashMap::<Option<u32>, Vec<usize>>::new();
        for (index, reference) in references.iter().enumerate() {
            reference_children
                .entry(reference.parent)
                .or_default()
                .push(index);
        }
        for children in reference_children.values_mut() {
            children.sort_unstable_by_key(|index| {
                let reference = &references[*index];
                (reference.atoms.start, reference.atoms.end)
            });
        }
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
            reference_children,
            anchors: anchors.iter().map(|value| (value.key, value)).collect(),
            anchor_ids,
            authored_sections,
        }
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
        let mut break_cursors = BTreeSet::new();
        for boundary in boundaries.iter().filter(|boundary| {
            matches!(
                boundary.request,
                libmandoc_rs::BoundaryRequest::Newline | libmandoc_rs::BoundaryRequest::Endline
            ) && matches!(
                boundary.effect,
                libmandoc_rs::BoundaryEffect::EndedLine
                    | libmandoc_rs::BoundaryEffect::AddedVerticalSpace
            )
        }) {
            let atoms = self
                .report
                .atoms()
                .get(range.start as usize..range.end as usize)
                .unwrap_or_default();
            let index = atoms.partition_point(|atom| atom.sequence <= boundary.leave_sequence);
            let cursor = atoms.get(index).map_or(range.end, |atom| atom.key.0);
            break_cursors.insert(cursor);
        }
        self.range(range, None, &anchor_cursors, &break_cursors, source)
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
        breaks: &BTreeSet<u32>,
        source: Option<SourceSpan>,
    ) -> Vec<Inline> {
        let mut output = Vec::new();
        let children = self
            .reference_children
            .get(&parent_reference)
            .map_or(&[][..], Vec::as_slice);
        let child_start =
            children.partition_point(|index| self.references[*index].atoms.start < range.start);
        let child_end =
            children.partition_point(|index| self.references[*index].atoms.start < range.end);
        let children = &children[child_start..child_end];
        let mut child = 0;
        let mut cursor = range.start;
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
            if breaks.contains(&cursor) && !matches!(output.last(), Some(Inline::LineBreak)) {
                output.push(Inline::LineBreak);
            }
            if let Some(reference) = children.get(child).map(|index| &self.references[*index])
                && reference.atoms.start == cursor
                && reference.atoms.end <= range.end
            {
                let nested = self.range(
                    reference.atoms.clone(),
                    Some(reference.key),
                    anchors,
                    breaks,
                    source,
                );
                if let Some(target) = reference_target(reference, self.authored_sections) {
                    output.push(Inline::Link {
                        target,
                        title: None,
                        children: nested,
                    });
                } else {
                    output.extend(nested);
                }
                cursor = reference.atoms.end;
                child += 1;
                continue;
            }
            let atom = &self.report.atoms()[cursor as usize];
            if let Some((font, value)) = visible_atom(atom) {
                push_styled_text(&mut output, font, value);
            }
            cursor += 1;
        }
        output
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
        if !matches!(output.last(), Some(Inline::LineBreak)) {
            output.push(Inline::LineBreak);
        }
        return;
    }
    match (font, output.last_mut()) {
        (ExecutionFont::Roman, Some(Inline::Text { value: current })) => current.push_str(&value),
        (ExecutionFont::Bold, Some(Inline::Strong { children }))
        | (ExecutionFont::Underline, Some(Inline::Emphasis { children }))
            if matches!(children.as_slice(), [Inline::Text { .. }]) =>
        {
            let Inline::Text { value: current } = &mut children[0] else {
                unreachable!()
            };
            current.push_str(&value);
        }
        _ => output.push(match font {
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
        }),
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

fn definition_list_mut(blocks: &mut [Block], index: usize) -> &mut Vec<DefinitionItem> {
    let Block::DefinitionList { items, .. } = &mut blocks[index] else {
        unreachable!("native definition block")
    };
    items
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

fn append_definition_list(blocks: &mut Vec<Block>, compact: bool) -> usize {
    let index = blocks.len();
    blocks.push(Block::DefinitionList {
        items: Vec::new(),
        declaration_groups: Vec::new(),
        compact,
        layout: LayoutHint::default(),
        source: None,
    });
    index
}

#[derive(Clone, Copy)]
struct ManActive {
    location: Location,
    block_index: usize,
    pending: Option<usize>,
    previous_empty: bool,
    flow_epoch: usize,
}

struct ProjectionOutput<'a> {
    root: &'a mut Vec<Block>,
    sections: &'a mut SectionPlan,
    slots: &'a mut Vec<OwnerSlot>,
}

fn merge_man_additional_tag(
    report: &NativeExecutionReport,
    projector: &InlineProjector<'_>,
    native: &super::NativeManBlock,
    active: ManActive,
    term: Vec<Inline>,
    description: Vec<Block>,
    output: &mut ProjectionOutput<'_>,
) -> ManActive {
    let ManActive {
        location,
        block_index,
        pending,
        flow_epoch,
        ..
    } = active;
    let mut additional = man_head_evidence(native, &term, report, projector);
    let items = definition_list_mut(
        blocks_mut(output.root, output.sections, location),
        block_index,
    );
    let item_index = items.len() - 1;
    items[item_index].terms.push(term);
    if !description.is_empty() {
        items[item_index].description = description;
    }
    let slot = output
        .slots
        .iter_mut()
        .rev()
        .find(|slot| {
            slot.location == location
                && slot.blocks
                    == [ContentBlockStep::Block {
                        index: u32::try_from(block_index).unwrap(),
                    }]
                && slot.item_index == item_index
        })
        .expect("native TQ continuation owner");
    slot.origins.push(native.owner);
    if additional.role.is_some() {
        slot.evidence.role = additional.role;
    }
    let term_index = items[item_index].terms.len() - 1;
    additional.shift_terms(term_index);
    slot.evidence.append(additional);
    ManActive {
        location,
        block_index,
        pending,
        previous_empty: items[item_index].description.is_empty(),
        flow_epoch,
    }
}

#[allow(clippy::too_many_arguments)]
fn append_man_definition(
    report: &NativeExecutionReport,
    projector: &InlineProjector<'_>,
    native: &super::NativeManBlock,
    location: Location,
    active: Option<ManActive>,
    term: Vec<Inline>,
    description: Vec<Block>,
    source: Option<SourceSpan>,
    layout: DefinitionLayout,
    root: &mut Vec<Block>,
    sections: &mut SectionPlan,
    slots: &mut Vec<OwnerSlot>,
    candidates: &mut Vec<GroupCandidate>,
) -> ManActive {
    let evidence = man_head_evidence(native, &term, report, projector);
    let (block_index, pending_start) = if let Some(active) = active {
        (active.block_index, active.pending)
    } else {
        let block_index = append_definition_list(blocks_mut(root, sections, location), true);
        (block_index, None)
    };
    let items = definition_list_mut(blocks_mut(root, sections, location), block_index);
    let item_index = items.len();
    let empty = description.is_empty();
    items.push(DefinitionItem {
        terms: vec![term],
        description,
        entry: None,
        source,
        layout,
    });
    slots.push(OwnerSlot {
        origins: vec![native.owner],
        location,
        blocks: vec![ContentBlockStep::Block {
            index: u32::try_from(block_index).unwrap(),
        }],
        item_index,
        evidence,
    });
    let next_pending = if empty {
        Some(pending_start.unwrap_or(item_index))
    } else {
        if let Some(start) = pending_start
            && start < item_index
        {
            candidates.push(GroupCandidate {
                location,
                blocks: vec![ContentBlockStep::Block {
                    index: u32::try_from(block_index).unwrap(),
                }],
                start,
                end: item_index + 1,
            });
        }
        None
    };
    ManActive {
        location,
        block_index,
        pending: next_pending,
        previous_empty: empty,
        flow_epoch: native.flow_epoch,
    }
}

fn project_man(
    report: &NativeExecutionReport,
    projection: &NativeProjection,
    projector: &InlineProjector<'_>,
    root: &mut Vec<Block>,
    sections: &mut SectionPlan,
    slots: &mut Vec<OwnerSlot>,
    candidates: &mut Vec<GroupCandidate>,
) {
    let layouts = projection
        .definitions
        .iter()
        .map(|definition| (definition.owner, definition.responsive.layout))
        .collect::<HashMap<_, _>>();
    let mut native_blocks = projection.man_blocks.iter().collect::<Vec<_>>();
    native_blocks.sort_by_key(|block| report.wrappers()[block.wrapper as usize].enter_sequence);
    let mut active: Option<ManActive> = None;

    for native in native_blocks {
        if !matches!(
            native.kind,
            ExecutionManBlockKind::IndentedParagraph
                | ExecutionManBlockKind::TaggedParagraph
                | ExecutionManBlockKind::AdditionalTag
        ) {
            active = None;
            continue;
        }
        let location = sections.location_for(report, native.owner);
        if active.is_some_and(|active| {
            active.location != location || active.flow_epoch != native.flow_epoch
        }) {
            active = None;
        }
        let source = source_span(report, native.owner);
        let term = projector.segment(
            native.head.atoms.clone(),
            &native.head.anchors,
            &native.head.boundaries,
            source,
        );
        let description = paragraph(
            projector.segment(
                native.body.atoms.clone(),
                &native.body.anchors,
                &native.body.boundaries,
                source,
            ),
            source,
        );
        let merge_tq = native.kind == ExecutionManBlockKind::AdditionalTag
            && active.is_some_and(|active| active.previous_empty);
        if merge_tq {
            active = Some(merge_man_additional_tag(
                report,
                projector,
                native,
                active.unwrap(),
                term,
                description,
                &mut ProjectionOutput {
                    root,
                    sections,
                    slots,
                },
            ));
            continue;
        }
        active = Some(append_man_definition(
            report,
            projector,
            native,
            location,
            active,
            term,
            description,
            source,
            *layouts
                .get(&native.owner)
                .expect("native definition layout"),
            root,
            sections,
            slots,
            candidates,
        ));
    }
}

fn project_mdoc(
    report: &NativeExecutionReport,
    projection: &NativeProjection,
    projector: &InlineProjector<'_>,
    root: &mut Vec<Block>,
    sections: &mut SectionPlan,
    slots: &mut Vec<OwnerSlot>,
) {
    let plan = MdocPlan::new(report, projection, projector);
    for list in &plan.roots {
        let native = &projection.mdoc_lists[*list];
        let location = sections.location_for(report, native.owner);
        let orphan_anchors = plan.orphan_anchor_block(*list);
        blocks_mut(root, sections, location).extend(orphan_anchors);
        let block_index = blocks_mut(root, sections, location).len();
        let path = vec![ContentBlockStep::Block {
            index: u32::try_from(block_index).unwrap(),
        }];
        let block = plan.materialize(*list, location, &path, slots);
        blocks_mut(root, sections, location).push(block);
    }
}

struct MdocPlan<'a> {
    report: &'a NativeExecutionReport,
    projection: &'a NativeProjection,
    projector: &'a InlineProjector<'a>,
    layouts: HashMap<ExecutionNodeKey, DefinitionLayout>,
    children: HashMap<(usize, usize), Vec<usize>>,
    ranges: Vec<Range<u32>>,
    list_anchors: Vec<ListLevelAnchors>,
    roots: Vec<usize>,
}

impl<'a> MdocPlan<'a> {
    fn new(
        report: &'a NativeExecutionReport,
        projection: &'a NativeProjection,
        projector: &'a InlineProjector<'a>,
    ) -> Self {
        let node_wrappers = native_node_wrapper_index(report);
        let ranges = projection
            .mdoc_lists
            .iter()
            .map(|list| {
                let wrapper = node_wrappers[list.owner.0 as usize]
                    .expect("validated native mdoc list wrapper");
                let wrapper = &report.wrappers()[wrapper];
                wrapper.enter_atom..wrapper.leave_atom
            })
            .collect::<Vec<_>>();
        let body_owners = projection
            .mdoc_lists
            .iter()
            .enumerate()
            .flat_map(|(list, value)| {
                value
                    .items
                    .iter()
                    .enumerate()
                    .flat_map(move |(item, value)| {
                        value
                            .bodies
                            .iter()
                            .map(move |body| (body.node, (list, item)))
                    })
            })
            .collect::<HashMap<_, _>>();
        let mut children = HashMap::<(usize, usize), Vec<usize>>::new();
        let mut roots = Vec::new();
        for (list, value) in projection.mdoc_lists.iter().enumerate() {
            let mut node = report.nodes()[value.owner.0 as usize].parent;
            let mut parent = None;
            while let Some(key) = node {
                if let Some(owner) = body_owners.get(&key) {
                    parent = Some(*owner);
                    break;
                }
                node = report.nodes()[key.0 as usize].parent;
            }
            if let Some(owner) = parent {
                children.entry(owner).or_default().push(list);
            } else {
                roots.push(list);
            }
        }
        // Execution node keys are validated dense DFS identities.  Sorting all
        // lists by that one structural order also places empty Bl owners; never
        // mix wrapper sequence numbers with node keys in the same key space.
        let structural_order = |list: &usize| projection.mdoc_lists[*list].owner;
        roots.sort_by_key(structural_order);
        for nested in children.values_mut() {
            nested.sort_by_key(structural_order);
        }
        let layouts = projection
            .definitions
            .iter()
            .map(|definition| (definition.owner, definition.responsive.layout))
            .collect();
        let mut anchors_by_parent = HashMap::<ExecutionNodeKey, Vec<&NativeAnchor>>::new();
        let mut anchors_by_node = HashMap::<ExecutionNodeKey, Vec<&NativeAnchor>>::new();
        for anchor in &projection.anchors {
            anchors_by_node.entry(anchor.node).or_default().push(anchor);
            if let Some(parent) = report.nodes()[anchor.node.0 as usize].parent {
                anchors_by_parent.entry(parent).or_default().push(anchor);
            }
        }
        for anchors in anchors_by_parent.values_mut() {
            anchors.sort_unstable_by_key(|anchor| anchor.node);
        }
        for anchors in anchors_by_node.values_mut() {
            anchors.sort_unstable_by_key(|anchor| anchor.key);
        }
        let mut body_by_parent = vec![None; report.nodes().len()];
        for node in report.nodes() {
            if node.kind == NodeKind::Body
                && let Some(parent) = node.parent
            {
                body_by_parent[parent.0 as usize] = Some(node.key);
            }
        }
        let list_anchors = projection
            .mdoc_lists
            .iter()
            .map(|list| {
                let body = mdoc_list_body(list, report, &body_by_parent);
                list_level_anchor_keys(
                    list,
                    anchors_by_node.get(&body).map_or(&[], Vec::as_slice),
                    anchors_by_parent.get(&body).map_or(&[], Vec::as_slice),
                )
            })
            .collect();
        Self {
            report,
            projection,
            projector,
            layouts,
            children,
            ranges,
            list_anchors,
            roots,
        }
    }

    fn materialize(
        &self,
        list_index: usize,
        location: Location,
        path: &[ContentBlockStep],
        slots: &mut Vec<OwnerSlot>,
    ) -> Block {
        let list = &self.projection.mdoc_lists[list_index];
        if is_definition_list(list.kind) {
            self.definition_list(list_index, location, path, slots)
        } else {
            self.ordinary_list(list_index, location, path, slots)
        }
    }

    fn orphan_anchor_block(&self, list_index: usize) -> Vec<Block> {
        let list = &self.projection.mdoc_lists[list_index];
        let keys = &self.list_anchors[list_index].leading;
        paragraph(
            self.projector
                .standalone_anchors(keys, source_span(self.report, list.owner)),
            source_span(self.report, list.owner),
        )
    }

    fn definition_list(
        &self,
        list_index: usize,
        location: Location,
        path: &[ContentBlockStep],
        slots: &mut Vec<OwnerSlot>,
    ) -> Block {
        let list = &self.projection.mdoc_lists[list_index];
        let anchors = &self.list_anchors[list_index];
        let mut items = Vec::new();
        let mut declaration_groups = Vec::new();
        let mut pending = None;
        let mut previous_epoch = None;
        for (item_index, item) in list.items.iter().enumerate() {
            if previous_epoch.is_some_and(|epoch| epoch != item.flow_epoch) {
                pending = None;
            }
            let source = source_span(self.report, item.owner);
            let mut term = self
                .projector
                .standalone_anchors(&anchors.by_item[item_index], source);
            term.extend(self.projector.segment(
                item.head.atoms.clone(),
                &item.head.anchors,
                &item.head.boundaries,
                source,
            ));
            let description = self.item_body(
                list_index,
                item_index,
                item,
                location,
                path,
                ContentBlockStep::DefinitionItem {
                    index: u32::try_from(item_index).unwrap(),
                },
                slots,
            );
            if description.is_empty() {
                pending = Some(pending.unwrap_or(item_index));
            } else if let Some(start_item) = pending.take()
                && start_item < item_index
            {
                declaration_groups.push(mant_ir::DeclarationGroup {
                    start_item,
                    end_item: item_index + 1,
                });
            }
            previous_epoch = Some(item.flow_epoch);
            items.push(DefinitionItem {
                terms: vec![term],
                description,
                entry: None,
                source,
                layout: *self
                    .layouts
                    .get(&item.owner)
                    .expect("native mdoc definition layout"),
            });
            slots.push(OwnerSlot {
                origins: vec![item.owner],
                location,
                blocks: path.to_vec(),
                item_index,
                evidence: markup_evidence(&[item.head.node], self.report, self.projector),
            });
        }
        Block::DefinitionList {
            items,
            declaration_groups,
            compact: list.compact,
            layout: LayoutHint::default(),
            source: source_span(self.report, list.owner),
        }
    }

    fn ordinary_list(
        &self,
        list_index: usize,
        location: Location,
        path: &[ContentBlockStep],
        slots: &mut Vec<OwnerSlot>,
    ) -> Block {
        let list = &self.projection.mdoc_lists[list_index];
        let anchors = &self.list_anchors[list_index];
        let mut items = Vec::new();
        for (item_index, item) in list.items.iter().enumerate() {
            let source = source_span(self.report, item.owner);
            let leading = self
                .projector
                .standalone_anchors(&anchors.by_item[item_index], source);
            let mut item_blocks = paragraph(leading, source);
            item_blocks.extend(self.item_body(
                list_index,
                item_index,
                item,
                location,
                path,
                ContentBlockStep::ListItem {
                    index: u32::try_from(item_index).unwrap(),
                },
                slots,
            ));
            items.push(ListItem {
                layout: ListItemLayout::default(),
                source,
                entry: None,
                blocks: item_blocks,
            });
        }
        Block::List {
            kind: ordinary_list_kind(list.kind).expect("non-definition native mdoc list"),
            items,
            compact: list.compact,
            layout: LayoutHint::default(),
            source: source_span(self.report, list.owner),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn item_body(
        &self,
        list_index: usize,
        item_index: usize,
        item: &NativeMdocListItem,
        location: Location,
        parent_path: &[ContentBlockStep],
        item_step: ContentBlockStep,
        slots: &mut Vec<OwnerSlot>,
    ) -> Vec<Block> {
        let nested = self
            .children
            .get(&(list_index, item_index))
            .map_or(&[][..], Vec::as_slice);
        let mut output = Vec::new();
        for body in &item.bodies {
            let mut cursor = body.atoms.start;
            for child in nested.iter().copied().filter(|child| {
                self.ranges[*child].start >= body.atoms.start
                    && self.ranges[*child].end <= body.atoms.end
            }) {
                output.extend(paragraph(
                    self.projector.segment(
                        cursor..self.ranges[child].start,
                        &body.anchors,
                        &body.boundaries,
                        source_span(self.report, item.owner),
                    ),
                    source_span(self.report, item.owner),
                ));
                let mut child_path = parent_path.to_vec();
                child_path.push(item_step);
                output.extend(self.orphan_anchor_block(child));
                child_path.push(ContentBlockStep::Block {
                    index: u32::try_from(output.len()).unwrap(),
                });
                output.push(self.materialize(child, location, &child_path, slots));
                cursor = self.ranges[child].end;
            }
            output.extend(paragraph(
                self.projector.segment(
                    cursor..body.atoms.end,
                    &body.anchors,
                    &body.boundaries,
                    source_span(self.report, item.owner),
                ),
                source_span(self.report, item.owner),
            ));
        }
        output
    }
}

struct ListLevelAnchors {
    leading: Vec<u32>,
    by_item: Vec<Vec<u32>>,
}

fn mdoc_list_body(
    list: &super::NativeMdocList,
    report: &NativeExecutionReport,
    body_by_parent: &[Option<ExecutionNodeKey>],
) -> ExecutionNodeKey {
    list.items
        .first()
        .and_then(|item| report.nodes()[item.owner.0 as usize].parent)
        .or(body_by_parent[list.owner.0 as usize])
        .expect("validated mdoc list body")
}

fn list_level_anchor_keys(
    list: &super::NativeMdocList,
    container_anchors: &[&NativeAnchor],
    anchors: &[&NativeAnchor],
) -> ListLevelAnchors {
    let mut output = ListLevelAnchors {
        leading: container_anchors.iter().map(|anchor| anchor.key).collect(),
        by_item: vec![Vec::new(); list.items.len()],
    };
    for anchor in anchors {
        let preceding_items = list.items.partition_point(|item| item.owner <= anchor.node);
        if preceding_items == 0 {
            output.leading.push(anchor.key);
        } else {
            output.by_item[preceding_items - 1].push(anchor.key);
        }
    }
    output
}

const fn is_definition_list(kind: ExecutionMdocListKind) -> bool {
    matches!(
        kind,
        ExecutionMdocListKind::Hang
            | ExecutionMdocListKind::Overhang
            | ExecutionMdocListKind::Inset
            | ExecutionMdocListKind::Diagnostic
            | ExecutionMdocListKind::Tag
    )
}

const fn ordinary_list_kind(kind: ExecutionMdocListKind) -> Option<ListKind> {
    match kind {
        ExecutionMdocListKind::Bullet
        | ExecutionMdocListKind::Dash
        | ExecutionMdocListKind::Hyphen => Some(ListKind::Bullet),
        ExecutionMdocListKind::Enum => Some(ListKind::Ordered { start: None }),
        ExecutionMdocListKind::Item | ExecutionMdocListKind::Column => Some(ListKind::Plain),
        ExecutionMdocListKind::Hang
        | ExecutionMdocListKind::Overhang
        | ExecutionMdocListKind::Inset
        | ExecutionMdocListKind::Diagnostic
        | ExecutionMdocListKind::Tag => None,
    }
}

fn evidence_in_document_order(
    root: &[Block],
    sections: &[Section],
    slots: &[OwnerSlot],
) -> Vec<crate::definitions::ExactNativeDefinitionEvidence> {
    let by_slot = slots
        .iter()
        .map(|slot| {
            (
                (slot.location, slot.blocks.clone(), slot.item_index),
                slot.evidence.clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut output = Vec::new();
    collect_evidence(root, Location::Root, &mut Vec::new(), &by_slot, &mut output);
    for (top, section) in sections.iter().enumerate() {
        collect_evidence(
            &section.blocks,
            Location::Section { top, child: None },
            &mut Vec::new(),
            &by_slot,
            &mut output,
        );
        for (child, section) in section.children.iter().enumerate() {
            collect_evidence(
                &section.blocks,
                Location::Section {
                    top,
                    child: Some(child),
                },
                &mut Vec::new(),
                &by_slot,
                &mut output,
            );
        }
    }
    output
}

fn collect_evidence(
    blocks: &[Block],
    location: Location,
    path: &mut Vec<ContentBlockStep>,
    by_slot: &HashMap<
        (Location, Vec<ContentBlockStep>, usize),
        crate::definitions::ExactNativeDefinitionEvidence,
    >,
    output: &mut Vec<crate::definitions::ExactNativeDefinitionEvidence>,
) {
    for (block_index, block) in blocks.iter().enumerate() {
        path.push(ContentBlockStep::Block {
            index: u32::try_from(block_index).unwrap(),
        });
        match block {
            Block::DefinitionList { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    output.push(
                        by_slot
                            .get(&(location, path.clone(), item_index))
                            .expect("exact native definition slot")
                            .clone(),
                    );
                    path.push(ContentBlockStep::DefinitionItem {
                        index: u32::try_from(item_index).unwrap(),
                    });
                    collect_evidence(&item.description, location, path, by_slot, output);
                    path.pop();
                }
            }
            Block::List { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    path.push(ContentBlockStep::ListItem {
                        index: u32::try_from(item_index).unwrap(),
                    });
                    collect_evidence(&item.blocks, location, path, by_slot, output);
                    path.pop();
                }
            }
            Block::Table { rows, .. } => {
                for (row_index, row) in rows.iter().enumerate() {
                    for (column_index, cell) in row.cells.iter().enumerate() {
                        path.push(ContentBlockStep::TableCell {
                            row: u32::try_from(row_index).unwrap(),
                            column: u32::try_from(column_index).unwrap(),
                        });
                        collect_evidence(&cell.blocks, location, path, by_slot, output);
                        path.pop();
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
        path.pop();
    }
}

fn apply_groups(
    root: &mut [Block],
    sections: &mut SectionPlan,
    candidates: &[GroupCandidate],
    groupable: &HashSet<String>,
) {
    for candidate in candidates {
        let block = match candidate.location {
            Location::Root => resolve_block_mut(root, &candidate.blocks),
            Location::Section { .. } => {
                resolve_block_mut(sections.blocks_mut(candidate.location), &candidate.blocks)
            }
        };
        let Some(block) = block else { continue };
        let Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = block
        else {
            continue;
        };
        if items[candidate.start..candidate.end].iter().all(|item| {
            item.entry
                .as_ref()
                .is_some_and(|facts| groupable.contains(facts.id.as_str()))
        }) {
            declaration_groups.push(mant_ir::DeclarationGroup {
                start_item: candidate.start,
                end_item: candidate.end,
            });
        }
    }
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
    root_blocks: Vec<Block>,
    section_plan: SectionPlan,
    slots: &[OwnerSlot],
) -> NativeSemanticProjection {
    let receipts = slots
        .iter()
        .filter_map(|slot| {
            let Block::DefinitionList { items, .. } =
                resolve_location_block(&root_blocks, &section_plan, slot.location, &slot.blocks)?
            else {
                return None;
            };
            let facts = items[slot.item_index].entry.as_ref()?;
            Some(NativeSemanticReceipt {
                origins: slot.origins.clone(),
                id: facts.id.clone(),
                sections: section_coordinates(slot.location),
                blocks: slot.blocks.clone(),
                item_index: u32::try_from(slot.item_index).unwrap(),
            })
        })
        .collect();
    let mut document = Document {
        parser: Some(ParserInfo {
            name: "libmandoc-native-staged".to_owned(),
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
            title: native.metadata.title.clone(),
            manual_section: native.metadata.section.clone(),
            date: native.metadata.date.clone(),
            volume: native.metadata.volume.clone(),
            os: native.metadata.os.clone(),
            arch: native.metadata.arch.clone(),
            names: native.metadata.name.clone().into_iter().collect(),
            alias_target: native.metadata.alias_target.clone(),
        },
        heading: None,
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
        blocks: root_blocks,
        sections: section_plan.sections,
    };
    document
        .diagnostics
        .extend(crate::producer_identity::outline_identity_diagnostics(
            &document.blocks,
            &document.sections,
            "roff",
        ));
    document
        .diagnostics
        .extend(mant_ir::validate_document(&document));
    NativeSemanticProjection { document, receipts }
}

pub(super) fn project_semantics(
    path: &Path,
    native: &libmandoc_rs::Document,
    report: &NativeExecutionReport,
    projection: &NativeProjection,
) -> NativeSemanticProjection {
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
    let mut root_blocks = Vec::new();
    let mut slots = Vec::new();
    let mut candidates = Vec::new();
    match native.macro_set {
        MacroSet::Man | MacroSet::None => project_man(
            report,
            projection,
            &projector,
            &mut root_blocks,
            &mut section_plan,
            &mut slots,
            &mut candidates,
        ),
        MacroSet::Mdoc => project_mdoc(
            report,
            projection,
            &projector,
            &mut root_blocks,
            &mut section_plan,
            &mut slots,
        ),
    }
    let evidence = evidence_in_document_order(&root_blocks, &section_plan.sections, &slots);
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
    let identities = crate::definitions::identify_exact_native_definitions(
        &mut root_blocks,
        &mut section_plan.sections,
        &reserved_targets,
        native.metadata.name.as_deref(),
        evidence,
        &target_aliases,
        &authored_titles,
    );
    apply_groups(
        &mut root_blocks,
        &mut section_plan,
        &candidates,
        &identities.groupable,
    );
    retain_semantic_groups(&mut root_blocks, &identities.groupable);
    for section in &mut section_plan.sections {
        retain_semantic_groups(&mut section.blocks, &identities.groupable);
        for child in &mut section.children {
            retain_semantic_groups(&mut child.blocks, &identities.groupable);
        }
    }
    let _retained_targets = identities.retained;

    finish_projection(path, native, root_blocks, section_plan, &slots)
}

#[cfg(test)]
mod tests;
