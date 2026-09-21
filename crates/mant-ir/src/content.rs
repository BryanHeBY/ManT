//! Contextual reads from one authoritative inline-content store.
//!
//! The current IR still stores text directly in [`Inline`] leaves. Keeping
//! reads behind this store-bound facade lets a document and a future
//! response-local content projection share one access path without copying
//! text or exposing a second store.

use std::{error::Error, fmt};

use crate::{
    ContentLocationRef, Document, EntryContentSlice, EntryOwner, FragmentAlias, Inline, LinkTarget,
    NodeId, RootTextRange, SourceSpan,
};

/// One store-resolved inline node.
///
/// Consumers match this view instead of reading storage fields from [`Inline`]
/// directly. The view can therefore remain stable when leaves become content
/// references and link metadata moves into an occurrence table.
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
    target: &'store LinkTarget,
    title: Option<&'store str>,
    children: &'store [Inline],
}

impl<'store> LinkView<'store> {
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
    id: &'store NodeId,
    fragment_aliases: &'store [FragmentAlias],
    owner_source: Option<SourceSpan>,
}

impl<'store> AnchorView<'store> {
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
        Some(match inline {
            Inline::Text { value } => InlineView::Text(value),
            Inline::Code { value } => InlineView::Code(value),
            Inline::Strong { children } => InlineView::Strong(children),
            Inline::Emphasis { children } => InlineView::Emphasis(children),
            Inline::Link {
                target,
                title,
                children,
            } => InlineView::Link(LinkView {
                target,
                title: title.as_deref(),
                children,
            }),
            Inline::Anchor {
                id,
                fragment_aliases,
                owner_source,
            } => InlineView::Anchor(AnchorView {
                id,
                fragment_aliases,
                owner_source: *owner_source,
            }),
            Inline::LineBreak => InlineView::LineBreak,
        })
    }
}

impl ContentBackend for Document {
    fn resolve(&self, location: ContentLocationRef<'_>) -> Option<&[Inline]> {
        crate::content_location::resolve_location(self, location)
    }
}

struct DetachedContent;

impl ContentBackend for DetachedContent {
    fn resolve(&self, _location: ContentLocationRef<'_>) -> Option<&[Inline]> {
        None
    }
}

static DETACHED_CONTENT: DetachedContent = DetachedContent;

/// Read-only access to one authoritative inline-content store.
///
/// A context never owns or copies an alternate text store. Obtain the document
/// context through [`Document::content`]. The backend is private so future
/// response-local projections can reuse this facade without exposing parallel
/// public resolver contracts.
#[derive(Clone, Copy)]
pub struct ContentContext<'store> {
    backend: &'store dyn ContentBackend,
}

impl fmt::Debug for ContentContext<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContentContext")
            .finish_non_exhaustive()
    }
}

impl<'store> ContentContext<'store> {
    pub(crate) fn new(backend: &'store dyn ContentBackend) -> Self {
        Self { backend }
    }

    pub(crate) const fn detached() -> Self {
        Self {
            backend: &DETACHED_CONTENT,
        }
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
    pub(crate) fn project_content_slice(
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
                let Some(children) = inline_children(node) else {
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

fn inline_children(inline: &Inline) -> Option<&[Inline]> {
    match inline {
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => Some(children),
        _ => None,
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
    use crate::{ContentContext, ContentLocation, Document, Inline};
    use serde_json::json;

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
            "meta": {},
            "heading": {"content": [{
                "type": "link",
                "target": {"kind": "document", "name": "index"},
                "children": [{"type": "text", "value": "index"}]
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
            crate::inline_plain_text(heading)
        );
        assert_eq!(
            content.scalar_len(heading).expect("valid content"),
            crate::inline_scalar_len(heading)
        );

        let _: ContentContext<'_> = content;
        fn assert_send_sync<T: Send + Sync>() {}
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

        let nodes = [Inline::Text {
            value: "borrowed".into(),
        }];
        assert_eq!(
            first(ContentContext::detached(), &nodes),
            Ok(Some("borrowed"))
        );
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
        }

        let nodes = [Inline::Text {
            value: "dangling".into(),
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
