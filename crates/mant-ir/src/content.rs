//! Contextual reads from one authoritative inline-content store.
//!
//! [`Inline`] leaves carry only checked keys and ranges. Keeping reads behind
//! this store-bound facade lets a document and a response-local projection
//! share one access path without copying text or exposing a second authority.

use std::{error::Error, fmt};

use crate::{
    ContentLocationRef, ContentPointKey, ContentProjection, ContentStore, Document,
    EntryContentSlice, EntryOwner, FragmentAlias, Heading, Inline, LinkOccurrenceKey, LinkTarget,
    NodeId, RootTextRange, SourceSpan,
};

/// One store-resolved inline node.
///
/// Consumers match this view instead of reading storage fields from [`Inline`]
/// directly. The view remains stable while document and response-local key
/// domains resolve through their respective authoritative stores.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum InlineView<'store> {
    /// Plain logical text.
    Text(&'store str),
    /// Literal logical text.
    Code(&'store str),
    /// Strong wrapper children.
    Strong(&'store [Inline]),
    /// Emphasis wrapper children.
    Emphasis(&'store [Inline]),
    /// A structural fragment of one resolved link occurrence.
    Link(LinkView<'store>),
    /// A zero-width local destination.
    Anchor(AnchorView<'store>),
    /// One logical hard line break.
    LineBreak,
}

/// Store-resolved metadata for one structural link fragment.
#[derive(Debug, Clone, Copy)]
pub struct LinkView<'store> {
    occurrence: LinkOccurrenceKey,
    target: &'store LinkTarget,
    title: Option<&'store str>,
    children: &'store [Inline],
}

impl<'store> LinkView<'store> {
    /// Document- or projection-local logical occurrence identity.
    #[must_use]
    pub const fn occurrence(self) -> LinkOccurrenceKey {
        self.occurrence
    }

    /// Typed destination shared by all fragments of the occurrence.
    #[must_use]
    pub const fn target(self) -> &'store LinkTarget {
        self.target
    }

    /// Optional non-visible advisory title.
    #[must_use]
    pub const fn title(self) -> Option<&'store str> {
        self.title
    }

    /// Visible children of this structural fragment.
    #[must_use]
    pub const fn children(self) -> &'store [Inline] {
        self.children
    }
}

/// Store-resolved metadata for one zero-width local destination.
#[derive(Debug, Clone, Copy)]
pub struct AnchorView<'store> {
    point: ContentPointKey,
    id: &'store NodeId,
    fragment_aliases: &'store [FragmentAlias],
    owner_source: Option<SourceSpan>,
}

impl<'store> AnchorView<'store> {
    /// Exact zero-width point in the authoritative store.
    #[must_use]
    pub const fn point(self) -> ContentPointKey {
        self.point
    }

    /// Normalized document-local destination identity.
    #[must_use]
    pub const fn id(self) -> &'store NodeId {
        self.id
    }

    /// Exact authored fragment aliases.
    #[must_use]
    pub const fn fragment_aliases(self) -> &'store [FragmentAlias] {
        self.fragment_aliases
    }

    /// Source owner retained by the legacy inline representation.
    #[must_use]
    pub const fn owner_source(self) -> Option<SourceSpan> {
        self.owner_source
    }
}

/// A content key or byte range does not resolve in this context's store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentReadError;

impl fmt::Display for ContentReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("content does not resolve in this store")
    }
}

impl Error for ContentReadError {}

