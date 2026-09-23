//! Bounded closure and dense remapping for response-local content stores.

use std::{collections::VecDeque, error::Error, fmt};

use crate::{
    Block, ContentAtomKey, ContentOwnerKey, ContentPointKey, ContentProjection, ContentRootKey,
    ContentStore, DecorationKey, FixedLineKey, FixedViewKey, Heading, Inline, LinkLabelPart,
    LinkOccurrenceKey, PlacementKey, PlacementTarget, Section,
};

const MAX_PROJECTION_OBJECTS: usize = 1_000_000;
const MAX_PROJECTION_EDGES: usize = 8_000_000;
const MAX_PROJECTION_BYTES: usize = 32 * 1024 * 1024;
const MAX_PROJECTION_STEPS: usize = 16_000_000;

/// A bounded response-local projection cannot be closed or remapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentProjectionError(&'static str);

impl fmt::Display for ContentProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for ContentProjectionError {}

#[derive(Clone, Copy)]
enum Edge {
    Root(ContentRootKey),
    Link(LinkOccurrenceKey),
}

#[derive(Clone, Copy)]
enum Selection {
    Owner(ContentOwnerKey),
    Root(ContentRootKey),
    Atom(ContentAtomKey),
    Point(ContentPointKey),
    Link(LinkOccurrenceKey),
    FixedView(FixedViewKey),
}

/// Selection state needed to undo one speculative projection admission.
/// Work already performed is deliberately not refundable.
#[derive(Debug)]
pub struct ContentProjectionCheckpoint {
    generation: usize,
    snapshot_generation: Option<usize>,
}

/// Collect a fixed-point closure from retained topology before one dense remap.
#[derive(Clone)]
pub struct ContentProjectionBuilder<'a> {
    source: &'a ContentStore,
    owners: Vec<bool>,
    roots: Vec<bool>,
    atoms: Vec<bool>,
    points: Vec<bool>,
    links: Vec<bool>,
    fixed_views: Vec<bool>,
    queue: VecDeque<Edge>,
    steps: usize,
    snapshot_work: usize,
    generation: usize,
    snapshot_generation: Option<usize>,
    trial_active: bool,
    trial_selections: Vec<Selection>,
}

impl<'a> ContentProjectionBuilder<'a> {
    /// Start an empty projection over one source document store.
    #[must_use]
    pub fn new(source: &'a ContentStore) -> Self {
        Self {
            source,
            owners: vec![false; source.owners.len()],
            roots: vec![false; source.roots.len()],
            atoms: vec![false; source.atoms.len()],
            points: vec![false; source.points.len()],
            links: vec![false; source.links.len()],
            fixed_views: vec![false; source.fixed_views.len()],
            queue: VecDeque::new(),
            steps: 0,
            snapshot_work: 0,
            generation: 0,
            snapshot_generation: None,
            trial_active: false,
            trial_selections: Vec::new(),
        }
    }

    /// Start one speculative admission without copying source-sized bitsets.
    /// A caller must commit or roll back before starting another trial.
    ///
    /// # Errors
    ///
    /// Returns an error if a trial is already active.
    pub fn checkpoint(&mut self) -> Result<ContentProjectionCheckpoint, ContentProjectionError> {
        if self.trial_active || !self.queue.is_empty() {
            return Err(ContentProjectionError(
                "projection trial requires a closed idle builder",
            ));
        }
        self.trial_active = true;
        Ok(ContentProjectionCheckpoint {
            generation: self.generation,
            snapshot_generation: self.snapshot_generation,
        })
    }

    /// Keep a successful speculative admission.
    pub fn commit(&mut self, _checkpoint: ContentProjectionCheckpoint) {
        self.trial_selections.clear();
        self.trial_active = false;
    }

    /// Restore selected objects and closure state after failure. Traversal and
    /// snapshot work remain charged even when the result is discarded.
    #[allow(clippy::needless_pass_by_value)] // Consumes the single-use trial token.
    pub fn rollback(&mut self, checkpoint: ContentProjectionCheckpoint) {
        for selection in self.trial_selections.drain(..).rev() {
            let (bits, key) = match selection {
                Selection::Owner(key) => (&mut self.owners, key.get()),
                Selection::Root(key) => (&mut self.roots, key.get()),
                Selection::Atom(key) => (&mut self.atoms, key.get()),
                Selection::Point(key) => (&mut self.points, key.get()),
                Selection::Link(key) => (&mut self.links, key.get()),
                Selection::FixedView(key) => (&mut self.fixed_views, key.get()),
            };
            if let Ok(index) = usize::try_from(key - 1)
                && let Some(selected) = bits.get_mut(index)
            {
                *selected = false;
            }
        }
        self.queue.clear();
        self.generation = checkpoint.generation;
        self.snapshot_generation = checkpoint.snapshot_generation;
        self.trial_active = false;
    }

    fn newly_selected(&mut self, selection: Selection) {
        self.generation = self.generation.saturating_add(1);
        if self.trial_active {
            self.trial_selections.push(selection);
        }
    }

    /// Retain every store relation reachable from inline topology.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid keys or exhausted projection bounds.
    pub fn include_inlines(&mut self, nodes: &[Inline]) -> Result<(), ContentProjectionError> {
        for node in nodes {
            self.step()?;
            match node {
                Inline::Text { content } | Inline::Code { content } => {
                    self.include_atom(content.atom)?;
                }
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    self.include_inlines(children)?;
                }
                Inline::Link {
                    occurrence,
                    children,
                } => {
                    self.include_link(*occurrence)?;
                    self.include_inlines(children)?;
                }
                Inline::Anchor { point, .. } => self.include_point(*point)?,
                Inline::LineBreak { atom } => self.include_atom(*atom)?,
            }
        }
        Ok(())
    }

    /// Retain every relation reachable from a heading.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid keys or exhausted projection bounds.
    pub fn include_heading(&mut self, heading: &Heading) -> Result<(), ContentProjectionError> {
        self.include_inlines(&heading.content)
    }

    /// Retain every relation reachable from nested blocks.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid keys or exhausted projection bounds.
    pub fn include_blocks(&mut self, blocks: &[Block]) -> Result<(), ContentProjectionError> {
        for block in blocks {
            self.step()?;
            match block {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    self.include_inlines(children)?;
                }
                Block::FixedDisplay { children, view, .. } => {
                    self.include_fixed_view(*view)?;
                    self.include_inlines(children)?;
                }
                Block::List { items, .. } => {
                    for item in items {
                        self.include_blocks(&item.blocks)?;
                    }
                }
                Block::DefinitionList { items, .. } => self.include_definition_items(items)?,
                Block::Table {
                    rows, fixed_view, ..
                } => {
                    if let Some(view) = fixed_view {
                        self.include_fixed_view(*view)?;
                    }
                    for cell in rows.iter().flat_map(|row| &row.cells) {
                        if let Some(point) = cell.point {
                            self.include_point(point)?;
                        }
                        self.include_blocks(&cell.blocks)?;
                    }
                }
                Block::Equation { .. }
                | Block::VerticalSpace { .. }
                | Block::ThematicBreak { .. }
                | Block::Unsupported { .. } => {}
            }
        }
        Ok(())
    }

    /// Retain exactly the borrowed items selected for one declaration group.
    ///
    /// This is distinct from including the enclosing source block: the latter
    /// would pull unrelated sibling terms into a response-local projection.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid keys or exhausted projection bounds.
    pub fn include_definition_items(
        &mut self,
        items: &[crate::DefinitionItem],
    ) -> Result<(), ContentProjectionError> {
        for item in items {
            self.step()?;
            for term in &item.terms {
                self.include_inlines(term)?;
            }
            self.include_blocks(&item.description)?;
        }
        Ok(())
    }

    /// Retain a complete section subtree.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid keys or exhausted projection bounds.
    pub fn include_section(&mut self, section: &Section) -> Result<(), ContentProjectionError> {
        self.include_heading(&section.heading)?;
        self.include_blocks(&section.blocks)?;
        for child in &section.children {
            self.include_section(child)?;
        }
        Ok(())
    }

    /// Close all owner/root/link edges and build a dense response-local store.
    ///
    /// # Errors
    ///
    /// Returns an error when closure, budgets, or dense remapping fail.
    #[allow(clippy::too_many_lines)]
    pub fn finish(
        mut self,
    ) -> Result<(ContentProjection, ContentKeyRemap), ContentProjectionError> {
        self.snapshot()
    }

    /// Close current edges and snapshot the selected closure without consuming
    /// the builder. Further selected roots may then extend the same closure.
    ///
    /// # Errors
    ///
    /// Returns an error when closure, budgets, or dense remapping fail.
    #[allow(clippy::too_many_lines)]
    pub fn snapshot(
        &mut self,
    ) -> Result<(ContentProjection, ContentKeyRemap), ContentProjectionError> {
        while let Some(edge) = self.queue.pop_front() {
            self.step()?;
            match edge {
                Edge::Root(key) => {
                    let edge_work = {
                        let root = self
                            .source
                            .root(key)
                            .ok_or(ContentProjectionError("projection root key is invalid"))?;
                        root.atoms.len().saturating_add(root.points.len())
                    };
                    self.charge_snapshot_work(edge_work)?;
                    let root = self
                        .source
                        .root(key)
                        .ok_or(ContentProjectionError("projection root key is invalid"))?;
                    self.include_owner(root.owner)?;
                    for &atom in &root.atoms {
                        self.include_atom(atom)?;
                    }
                    for &point in &root.points {
                        self.include_point(point)?;
                    }
                }
                Edge::Link(key) => {
                    let edge_work = self
                        .source
                        .link(key)
                        .ok_or(ContentProjectionError("projection link key is invalid"))?
                        .label
                        .len();
                    self.charge_snapshot_work(edge_work)?;
                    let link = self
                        .source
                        .link(key)
                        .ok_or(ContentProjectionError("projection link key is invalid"))?;
                    self.include_owner(link.owner)?;
                    for part in &link.label {
                        let atom = match part {
                            LinkLabelPart::Content { content } => content.atom,
                            LinkLabelPart::HardBreak { atom } => *atom,
                        };
                        self.include_atom(atom)?;
                    }
                }
            }
        }

        let source_objects = self
            .source
            .owners
            .len()
            .saturating_add(self.source.roots.len())
            .saturating_add(self.source.atoms.len())
            .saturating_add(self.source.points.len())
            .saturating_add(self.source.links.len())
            .saturating_add(self.source.fixed_views.len());
        // Each scan is charged before it runs, including a trial that is
        // rejected by an object, edge, or byte limit below.
        self.charge_snapshot_work(source_objects)?;
        let fixed_objects = self
            .source
            .fixed_views
            .iter()
            .filter(|view| selected(&self.fixed_views, view.key.get()))
            .flat_map(|view| view.lines.iter())
            .fold(0_usize, |total, line| {
                total
                    .saturating_add(1)
                    .saturating_add(line.placements.len())
                    .saturating_add(line.decorations.len())
            });
        self.charge_snapshot_work(fixed_objects)?;
        let object_count = count(&self.owners)
            .saturating_add(count(&self.roots))
            .saturating_add(count(&self.atoms))
            .saturating_add(count(&self.points))
            .saturating_add(count(&self.links))
            .saturating_add(count(&self.fixed_views))
            .saturating_add(fixed_objects);
        if object_count > MAX_PROJECTION_OBJECTS {
            return Err(ContentProjectionError(
                "content projection exceeds the object limit",
            ));
        }

        self.charge_snapshot_work(source_objects)?;
        let owner_root_edges = self
            .source
            .owners
            .iter()
            .filter(|record| selected(&self.owners, record.key.get()))
            .map(|record| record.roots.len())
            .fold(0_usize, usize::saturating_add);
        self.charge_snapshot_work(owner_root_edges)?;
        let retained_edges = self
            .source
            .owners
            .iter()
            .filter(|record| selected(&self.owners, record.key.get()))
            .map(|record| {
                record
                    .roots
                    .iter()
                    .filter(|root| selected(&self.roots, root.get()))
                    .count()
            })
            .chain(
                self.source
                    .roots
                    .iter()
                    .filter(|record| selected(&self.roots, record.key.get()))
                    .map(|record| record.atoms.len().saturating_add(record.points.len())),
            )
            .chain(
                self.source
                    .links
                    .iter()
                    .filter(|record| selected(&self.links, record.key.get()))
                    .map(|record| record.label.len()),
            )
            .chain(
                self.source
                    .fixed_views
                    .iter()
                    .filter(|view| selected(&self.fixed_views, view.key.get()))
                    .map(|view| {
                        view.lines.iter().fold(0_usize, |total, line| {
                            total
                                .saturating_add(1)
                                .saturating_add(line.placements.len())
                                .saturating_add(line.decorations.len())
                        })
                    }),
            )
            .fold(0_usize, usize::saturating_add);
        if retained_edges > MAX_PROJECTION_EDGES {
            return Err(ContentProjectionError(
                "content projection exceeds the edge limit",
            ));
        }
        self.charge_snapshot_work(source_objects)?;
        let retained_bytes = self
            .source
            .atoms
            .iter()
            .filter(|record| selected(&self.atoms, record.key.get()))
            .map(|record| {
                record
                    .kind
                    .text()
                    .map_or(0, str::len)
                    .saturating_add(match &record.kind {
                        crate::ContentAtomKind::Text {
                            display_override, ..
                        }
                        | crate::ContentAtomKind::Whitespace {
                            display_override, ..
                        } => display_override.as_deref().map_or(0, str::len),
                        crate::ContentAtomKind::BreakOpportunity {}
                        | crate::ContentAtomKind::HardBreak {} => 0,
                    })
            })
            .chain(
                self.source
                    .links
                    .iter()
                    .filter(|record| selected(&self.links, record.key.get()))
                    .map(|record| {
                        link_target_bytes(&record.target)
                            .saturating_add(record.title.as_deref().map_or(0, str::len))
                    }),
            )
            .chain(
                self.source
                    .fixed_views
                    .iter()
                    .filter(|view| selected(&self.fixed_views, view.key.get()))
                    .flat_map(|view| view.lines.iter())
                    .flat_map(|line| line.decorations.iter())
                    .map(|decoration| decoration.text.len()),
            )
            .fold(0_usize, usize::saturating_add);
        if retained_bytes > MAX_PROJECTION_BYTES {
            return Err(ContentProjectionError(
                "content projection exceeds the retained-byte limit",
            ));
        }
        // Snapshot clones these bytes, validation scans them, and admission
        // serializes the candidate. Charge that unavoidable linear work even
        // when a later response-byte check discards the candidate.
        let byte_work = (retained_bytes.saturating_add(63) / 64).saturating_mul(3);
        self.charge_snapshot_work(byte_work)?;
        let remap_steps = object_count
            .saturating_add(retained_edges)
            .saturating_mul(2)
            .saturating_add(source_objects);
        // Reserve the full remap/validation work before allocating anything.
        // A malformed source can fail later, but cannot reuse that computation.
        self.charge_snapshot_work(remap_steps)?;

        let remap = ContentKeyRemap {
            owners: dense_map(&self.owners, ContentOwnerKey::new)?,
            roots: dense_map(&self.roots, ContentRootKey::new)?,
            atoms: dense_map(&self.atoms, ContentAtomKey::new)?,
            points: dense_map(&self.points, ContentPointKey::new)?,
            links: dense_map(&self.links, LinkOccurrenceKey::new)?,
            fixed_views: dense_map(&self.fixed_views, FixedViewKey::new)?,
        };
        let owners = self
            .source
            .owners
            .iter()
            .filter(|record| selected(&self.owners, record.key.get()))
            .map(|record| {
                let mut record = record.clone();
                record.key = remap.owner(record.key)?;
                record.roots = record
                    .roots
                    .iter()
                    .filter(|key| selected(&self.roots, key.get()))
                    .map(|&key| remap.root(key))
                    .collect::<Result<_, _>>()?;
                Ok(record)
            })
            .collect::<Result<Vec<_>, ContentProjectionError>>()?;
        let roots = self
            .source
            .roots
            .iter()
            .filter(|record| selected(&self.roots, record.key.get()))
            .map(|record| {
                let mut record = record.clone();
                record.key = remap.root(record.key)?;
                record.owner = remap.owner(record.owner)?;
                record.atoms = record
                    .atoms
                    .iter()
                    .map(|&key| remap.atom(key))
                    .collect::<Result<_, _>>()?;
                record.points = record
                    .points
                    .iter()
                    .map(|&key| remap.point(key))
                    .collect::<Result<_, _>>()?;
                Ok(record)
            })
            .collect::<Result<Vec<_>, ContentProjectionError>>()?;
        let atoms = self
            .source
            .atoms
            .iter()
            .filter(|record| selected(&self.atoms, record.key.get()))
            .map(|record| {
                let mut record = record.clone();
                record.key = remap.atom(record.key)?;
                record.root = remap.root(record.root)?;
                record.owner = remap.owner(record.owner)?;
                record.link = record.link.map(|key| remap.link(key)).transpose()?;
                Ok(record)
            })
            .collect::<Result<Vec<_>, ContentProjectionError>>()?;
        let points = self
            .source
            .points
            .iter()
            .filter(|record| selected(&self.points, record.key.get()))
            .map(|record| {
                let mut record = record.clone();
                record.key = remap.point(record.key)?;
                record.root = remap.root(record.root)?;
                record.owner = remap.owner(record.owner)?;
                if let crate::PointBoundary::InAtom { atom, .. } = &mut record.boundary {
                    *atom = remap.atom(*atom)?;
                }
                Ok(record)
            })
            .collect::<Result<Vec<_>, ContentProjectionError>>()?;
        let links = self
            .source
            .links
            .iter()
            .filter(|record| selected(&self.links, record.key.get()))
            .map(|record| {
                let mut record = record.clone();
                record.key = remap.link(record.key)?;
                record.owner = remap.owner(record.owner)?;
                for part in &mut record.label {
                    match part {
                        LinkLabelPart::Content { content } => {
                            content.atom = remap.atom(content.atom)?;
                        }
                        LinkLabelPart::HardBreak { atom } => *atom = remap.atom(*atom)?,
                    }
                }
                Ok(record)
            })
            .collect::<Result<Vec<_>, ContentProjectionError>>()?;

        let mut next_line = 0_usize;
        let mut next_placement = 0_usize;
        let mut next_decoration = 0_usize;
        let fixed_views = self
            .source
            .fixed_views
            .iter()
            .filter(|view| selected(&self.fixed_views, view.key.get()))
            .map(|view| {
                let mut view = view.clone();
                view.key = remap.fixed_view(view.key)?;
                view.owner = remap.owner(view.owner)?;
                for line in &mut view.lines {
                    next_line = next_line.saturating_add(1);
                    line.key = FixedLineKey::new(
                        u32::try_from(next_line)
                            .map_err(|_| ContentProjectionError("fixed line key overflow"))?,
                    )
                    .ok_or(ContentProjectionError("fixed line key overflow"))?;
                    for placement in &mut line.placements {
                        next_placement = next_placement.saturating_add(1);
                        placement.key =
                            PlacementKey::new(u32::try_from(next_placement).map_err(|_| {
                                ContentProjectionError("fixed placement key overflow")
                            })?)
                            .ok_or(ContentProjectionError("fixed placement key overflow"))?;
                        match &mut placement.target {
                            PlacementTarget::Content(content) => {
                                content.atom = remap.atom(content.atom)?;
                            }
                            PlacementTarget::Point(point) => *point = remap.point(*point)?,
                        }
                    }
                    for decoration in &mut line.decorations {
                        next_decoration = next_decoration.saturating_add(1);
                        decoration.key =
                            DecorationKey::new(u32::try_from(next_decoration).map_err(|_| {
                                ContentProjectionError("fixed decoration key overflow")
                            })?)
                            .ok_or(ContentProjectionError("fixed decoration key overflow"))?;
                    }
                }
                Ok(view)
            })
            .collect::<Result<Vec<_>, ContentProjectionError>>()?;

        let content_store = ContentStore {
            owners,
            roots,
            atoms,
            points,
            links,
            fixed_views,
        };
        crate::validate_content_store(&content_store)
            .map_err(|_| ContentProjectionError("constructed projection is not closed"))?;
        self.snapshot_generation = Some(self.generation);
        Ok((ContentProjection { content_store }, remap))
    }

    /// Whether the last successful snapshot still describes this exact closure.
    /// Include traversal may have spent more steps even without selecting a new
    /// object. Previously completed remaps remain charged to this builder.
    #[must_use]
    pub fn can_reuse_snapshot(&self) -> bool {
        self.snapshot_generation == Some(self.generation)
            && self.queue.is_empty()
            && self.steps.saturating_add(self.snapshot_work) <= MAX_PROJECTION_STEPS
    }

    fn include_owner(&mut self, key: ContentOwnerKey) -> Result<(), ContentProjectionError> {
        // An owner is metadata for selected roots, not a request to copy all
        // sibling roots in the original document.  Its response-local roots
        // are filtered during dense remap.
        if mark(
            &mut self.owners,
            key.get(),
            "projection owner key is invalid",
        )? {
            self.newly_selected(Selection::Owner(key));
        }
        Ok(())
    }

    /// Retain one complete logical root and its fixed-point closure.
    ///
    /// # Errors
    ///
    /// Returns an error when the root is invalid or work is exhausted.
    pub fn include_root(&mut self, key: ContentRootKey) -> Result<(), ContentProjectionError> {
        if mark(&mut self.roots, key.get(), "projection root key is invalid")? {
            self.newly_selected(Selection::Root(key));
            self.queue.push_back(Edge::Root(key));
        }
        Ok(())
    }

    fn include_atom(&mut self, key: ContentAtomKey) -> Result<(), ContentProjectionError> {
        let Some(atom) = self.source.atom(key) else {
            return Err(ContentProjectionError("projection atom key is invalid"));
        };
        if mark(&mut self.atoms, key.get(), "projection atom key is invalid")? {
            self.newly_selected(Selection::Atom(key));
            self.include_root(atom.root)?;
            if let Some(link) = atom.link {
                self.include_link(link)?;
            }
        }
        Ok(())
    }

    fn include_point(&mut self, key: ContentPointKey) -> Result<(), ContentProjectionError> {
        let Some(point) = self.source.point(key) else {
            return Err(ContentProjectionError("projection point key is invalid"));
        };
        if mark(
            &mut self.points,
            key.get(),
            "projection point key is invalid",
        )? {
            self.newly_selected(Selection::Point(key));
            self.include_root(point.root)?;
        }
        Ok(())
    }

    /// Retain one logical link occurrence and its fixed-point closure.
    ///
    /// This is the only complete entry point for an occurrence whose authored
    /// label is empty and therefore has no atom or root from which to discover
    /// the link relation.
    ///
    /// # Errors
    ///
    /// Returns an error when the occurrence is invalid or work is exhausted.
    pub fn include_link(&mut self, key: LinkOccurrenceKey) -> Result<(), ContentProjectionError> {
        if mark(&mut self.links, key.get(), "projection link key is invalid")? {
            self.newly_selected(Selection::Link(key));
            self.queue.push_back(Edge::Link(key));
        }
        Ok(())
    }

    fn include_fixed_view(&mut self, key: FixedViewKey) -> Result<(), ContentProjectionError> {
        if !mark(
            &mut self.fixed_views,
            key.get(),
            "projection fixed view key is invalid",
        )? {
            return Ok(());
        }
        self.newly_selected(Selection::FixedView(key));
        let view = self.source.fixed_view(key).ok_or(ContentProjectionError(
            "projection fixed view key is invalid",
        ))?;
        let work = view.lines.iter().fold(1_usize, |total, line| {
            total
                .saturating_add(1)
                .saturating_add(line.placements.len())
                .saturating_add(line.decorations.len())
        });
        self.charge_snapshot_work(work)?;
        self.include_owner(view.owner)?;
        for line in &view.lines {
            for placement in &line.placements {
                match placement.target {
                    PlacementTarget::Content(content) => self.include_atom(content.atom)?,
                    PlacementTarget::Point(point) => self.include_point(point)?,
                }
            }
        }
        Ok(())
    }

    fn step(&mut self) -> Result<(), ContentProjectionError> {
        self.steps = self.steps.saturating_add(1);
        if self.steps.saturating_add(self.snapshot_work) > MAX_PROJECTION_STEPS {
            return Err(ContentProjectionError(
                "content projection exceeds the step limit",
            ));
        }
        Ok(())
    }

    fn charge_snapshot_work(&mut self, work: usize) -> Result<(), ContentProjectionError> {
        if self
            .steps
            .saturating_add(self.snapshot_work)
            .saturating_add(work)
            > MAX_PROJECTION_STEPS
        {
            return Err(ContentProjectionError(
                "content projection exceeds the step limit",
            ));
        }
        self.snapshot_work = self.snapshot_work.saturating_add(work);
        Ok(())
    }
}