pub(crate) trait ContentBackend: Send + Sync {
    fn resolve(&self, location: ContentLocationRef<'_>) -> Option<&[Inline]>;

    fn inline<'content>(&'content self, inline: &'content Inline) -> Option<InlineView<'content>> {
        let store = self.store()?;
        Some(match inline {
            Inline::Text { content } => InlineView::Text(store.text(*content)?),
            Inline::Code { content } => InlineView::Code(store.text(*content)?),
            Inline::Strong { children } => InlineView::Strong(children),
            Inline::Emphasis { children } => InlineView::Emphasis(children),
            Inline::Link {
                occurrence,
                children,
            } => {
                let link = store.link(*occurrence)?;
                InlineView::Link(LinkView {
                    occurrence: *occurrence,
                    target: &link.target,
                    title: link.title.as_deref(),
                    children,
                })
            }
            Inline::Anchor {
                point,
                id,
                fragment_aliases,
            } => {
                let point_record = store.point(*point)?;
                InlineView::Anchor(AnchorView {
                    point: *point,
                    id,
                    fragment_aliases,
                    owner_source: match point_record.provenance {
                        crate::Provenance::Authored { span } => Some(span),
                        crate::Provenance::Generated { .. } | crate::Provenance::Unknown => None,
                    },
                })
            }
            Inline::LineBreak { atom } => matches!(
                store.atom(*atom)?.kind,
                crate::ContentAtomKind::HardBreak {}
            )
            .then_some(InlineView::LineBreak)?,
        })
    }

    fn store(&self) -> Option<&ContentStore>;
}

impl ContentBackend for Document {
    fn resolve(&self, location: ContentLocationRef<'_>) -> Option<&[Inline]> {
        crate::content_location::resolve_location(self, location)
    }

    fn store(&self) -> Option<&ContentStore> {
        Some(&self.content_store)
    }
}

impl ContentBackend for ContentStore {
    fn resolve(&self, _location: ContentLocationRef<'_>) -> Option<&[Inline]> {
        None
    }

    fn store(&self) -> Option<&ContentStore> {
        Some(self)
    }
}

impl ContentBackend for ContentProjection {
    fn resolve(&self, _location: ContentLocationRef<'_>) -> Option<&[Inline]> {
        None
    }

    fn store(&self) -> Option<&ContentStore> {
        Some(&self.content_store)
    }
}

/// Read-only access to one authoritative inline-content store.
///
/// A context never owns or copies an alternate text store. Obtain the document
/// context through [`Document::content`] or a projection context through
/// [`ContentProjection::content`]. The backend is private so neither storage
/// form creates a parallel public resolver contract.
#[derive(Clone, Copy)]
pub struct ContentContext<'store> {
    backend: &'store dyn ContentBackend,
}

/// Operation-local scalar starts for validating inline topology in linear work.
///
/// A logical root can be split across several structural inline containers.
/// The index therefore records absolute root-relative starts instead of
/// assuming every heading, paragraph, or term begins at scalar zero.
pub struct InlinePositionIndex<'store> {
    content: ContentContext<'store>,
    atom_starts: Vec<u32>,
    atom_lengths: Vec<u32>,
    cursors: std::collections::HashMap<crate::ContentRootKey, u32>,
    document_cursors: std::collections::HashMap<crate::ContentRootKey, u32>,
}