/// Dense old-to-response-local key mapping produced with a projection.
#[derive(Debug, Clone)]
pub struct ContentKeyRemap {
    owners: Vec<Option<ContentOwnerKey>>,
    roots: Vec<Option<ContentRootKey>>,
    atoms: Vec<Option<ContentAtomKey>>,
    points: Vec<Option<ContentPointKey>>,
    links: Vec<Option<LinkOccurrenceKey>>,
    fixed_views: Vec<Option<FixedViewKey>>,
}

impl ContentKeyRemap {
    /// Rewrite inline topology to this projection's key domain.
    ///
    /// # Errors
    ///
    /// Returns an error when a key is outside this projection.
    pub fn remap_inlines(&self, nodes: &mut [Inline]) -> Result<(), ContentProjectionError> {
        for node in nodes {
            match node {
                Inline::Text { content } | Inline::Code { content } => {
                    content.atom = self.atom(content.atom)?;
                }
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    self.remap_inlines(children)?;
                }
                Inline::Link {
                    occurrence,
                    children,
                } => {
                    *occurrence = self.link(*occurrence)?;
                    self.remap_inlines(children)?;
                }
                Inline::Anchor { point, .. } => *point = self.point(*point)?,
                Inline::LineBreak { atom } => *atom = self.atom(*atom)?,
            }
        }
        Ok(())
    }

    /// Rewrite a heading to this projection's key domain.
    ///
    /// # Errors
    ///
    /// Returns an error when a key is outside this projection.
    pub fn remap_heading(&self, heading: &mut Heading) -> Result<(), ContentProjectionError> {
        self.remap_inlines(&mut heading.content)
    }

    /// Rewrite nested blocks to this projection's key domain.
    ///
    /// # Errors
    ///
    /// Returns an error when a key is outside this projection.
    pub fn remap_blocks(&self, blocks: &mut [Block]) -> Result<(), ContentProjectionError> {
        for block in blocks {
            match block {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    self.remap_inlines(children)?;
                }
                Block::FixedDisplay { children, view, .. } => {
                    *view = self.fixed_view(*view)?;
                    self.remap_inlines(children)?;
                }
                Block::List { items, .. } => {
                    for item in items {
                        self.remap_blocks(&mut item.blocks)?;
                    }
                }
                Block::DefinitionList { items, .. } => {
                    for item in items {
                        for term in &mut item.terms {
                            self.remap_inlines(term)?;
                        }
                        self.remap_blocks(&mut item.description)?;
                    }
                }
                Block::Table {
                    rows, fixed_view, ..
                } => {
                    if let Some(view) = fixed_view {
                        *view = self.fixed_view(*view)?;
                    }
                    for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                        if let Some(point) = &mut cell.point {
                            *point = self.point(*point)?;
                        }
                        self.remap_blocks(&mut cell.blocks)?;
                    }
                }
                Block::Equation { .. }
                | Block::VerticalSpace { .. }
                | Block::ThematicBreak { .. }
                | Block::Unsupported { .. } => {}
            }
        }
        Ok(())
    }

    /// Rewrite a complete section subtree to this projection's key domain.
    ///
    /// # Errors
    ///
    /// Returns an error when a key is outside this projection.
    pub fn remap_section(&self, section: &mut Section) -> Result<(), ContentProjectionError> {
        self.remap_heading(&mut section.heading)?;
        self.remap_blocks(&mut section.blocks)?;
        for child in &mut section.children {
            self.remap_section(child)?;
        }
        Ok(())
    }

    /// Rewrite one occurrence key.
    ///
    /// # Errors
    ///
    /// Returns an error when the key is outside this projection.
    pub fn remap_link(&self, key: &mut LinkOccurrenceKey) -> Result<(), ContentProjectionError> {
        *key = self.link(*key)?;
        Ok(())
    }

    fn owner(&self, key: ContentOwnerKey) -> Result<ContentOwnerKey, ContentProjectionError> {
        lookup(
            &self.owners,
            key.get(),
            "content owner is outside the projection",
        )
    }
    /// Map one retained source root into this projection's key domain.
    ///
    /// # Errors
    ///
    /// Returns an error when the root is outside this projection.
    pub fn root(&self, key: ContentRootKey) -> Result<ContentRootKey, ContentProjectionError> {
        lookup(
            &self.roots,
            key.get(),
            "content root is outside the projection",
        )
    }
    fn atom(&self, key: ContentAtomKey) -> Result<ContentAtomKey, ContentProjectionError> {
        lookup(
            &self.atoms,
            key.get(),
            "content atom is outside the projection",
        )
    }
    fn point(&self, key: ContentPointKey) -> Result<ContentPointKey, ContentProjectionError> {
        lookup(
            &self.points,
            key.get(),
            "content point is outside the projection",
        )
    }
    fn link(&self, key: LinkOccurrenceKey) -> Result<LinkOccurrenceKey, ContentProjectionError> {
        lookup(
            &self.links,
            key.get(),
            "link occurrence is outside the projection",
        )
    }

    fn fixed_view(&self, key: FixedViewKey) -> Result<FixedViewKey, ContentProjectionError> {
        lookup(
            &self.fixed_views,
            key.get(),
            "fixed view is outside this projection",
        )
    }
}

fn selected(bits: &[bool], key: u32) -> bool {
    usize::try_from(key - 1)
        .ok()
        .and_then(|index| bits.get(index))
        .copied()
        .unwrap_or(false)
}

fn mark(bits: &mut [bool], key: u32, error: &'static str) -> Result<bool, ContentProjectionError> {
    let index = usize::try_from(key - 1).map_err(|_| ContentProjectionError(error))?;
    let selected = bits.get_mut(index).ok_or(ContentProjectionError(error))?;
    let changed = !*selected;
    *selected = true;
    Ok(changed)
}

fn count(bits: &[bool]) -> usize {
    bits.iter().filter(|selected| **selected).count()
}

fn dense_map<K: Copy>(
    selected: &[bool],
    constructor: impl Fn(u32) -> Option<K>,
) -> Result<Vec<Option<K>>, ContentProjectionError> {
    let mut next = 1_u32;
    selected
        .iter()
        .map(|selected| {
            if !selected {
                return Ok(None);
            }
            let key = constructor(next)
                .ok_or(ContentProjectionError("projection key capacity exceeded"))?;
            next = next
                .checked_add(1)
                .ok_or(ContentProjectionError("projection key capacity exceeded"))?;
            Ok(Some(key))
        })
        .collect()
}

fn lookup<K: Copy>(
    map: &[Option<K>],
    key: u32,
    error: &'static str,
) -> Result<K, ContentProjectionError> {
    usize::try_from(key - 1)
        .ok()
        .and_then(|index| map.get(index))
        .and_then(|key| *key)
        .ok_or(ContentProjectionError(error))
}