impl InlinePositionIndex<'_> {
    /// Check one inline container against the same original logical root.
    #[must_use]
    pub fn is_positioned(&mut self, nodes: &[Inline]) -> bool {
        self.cursors.clear();
        Self::walk(
            self.content,
            &self.atom_starts,
            &self.atom_lengths,
            nodes,
            &mut self.cursors,
            false,
        )
    }

    /// Check the next inline container in complete document traversal order.
    /// Unlike a response-local excerpt, a full document must not reorder
    /// separate containers that consume the same logical root.
    #[must_use]
    pub fn is_positioned_in_document(&mut self, nodes: &[Inline]) -> bool {
        Self::walk(
            self.content,
            &self.atom_starts,
            &self.atom_lengths,
            nodes,
            &mut self.document_cursors,
            true,
        )
    }

    #[allow(clippy::too_many_lines)] // Keep one scalar-order state machine for both admission modes.
    fn walk(
        content: ContentContext<'_>,
        atom_starts: &[u32],
        atom_lengths: &[u32],
        nodes: &[Inline],
        cursors: &mut std::collections::HashMap<crate::ContentRootKey, u32>,
        complete_document: bool,
    ) -> bool {
        fn advance(
            cursors: &mut std::collections::HashMap<crate::ContentRootKey, u32>,
            root: crate::ContentRootKey,
            start: u32,
            length: u32,
            complete_document: bool,
        ) -> bool {
            let current = cursors
                .entry(root)
                .or_insert(if complete_document { 0 } else { start });
            if *current != start {
                return false;
            }
            let Some(next) = start.checked_add(length) else {
                return false;
            };
            *current = next;
            true
        }
        fn walk(
            content: ContentContext<'_>,
            atom_starts: &[u32],
            atom_lengths: &[u32],
            nodes: &[Inline],
            cursors: &mut std::collections::HashMap<crate::ContentRootKey, u32>,
            complete_document: bool,
        ) -> bool {
            for node in nodes {
                match node {
                    Inline::Strong { children }
                    | Inline::Emphasis { children }
                    | Inline::Link { children, .. } => {
                        if !walk(
                            content,
                            atom_starts,
                            atom_lengths,
                            children,
                            cursors,
                            complete_document,
                        ) {
                            return false;
                        }
                    }
                    Inline::Text { content: range } | Inline::Code { content: range } => {
                        let (Some(atom), Some(text), Some(&start)) = (
                            content.atom(range.atom),
                            content.resolve_text(*range),
                            range.atom.index().and_then(|key| atom_starts.get(key)),
                        ) else {
                            return false;
                        };
                        let length = if range.bytes.start == 0
                            && usize::try_from(range.bytes.end).ok()
                                == atom.kind.text().map(str::len)
                        {
                            range
                                .atom
                                .index()
                                .and_then(|key| atom_lengths.get(key))
                                .copied()
                        } else {
                            u32::try_from(text.chars().count()).ok()
                        };
                        let Some(length) = length else {
                            return false;
                        };
                        if !advance(cursors, atom.root, start, length, complete_document) {
                            return false;
                        }
                    }
                    Inline::LineBreak { atom } => {
                        let (Some(record), Some(&start)) = (
                            content.atom(*atom),
                            atom.index().and_then(|key| atom_starts.get(key)),
                        ) else {
                            return false;
                        };
                        if !advance(cursors, record.root, start, 1, complete_document) {
                            return false;
                        }
                    }
                    Inline::Anchor { point, .. } => {
                        let Some(record) = content.point(*point) else {
                            return false;
                        };
                        // Pinned mdoc_validate.c::post_tg can attach a .Tg
                        // target to the following Bd body.  The native point
                        // remains in its own empty root while the visible
                        // body text has another root; joining them in one
                        // inline container does not move either authority.
                        if content.root(record.root).is_some_and(|root| {
                            root.atoms.is_empty() && record.scalar_boundary == 0
                        }) {
                            continue;
                        }
                        if !advance(
                            cursors,
                            record.root,
                            record.scalar_boundary,
                            0,
                            complete_document,
                        ) {
                            return false;
                        }
                    }
                }
            }
            true
        }
        // Pinned man_term.c::pre_SY/post_SY and mdoc_term.c::termp_bk_pre
        // can place separately emitted logical roots in one rendered inline
        // container. Validate each root independently, while complete-document
        // traversal also preserves order across containers of the same root.
        walk(
            content,
            atom_starts,
            atom_lengths,
            nodes,
            cursors,
            complete_document,
        )
    }
}

impl fmt::Debug for ContentContext<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContentContext")
            .finish_non_exhaustive()
    }
}

impl<'store> ContentContext<'store> {
    /// Resolve one checked native fixed display view without copying content.
    #[must_use]
    pub fn fixed_view(self, key: crate::FixedViewKey) -> Option<&'store crate::FixedView> {
        self.backend.store()?.fixed_view(key)
    }
    pub(crate) fn new(backend: &'store dyn ContentBackend) -> Self {
        Self { backend }
    }

    /// Resolve one checked location to its inline container or singleton node.
    #[must_use]
    pub fn resolve(self, location: ContentLocationRef<'_>) -> Option<&'store [Inline]> {
        self.backend.resolve(location)
    }

    /// Resolve one inline node through this store.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when the node contains an unresolved key or
    /// byte range in this store.
    pub fn inline(self, inline: &'store Inline) -> Result<InlineView<'store>, ContentReadError> {
        self.backend.inline(inline).ok_or(ContentReadError)
    }

    /// Resolve one checked atom byte range to logical UTF-8 text.
    #[must_use]
    pub fn resolve_text(self, content: crate::ContentRef) -> Option<&'store str> {
        self.backend.store()?.text(content)
    }

    /// Resolve one logical occurrence record.
    #[must_use]
    pub fn occurrence(self, key: LinkOccurrenceKey) -> Option<&'store crate::LinkOccurrence> {
        self.backend.store()?.link(key)
    }

    /// Number of document- or projection-local link occurrences.
    #[must_use]
    pub fn occurrence_count(self) -> usize {
        self.backend.store().map_or(0, |store| store.links.len())
    }

    /// Resolve one authoritative atom record.
    #[must_use]
    pub fn atom(self, key: crate::ContentAtomKey) -> Option<&'store crate::ContentAtom> {
        self.backend.store()?.atom(key)
    }

    /// Resolve one authoritative zero-width point record.
    #[must_use]
    pub fn point(self, key: ContentPointKey) -> Option<&'store crate::ContentPoint> {
        self.backend.store()?.point(key)
    }

    /// Resolve one authoritative structural content owner.
    #[must_use]
    pub fn owner(self, key: crate::ContentOwnerKey) -> Option<&'store crate::ContentOwner> {
        self.backend.store()?.owner(key)
    }

    /// Build one reusable scalar-position index for document or projection
    /// admission. The store must already have passed structural validation.
    #[must_use]
    pub fn inline_position_index(self) -> Option<InlinePositionIndex<'store>> {
        let store = self.backend.store()?;
        let mut atom_starts = vec![0_u32; store.atoms.len()];
        let mut atom_lengths = vec![0_u32; store.atoms.len()];
        for root in &store.roots {
            let mut scalar = 0_u32;
            for &key in &root.atoms {
                let record = store.atom(key)?;
                if record.root != root.key {
                    return None;
                }
                let index = key.index()?;
                atom_starts[index] = scalar;
                let length = match &record.kind {
                    crate::ContentAtomKind::Text { text, .. }
                    | crate::ContentAtomKind::Whitespace { text, .. } => {
                        u32::try_from(text.chars().count()).ok()?
                    }
                    crate::ContentAtomKind::HardBreak {} => 1,
                    crate::ContentAtomKind::BreakOpportunity {} => 0,
                };
                atom_lengths[index] = length;
                scalar = scalar.checked_add(length)?;
            }
        }
        Some(InlinePositionIndex {
            content: self,
            atom_starts,
            atom_lengths,
            cursors: std::collections::HashMap::new(),
            document_cursors: std::collections::HashMap::new(),
        })
    }

    /// Resolve one authoritative logical root record.
    #[must_use]
    pub fn root(self, key: crate::ContentRootKey) -> Option<&'store crate::ContentRoot> {
        self.backend.store()?.root(key)
    }

    /// Materialize one root's canonical searchable logical sequence.
    #[must_use]
    pub fn root_logical_text(self, key: crate::ContentRootKey) -> Option<String> {
        self.backend.store()?.root_logical_text(key)
    }

    /// Materialize the complete logical label of one occurrence.
    ///
    /// Structural link wrappers may span roots; this reads the occurrence
    /// table rather than concatenating one display fragment.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when the occurrence or a retained label
    /// component cannot be resolved in this context.
    pub fn occurrence_plain_text(self, key: LinkOccurrenceKey) -> Result<String, ContentReadError> {
        let occurrence = self.occurrence(key).ok_or(ContentReadError)?;
        let store = self.backend.store().ok_or(ContentReadError)?;
        let mut text = String::new();
        for part in &occurrence.label {
            match part {
                crate::LinkLabelPart::Content { content } => {
                    text.push_str(store.text(*content).ok_or(ContentReadError)?);
                }
                crate::LinkLabelPart::HardBreak { atom } => {
                    let atom = store.atom(*atom).ok_or(ContentReadError)?;
                    if !matches!(atom.kind, crate::ContentAtomKind::HardBreak {}) {
                        return Err(ContentReadError);
                    }
                    text.push('\n');
                }
            }
        }
        Ok(text)
    }

    /// Resolve link metadata for one structural inline fragment.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when the node does not resolve in this
    /// store.
    pub fn link(
        self,
        inline: &'store Inline,
    ) -> Result<Option<LinkView<'store>>, ContentReadError> {
        Ok(match self.inline(inline)? {
            InlineView::Link(link) => Some(link),
            _ => None,
        })
    }

    /// Resolve one checked location that identifies precisely one real link.
    #[must_use]
    pub fn resolve_link(self, location: ContentLocationRef<'_>) -> Option<&'store Inline> {
        let path = match location {
            ContentLocationRef::DocumentHeading { path }
            | ContentLocationRef::SectionHeading { path, .. }
            | ContentLocationRef::Content { path, .. } => path,
        };
        if path.is_empty() {
            return None;
        }
        let [link] = self.resolve(location)? else {
            return None;
        };
        self.link(link).ok()?.map(|_| link)
    }

    /// Resolve store-backed link metadata at one checked structural location.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when the selected link does not resolve in
    /// this store.
    pub fn resolve_link_view(
        self,
        location: ContentLocationRef<'_>,
    ) -> Result<Option<LinkView<'store>>, ContentReadError> {
        let path = match location {
            ContentLocationRef::DocumentHeading { path }
            | ContentLocationRef::SectionHeading { path, .. }
            | ContentLocationRef::Content { path, .. } => path,
        };
        if path.is_empty() {
            return Ok(None);
        }
        let Some(nodes) = self.resolve(location) else {
            return Ok(None);
        };
        let [link] = nodes else {
            return Ok(None);
        };
        self.link(link)
    }

    /// Flatten an inline root into its logical visible text.
    ///
    /// Styles and links contribute their children, anchors contribute no text,
    /// and hard breaks contribute `\n`.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when any retained leaf does not resolve in
    /// this store.
    pub fn plain_text(self, nodes: &'store [Inline]) -> Result<String, ContentReadError> {
        let mut output = String::new();
        self.visit_plain_text(nodes, |text| output.push_str(text))?;
        Ok(output)
    }

    /// Derive the visible text of one heading through this content store.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when retained heading content does not
    /// resolve in this store.
    pub fn heading_plain_text(self, heading: &'store Heading) -> Result<String, ContentReadError> {
        self.plain_text(&heading.content)
    }

    /// Derive a whitespace-normalized, single-line heading label.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when retained heading content does not
    /// resolve in this store.
    pub fn heading_single_line_text(
        self,
        heading: &'store Heading,
    ) -> Result<String, ContentReadError> {
        Ok(self
            .heading_plain_text(heading)?
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "))
    }

    /// Visit borrowed logical text leaves in source order without allocation.
    ///
    /// Emitted text remains borrowed from this context's store, not from an
    /// operation-local projection or callback scratch buffer.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when any retained leaf does not resolve in
    /// this store.
    pub fn visit_plain_text(
        self,
        nodes: &'store [Inline],
        mut emit: impl FnMut(&'store str),
    ) -> Result<(), ContentReadError> {
        self.append_plain_text(nodes, &mut emit)
    }

    fn append_plain_text(
        self,
        nodes: &'store [Inline],
        emit: &mut impl FnMut(&'store str),
    ) -> Result<(), ContentReadError> {
        for node in nodes {
            match self.inline(node)? {
                InlineView::Text(text) | InlineView::Code(text) => emit(text),
                InlineView::Strong(children) | InlineView::Emphasis(children) => {
                    self.append_plain_text(children, emit)?;
                }
                InlineView::Link(link) => self.append_plain_text(link.children(), emit)?,
                InlineView::Anchor(_) => {}
                InlineView::LineBreak => emit("\n"),
            }
        }
        Ok(())
    }

    /// Return the first logical character visible to a renderer.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when any inspected node does not resolve in
    /// this store.
    pub fn first_visible_character(
        self,
        nodes: &'store [Inline],
    ) -> Result<Option<char>, ContentReadError> {
        for node in nodes {
            if let Some(character) = self.first_node_character(node)? {
                return Ok(Some(character));
            }
        }
        Ok(None)
    }

    fn first_node_character(self, node: &'store Inline) -> Result<Option<char>, ContentReadError> {
        Ok(match self.inline(node)? {
            InlineView::Text(text) | InlineView::Code(text) => text.chars().next(),
            InlineView::Strong(children) | InlineView::Emphasis(children) => {
                self.first_visible_character(children)?
            }
            InlineView::Link(link) => self.first_visible_character(link.children())?,
            InlineView::Anchor(_) => None,
            InlineView::LineBreak => Some('\n'),
        })
    }

    /// Return the last logical character visible to a renderer.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when any inspected node does not resolve in
    /// this store.
    pub fn last_visible_character(
        self,
        nodes: &'store [Inline],
    ) -> Result<Option<char>, ContentReadError> {
        for node in nodes.iter().rev() {
            if let Some(character) = self.last_node_character(node)? {
                return Ok(Some(character));
            }
        }
        Ok(None)
    }

    fn last_node_character(self, node: &'store Inline) -> Result<Option<char>, ContentReadError> {
        Ok(match self.inline(node)? {
            InlineView::Text(text) | InlineView::Code(text) => text.chars().next_back(),
            InlineView::Strong(children) | InlineView::Emphasis(children) => {
                self.last_visible_character(children)?
            }
            InlineView::Link(link) => self.last_visible_character(link.children())?,
            InlineView::Anchor(_) => None,
            InlineView::LineBreak => Some('\n'),
        })
    }

    /// Whether a root contains content other than layout-only line breaks.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when any inspected node does not resolve in
    /// this store.
    pub fn has_printable_character(
        self,
        nodes: &'store [Inline],
    ) -> Result<bool, ContentReadError> {
        for node in nodes {
            let printable = match self.inline(node)? {
                InlineView::Text(text) | InlineView::Code(text) => {
                    text.chars().any(|character| character != '\n')
                }
                InlineView::Strong(children) | InlineView::Emphasis(children) => {
                    self.has_printable_character(children)?
                }
                InlineView::Link(link) => self.has_printable_character(link.children())?,
                InlineView::Anchor(_) | InlineView::LineBreak => false,
            };
            if printable {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether a literal inline stream contains an authored row.
    ///
    /// Empty wrappers and zero-width anchors alone do not create a row.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when any inspected node does not resolve in
    /// this store.
    pub fn has_literal_rows(self, nodes: &'store [Inline]) -> Result<bool, ContentReadError> {
        for node in nodes {
            let present = match self.inline(node)? {
                InlineView::Text(_) | InlineView::Code(_) | InlineView::LineBreak => true,
                InlineView::Strong(children) | InlineView::Emphasis(children) => {
                    self.has_literal_rows(children)?
                }
                InlineView::Link(link) => self.has_literal_rows(link.children())?,
                InlineView::Anchor(_) => false,
            };
            if present {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Measure the final open row of a definition's original labels.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when any inspected term does not resolve in
    /// this store.
    pub fn definition_run_in_width(
        self,
        terms: &'store [Vec<Inline>],
    ) -> Result<Option<usize>, ContentReadError> {
        let mut final_row = String::new();
        let mut present = false;
        for term in terms {
            let mut row = String::new();
            let mut term_present = false;
            self.append_final_row(term, &mut row, &mut term_present)?;
            if term_present {
                final_row = row;
                present = true;
            }
        }
        Ok((present && !final_row.is_empty()).then(|| crate::geometry::text_width(&final_row)))
    }

    fn append_final_row(
        self,
        nodes: &'store [Inline],
        row: &mut String,
        present: &mut bool,
    ) -> Result<(), ContentReadError> {
        for node in nodes {
            match self.inline(node)? {
                InlineView::Text(value) | InlineView::Code(value) => {
                    *present |= !value.is_empty();
                    if let Some((_, tail)) = value.rsplit_once('\n') {
                        row.clear();
                        row.push_str(tail);
                    } else {
                        row.push_str(value);
                    }
                }
                InlineView::Strong(children) | InlineView::Emphasis(children) => {
                    self.append_final_row(children, row, present)?;
                }
                InlineView::Link(link) => {
                    self.append_final_row(link.children(), row, present)?;
                }
                InlineView::LineBreak => {
                    row.clear();
                    *present = true;
                }
                InlineView::Anchor(_) => {}
            }
        }
        Ok(())
    }

    /// Count logical Unicode scalars in an inline root.
    ///
    /// Wrappers and anchors add no positions; every hard break contributes one.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] for unresolved content or an overflowing
    /// scalar count.
    pub fn scalar_len(self, nodes: &'store [Inline]) -> Result<usize, ContentReadError> {
        nodes.iter().try_fold(0usize, |total, node| {
            let length = match self.inline(node)? {
                InlineView::Text(text) | InlineView::Code(text) => text.chars().count(),
                InlineView::Strong(children) | InlineView::Emphasis(children) => {
                    self.scalar_len(children)?
                }
                InlineView::Link(link) => self.scalar_len(link.children())?,
                InlineView::LineBreak => 1,
                InlineView::Anchor(_) => 0,
            };
            total.checked_add(length).ok_or(ContentReadError)
        })
    }

    /// Project a checked owner-local content slice into logical scalar offsets.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when retained slice content does not
    /// resolve in this store or its scalar coordinates overflow.
    pub fn project_content_slice(
        self,
        owner: EntryOwner<'store>,
        slice: &EntryContentSlice,
    ) -> Result<Option<RootTextRange>, ContentReadError> {
        let Some(mut nodes) = owner.inline_root(&slice.root) else {
            return Ok(None);
        };
        let mut start = 0usize;
        for (depth, &index) in slice.path.iter().enumerate() {
            let Some(prefix) = nodes.get(..index) else {
                return Ok(None);
            };
            start = start
                .checked_add(self.scalar_len(prefix)?)
                .ok_or(ContentReadError)?;
            let Some(node) = nodes.get(index) else {
                return Ok(None);
            };
            nodes = if depth + 1 == slice.path.len() {
                std::slice::from_ref(node)
            } else {
                let Some(children) = (match self.inline(node)? {
                    InlineView::Strong(children) | InlineView::Emphasis(children) => Some(children),
                    InlineView::Link(link) => Some(link.children()),
                    _ => None,
                }) else {
                    return Ok(None);
                };
                children
            };
        }
        let length = if let Some(bytes) = &slice.bytes {
            if slice.path.is_empty() || bytes.start >= bytes.end {
                return Ok(None);
            }
            let [node] = nodes else {
                return Ok(None);
            };
            let (InlineView::Text(value) | InlineView::Code(value)) = self.inline(node)? else {
                return Ok(None);
            };
            let Some(selected) = value.get(bytes.clone()) else {
                return Ok(None);
            };
            let Some(prefix) = value.get(..bytes.start) else {
                return Ok(None);
            };
            start = start
                .checked_add(prefix.chars().count())
                .ok_or(ContentReadError)?;
            selected.chars().count()
        } else {
            self.scalar_len(nodes)?
        };
        let Some(end) = start.checked_add(length) else {
            return Err(ContentReadError);
        };
        Ok(Some(RootTextRange {
            root: slice.root.clone(),
            chars: start..end,
        }))
    }
}

impl crate::ContentProjection {
    /// Read authoritative content retained by this response-local projection.
    #[must_use]
    pub fn content(&self) -> ContentContext<'_> {
        ContentContext::new(self)
    }
}

impl crate::ContentStore {
    /// Read authoritative content while a producer assembles topology.
    #[must_use]
    pub fn content(&self) -> ContentContext<'_> {
        ContentContext::new(self)
    }
}

impl Document {
    /// Borrow the contextual reader for this document's authoritative content.
    #[must_use]
    pub fn content(&self) -> ContentContext<'_> {
        ContentContext::new(self)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        ContentByteRange, ContentContext, ContentLocation, ContentOwnerKind, ContentRef,
        ContentRootKind, ContentStoreBuilder, ContentStyle, Document, Inline, Provenance,
    };
    use serde_json::json;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn context_reads_text_and_resolves_links_from_one_document() {
        let document: Document = serde_json::from_value(json!({
            "sources": [{
                "key": 1,
                "identity": {"kind": "anonymous", "name": "test"},
                "format": "markdown",
                "decodedByteLength": 0,
                "coordinates": {"kind": "decoded-utf8-bytes"}
            }],
            "rootSource": 1,
            "contentStore": {
                "owners": [{"key": 1, "kind": "content", "roots": [1], "provenance": {"kind": "unknown"}}],
                "roots": [{"key": 1, "owner": 1, "kind": "heading", "atoms": [1], "points": [], "provenance": {"kind": "unknown"}}],
                "atoms": [{"key": 1, "root": 1, "owner": 1, "kind": "text", "text": "index", "link": 1, "provenance": {"kind": "unknown"}}],
                "points": [],
                "links": [{
                    "key": 1,
                    "owner": 1,
                    "target": {"kind": "document", "name": "index"},
                    "label": [{"kind": "content", "content": {"atom": 1, "bytes": {"start": 0, "end": 5}}}],
                    "provenance": {"kind": "unknown"}
                }]
            },
            "meta": {},
            "heading": {"content": [{
                "type": "link",
                "occurrence": 1,
                "children": [{"type": "text", "content": {"atom": 1, "bytes": {"start": 0, "end": 5}}}]
            }]},
            "sections": []
        }))
        .unwrap();
        let content = document.content();
        let location = ContentLocation::DocumentHeading { path: vec![0] };

        assert!(matches!(
            content.resolve_link(location.as_ref()),
            Some(Inline::Link { .. })
        ));
        let link = content
            .resolve_link_view(location.as_ref())
            .expect("valid content store")
            .expect("resolved link view");
        assert!(matches!(
            link.target(),
            crate::LinkTarget::Document { name, .. } if name == "index"
        ));
        assert_eq!(
            content.plain_text(link.children()).expect("valid content"),
            "index"
        );
        let heading = &document.heading.as_ref().unwrap().content;
        assert_eq!(
            content.plain_text(heading).expect("valid content"),
            crate::inline_plain_text(content, heading)
        );
        assert_eq!(
            content.scalar_len(heading).expect("valid content"),
            crate::inline_scalar_len(content, heading)
        );

        let _: ContentContext<'_> = content;
        assert_send_sync::<ContentContext<'static>>();
    }

    #[test]
    fn visitor_borrows_text_for_the_context_store_lifetime() {
        fn first<'store>(
            content: ContentContext<'store>,
            nodes: &'store [Inline],
        ) -> Result<Option<&'store str>, crate::ContentReadError> {
            let mut first = None;
            content.visit_plain_text(nodes, |text| {
                first.get_or_insert(text);
            })?;
            Ok(first)
        }

        let mut store = ContentStoreBuilder::new();
        let owner = store.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let root = store.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let content = store.push_text(
            root,
            "borrowed".into(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let nodes = [Inline::Text { content }];
        let store = store.finish();
        assert_eq!(first(store.content(), &nodes), Ok(Some("borrowed")));
    }

    #[test]
    fn unresolved_store_content_fails_instead_of_becoming_empty_text() {
        struct Missing<'a>(&'a [Inline]);
        impl super::ContentBackend for Missing<'_> {
            fn resolve(&self, _location: crate::ContentLocationRef<'_>) -> Option<&[Inline]> {
                Some(self.0)
            }

            fn inline<'content>(
                &'content self,
                _inline: &'content Inline,
            ) -> Option<super::InlineView<'content>> {
                None
            }

            fn store(&self) -> Option<&crate::ContentStore> {
                None
            }
        }

        let nodes = [Inline::Text {
            content: ContentRef {
                atom: crate::ContentAtomKey::FIRST,
                bytes: ContentByteRange { start: 0, end: 8 },
            },
        }];
        let missing = Missing(&nodes);
        let content = ContentContext::new(&missing);
        assert!(matches!(
            content.inline(&nodes[0]),
            Err(crate::ContentReadError)
        ));
        assert_eq!(content.plain_text(&nodes), Err(crate::ContentReadError));
        let location = ContentLocation::DocumentHeading { path: vec![0] };
        assert!(matches!(
            content.resolve_link_view(location.as_ref()),
            Err(crate::ContentReadError)
        ));
    }
}