fn link_target_bytes(target: &crate::LinkTarget) -> usize {
    match target {
        crate::LinkTarget::External { uri } => uri.len(),
        crate::LinkTarget::Email { address } => address.len(),
        crate::LinkTarget::Document { name, fragment } => name
            .len()
            .saturating_add(fragment.as_deref().map_or(0, str::len)),
        crate::LinkTarget::Manual {
            name,
            manual_section,
        } => name
            .len()
            .saturating_add(manual_section.as_deref().map_or(0, str::len)),
        crate::LinkTarget::Section { id } => id.as_str().len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentOwnerKind, ContentRootKind, ContentStoreBuilder, ContentStyle, Provenance};

    #[test]
    fn table_cell_point_closes_and_remaps_without_visible_content() {
        let mut builder = ContentStoreBuilder::new();
        let unrelated_owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let unrelated_root =
            builder.push_root(unrelated_owner, ContentRootKind::Body, Provenance::Unknown);
        let _ = builder.push_point(
            unrelated_root,
            crate::PointBoundary::BetweenAtoms { atom_boundary: 0 },
            0,
            Provenance::Unknown,
        );
        let cell_owner = builder.push_owner(ContentOwnerKind::TableCell, Provenance::Unknown);
        let cell_root = builder.push_root(cell_owner, ContentRootKind::Cell, Provenance::Unknown);
        let cell_point = builder.push_point(
            cell_root,
            crate::PointBoundary::BetweenAtoms { atom_boundary: 0 },
            0,
            Provenance::Unknown,
        );
        let source = builder.finish();
        let mut blocks = vec![Block::Table {
            rows: vec![crate::TableRow {
                kind: crate::TableRowKind::Data,
                cells: vec![crate::TableCell {
                    kind: crate::TableCellKind::Text,
                    blocks: Vec::new(),
                    point: Some(cell_point),
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                    source: None,
                }],
            }],
            fixed_view: None,
            layout: crate::LayoutHint::default(),
            source: None,
        }];
        let mut projection = ContentProjectionBuilder::new(&source);
        projection.include_blocks(&blocks).unwrap();
        let (snapshot, remap) = projection.finish().unwrap();
        remap.remap_blocks(&mut blocks).unwrap();
        crate::validate_content_store(&snapshot.content_store).unwrap();
        assert_eq!(snapshot.content_store.owners.len(), 1);
        assert_eq!(snapshot.content_store.roots.len(), 1);
        assert_eq!(snapshot.content_store.points.len(), 1);
        let Block::Table { rows, .. } = &blocks[0] else {
            panic!("table")
        };
        assert_eq!(rows[0].cells[0].point, Some(crate::ContentPointKey::FIRST));
    }

    #[test]
    fn wide_nested_link_intervals_validate_and_snapshot_with_linear_work() {
        const LINKS: usize = 8_000;
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let links = (0..LINKS)
            .map(|_| {
                builder.push_link(
                    owner,
                    crate::LinkTarget::External {
                        uri: "https://example.test".into(),
                    },
                    None,
                    Provenance::Unknown,
                )
            })
            .collect::<Vec<_>>();
        for link in links.iter().chain(links.iter().rev()) {
            let _ = builder.push_text(
                root,
                "x".into(),
                None,
                ContentStyle::default(),
                None,
                Some(*link),
                Provenance::Unknown,
            );
        }
        let source = builder.finish();
        crate::validate_content_store(&source).unwrap();
        let mut projection = ContentProjectionBuilder::new(&source);
        projection.include_root(root).unwrap();
        let (snapshot, _) = projection.snapshot().unwrap();
        crate::validate_content_store(&snapshot.content_store).unwrap();
        assert_eq!(snapshot.content_store.links.len(), LINKS);
        assert!(projection.snapshot_work < MAX_PROJECTION_STEPS);
    }

    #[test]
    fn selecting_one_root_does_not_copy_unrelated_siblings_of_its_owner() {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let selected = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let other = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let first = builder.push_text(
            selected,
            "selected".to_owned(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let _ = builder.push_text(
            other,
            "unrelated".to_owned(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let source = builder.finish();
        let mut projection = ContentProjectionBuilder::new(&source);
        projection
            .include_inlines(&[Inline::Text { content: first }])
            .unwrap();
        let (projected, _) = projection.finish().unwrap();
        assert_eq!(projected.content_store.owners.len(), 1);
        assert_eq!(projected.content_store.roots.len(), 1);
        assert_eq!(projected.content_store.atoms.len(), 1);
        assert_eq!(projected.content_store.owners[0].roots.len(), 1);
        assert_eq!(
            projected.content_store.atoms[0].kind.text(),
            Some("selected")
        );
    }

    #[test]
    fn snapshot_reuse_requires_a_successful_unchanged_closure() {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let first_root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let second_root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let first = builder.push_text(
            first_root,
            "first".to_owned(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let second = builder.push_text(
            second_root,
            "second".to_owned(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let source = builder.finish();
        let mut projection = ContentProjectionBuilder::new(&source);
        assert!(!projection.can_reuse_snapshot());
        projection
            .include_inlines(&[Inline::Text { content: first }])
            .unwrap();
        assert!(!projection.can_reuse_snapshot());
        projection.snapshot().unwrap();
        assert!(projection.can_reuse_snapshot());
        projection
            .include_inlines(&[Inline::Text { content: first }])
            .unwrap();
        assert!(projection.can_reuse_snapshot());
        let admitted_work = projection.snapshot_work;
        let checkpoint = projection.checkpoint().unwrap();
        projection
            .include_inlines(&[Inline::Text { content: second }])
            .unwrap();
        assert!(!projection.can_reuse_snapshot());
        assert_eq!(
            projection.snapshot().unwrap().0.content_store.roots.len(),
            2
        );
        assert!(projection.snapshot_work > admitted_work);
        projection.rollback(checkpoint);
        assert!(projection.snapshot_work > admitted_work);
        assert!(projection.can_reuse_snapshot());
        let (projected, _) = projection.snapshot().unwrap();
        assert_eq!(projected.content_store.roots.len(), 1);

        let checkpoint = projection.checkpoint().unwrap();
        projection
            .include_inlines(&[Inline::Text { content: second }])
            .unwrap();
        let (projected, _) = projection.snapshot().unwrap();
        assert_eq!(projected.content_store.roots.len(), 2);
        projection.commit(checkpoint);
        assert!(projection.can_reuse_snapshot());
        assert!(projection.snapshot_work > admitted_work);
    }

    #[test]
    fn rolled_back_snapshots_do_not_restore_the_work_budget() {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let mut selected = None;
        for _ in 0..20_000 {
            let content = builder.push_text(
                root,
                "x".to_owned(),
                None,
                ContentStyle::default(),
                None,
                None,
                Provenance::Unknown,
            );
            selected.get_or_insert(content);
        }
        let source = builder.finish();
        let mut projection = ContentProjectionBuilder::new(&source);
        let inline = Inline::Text {
            content: selected.unwrap(),
        };
        let mut completed = 0;
        for _ in 0..200 {
            let checkpoint = projection.checkpoint().unwrap();
            let result = projection
                .include_inlines(std::slice::from_ref(&inline))
                .and_then(|()| projection.snapshot().map(|_| ()));
            projection.rollback(checkpoint);
            if result.is_err() {
                break;
            }
            completed += 1;
        }
        assert!(completed > 0);
        assert!(
            completed < 200,
            "rolled-back work must reach the step limit"
        );
        assert!(projection.snapshot_work > 0);
        assert!(projection.roots.iter().all(|selected| !selected));
        assert!(projection.atoms.iter().all(|selected| !selected));
    }

    #[test]
    fn rolled_back_large_atom_snapshots_charge_byte_work() {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let content = builder.push_text(
            root,
            "x".repeat(8 * 1024 * 1024),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let source = builder.finish();
        let mut projection = ContentProjectionBuilder::new(&source);
        let inline = Inline::Text { content };
        let mut completed = 0;
        for _ in 0..200 {
            let checkpoint = projection.checkpoint().unwrap();
            let result = projection
                .include_inlines(std::slice::from_ref(&inline))
                .and_then(|()| projection.snapshot().map(|_| ()));
            projection.rollback(checkpoint);
            if result.is_err() {
                break;
            }
            completed += 1;
        }
        assert!(completed > 0 && completed < 200);
        assert!(projection.snapshot_work >= MAX_PROJECTION_STEPS / 2);
    }

    #[test]
    fn failed_remap_and_failed_closure_keep_their_work_charged() {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let content = builder.push_text(
            root,
            "x".to_owned(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let mut source = builder.finish();
        source.atoms[0].owner = ContentOwnerKey::new(999).unwrap();
        let mut projection = ContentProjectionBuilder::new(&source);
        let checkpoint = projection.checkpoint().unwrap();
        projection
            .include_inlines(&[Inline::Text { content }])
            .unwrap();
        assert!(projection.snapshot().is_err());
        let attempted_work = projection.snapshot_work;
        assert!(attempted_work > 0);
        projection.rollback(checkpoint);
        assert_eq!(projection.snapshot_work, attempted_work);
        assert!(projection.atoms.iter().all(|selected| !selected));

        projection.steps = MAX_PROJECTION_STEPS - projection.snapshot_work;
        let checkpoint = projection.checkpoint().unwrap();
        assert!(
            projection
                .include_inlines(&[Inline::Text { content }])
                .is_err()
        );
        let attempted_steps = projection.steps;
        projection.rollback(checkpoint);
        assert_eq!(projection.steps, attempted_steps);
        assert!(
            projection
                .include_inlines(&[Inline::Text { content }])
                .is_err()
        );
    }

    #[test]
    fn malformed_owner_root_edges_are_charged_before_scanning() {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let content = builder.push_text(
            root,
            "x".into(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let mut source = builder.finish();
        source.owners[0]
            .roots
            .extend(std::iter::repeat_n(root, 200_000));
        let mut projection = ContentProjectionBuilder::new(&source);
        let inline = Inline::Text { content };
        let checkpoint = projection.checkpoint().unwrap();
        projection
            .include_inlines(std::slice::from_ref(&inline))
            .unwrap();
        assert!(projection.snapshot().is_err());
        let charged = projection.snapshot_work;
        assert!(charged >= 200_000);
        projection.rollback(checkpoint);
        let checkpoint = projection.checkpoint().unwrap();
        projection
            .include_inlines(std::slice::from_ref(&inline))
            .unwrap();
        assert!(projection.snapshot().is_err());
        assert!(projection.snapshot_work > charged);
        projection.rollback(checkpoint);
    }
}
