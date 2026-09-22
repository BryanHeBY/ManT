//! Borrowed semantic owners and references into their authoritative content.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

use crate::{
    Block, ContentContext, ContentReadError, DefinitionItem, EntryFacts, Inline, InlineView,
    ListItem,
};

/// The content owner of one semantic entry; no body is copied into the index.
#[derive(Debug, Clone, Copy)]
pub enum EntryOwner<'a> {
    /// Displayed definition terms and their description, from any producer.
    Definition(&'a DefinitionItem),
    /// An ordinary item whose original block content remains unchanged.
    List(&'a ListItem),
}

impl Block {
    /// Return the sole addressable owner of a single-item content excerpt.
    ///
    /// Multi-item lists and unannotated items are not entry excerpts. The original
    /// list kind, numbering, layout and content remain on this block.
    #[must_use]
    pub fn entry_owner(&self) -> Option<EntryOwner<'_>> {
        let owner = match self {
            Self::List { items, .. } if items.len() == 1 => EntryOwner::List(&items[0]),
            Self::DefinitionList { items, .. } if items.len() == 1 => {
                EntryOwner::Definition(&items[0])
            }
            _ => return None,
        };
        owner.facts().map(|_| owner)
    }
}

/// A direct inline container within an entry owner.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum EntryInlineRoot {
    /// One native definition term (zero-based).
    Term {
        /// Zero-based index in the definition's terms.
        index: usize,
    },
    /// One direct paragraph or preformatted block (zero-based).
    Block {
        /// Zero-based index in the owner's direct blocks.
        index: usize,
    },
}

/// A checked slice of the final IR, never an offset into original source bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryContentSlice {
    /// Direct owner container.
    pub root: EntryInlineRoot,
    /// Zero-based inline indices, descending through styled/link children.
    /// An empty path addresses the entire root inline container.
    pub path: Vec<usize>,
    /// Optional half-open UTF-8 byte range, allowed only on Text/Code leaves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<std::ops::Range<usize>>,
}

/// An authored form projected from ordered content slices rather than a template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryForm {
    /// Source-order pieces of this form; empty or invalid forms are rejected.
    pub parts: Vec<EntryContentSlice>,
}

impl EntryForm {
    /// Reference one complete displayed definition term, without copying it.
    #[must_use]
    pub fn term(index: usize) -> Self {
        Self {
            parts: vec![EntryContentSlice {
                root: EntryInlineRoot::Term { index },
                path: Vec::new(),
                bytes: None,
            }],
        }
    }
}

/// Validated, operation-local forms. Unknown is distinct from invalid references
/// (`EntryOwner::forms` returns `None` for the latter). No content fallback is
/// performed for either state.
#[derive(Debug, Default)]
pub enum EntryForms<'a> {
    /// The owner exists, but no authored forms were recorded.
    #[default]
    Unrecorded,
    /// Complete consecutive native terms borrowed as one slice.
    Borrowed(&'a [Vec<Inline>]),
    /// Explicit forms, borrowing complete roots and materializing only slices
    /// that actually require reconstruction of text or style wrappers.
    Projected(Vec<Cow<'a, [Inline]>>),
}

impl EntryForms<'_> {
    /// Iterate complete forms in their declared order, without cloning content.
    pub fn iter(&self) -> impl Iterator<Item = &[Inline]> {
        let (terms, projected): (&[Vec<Inline>], &[Cow<'_, [Inline]>]) = match self {
            Self::Unrecorded => (&[], &[]),
            Self::Borrowed(terms) => (terms, &[]),
            Self::Projected(forms) => (&[], forms),
        };
        terms
            .iter()
            .map(Vec::as_slice)
            .chain(projected.iter().map(AsRef::as_ref))
    }

    /// Materialize forms only at a boundary that needs owned output.
    #[must_use]
    pub fn into_owned(self) -> Vec<Vec<Inline>> {
        match self {
            Self::Unrecorded => Vec::new(),
            Self::Borrowed(terms) => terms.to_vec(),
            Self::Projected(forms) => forms.into_iter().map(Cow::into_owned).collect(),
        }
    }
}

/// Why a producer recognized a documented name; not a confidence score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EntryNameEvidence {
    /// An explicit source-format semantic mark.
    NativeMarkup,
    /// The finite source-independent name grammar.
    Lexical,
    /// An author's explicit semantic declaration on visible content.
    Declared,
}

/// A name's occurrences in the owner's displayed forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryNameBinding {
    /// Zero-based index in `EntryFacts.names`, not a global identity.
    pub name: usize,
    /// One or more complete occurrences, each represented by ordered slices.
    pub occurrences: Vec<EntryForm>,
    /// Evidence available to the producer.
    pub evidence: EntryNameEvidence,
}

impl<'a> EntryOwner<'a> {
    /// Source span attached to the original owner, never guessed from content.
    #[must_use]
    pub const fn source(self) -> Option<crate::SourceSpan> {
        match self {
            Self::Definition(item) => item.source,
            Self::List(item) => item.source,
        }
    }
    /// Facts attached to this owner, if it is addressable.
    #[must_use]
    pub const fn facts(self) -> Option<&'a EntryFacts> {
        match self {
            Self::Definition(item) => item.entry.as_ref(),
            Self::List(item) => item.entry.as_ref(),
        }
    }

    /// Authoritative content containing this owner's semantic children.
    #[must_use]
    pub fn blocks(self) -> &'a [Block] {
        match self {
            Self::Definition(item) => &item.description,
            Self::List(item) => &item.blocks,
        }
    }

    /// Whether direct semantic children form a nonempty set of values.
    #[must_use]
    pub fn has_value_choices(self) -> bool {
        match self {
            Self::Definition(item) => item.has_value_choices(),
            Self::List(item) => item.has_value_choices(),
        }
    }

    /// Borrow an owner-local inline root. Nested owners cannot be addressed here.
    #[must_use]
    pub fn inline_root(self, root: &EntryInlineRoot) -> Option<&'a [Inline]> {
        match root {
            EntryInlineRoot::Term { index } => match self {
                Self::Definition(item) => item.terms.get(*index).map(Vec::as_slice),
                Self::List(_) => None,
            },
            EntryInlineRoot::Block { index } => match self.blocks().get(*index)? {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    Some(children)
                }
                _ => None,
            },
        }
    }
}

impl<'store> ContentContext<'store> {
    /// Compare one form's visible text against an expected value without
    /// constructing a styled projection.
    ///
    /// Invalid form bindings compare unequal. Store-resolution failures remain
    /// distinct from malformed bindings.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when retained form content does not resolve
    /// in this store.
    pub fn entry_form_text_equals(
        self,
        owner: EntryOwner<'store>,
        form: &EntryForm,
        expected: &str,
    ) -> Result<bool, ContentReadError> {
        let mut remaining = expected;
        if form.parts.is_empty() || !form.parts.windows(2).all(|pair| pair[0].precedes(&pair[1])) {
            return Ok(false);
        }
        for part in &form.parts {
            let Some(mut nodes) = owner.inline_root(&part.root) else {
                return Ok(false);
            };
            for (depth, &index) in part.path.iter().enumerate() {
                let Some(node) = nodes.get(index) else {
                    return Ok(false);
                };
                if depth + 1 == part.path.len() {
                    nodes = std::slice::from_ref(node);
                } else {
                    let Some(children) = self.inline_children(node)? else {
                        return Ok(false);
                    };
                    nodes = children;
                }
            }
            if let Some(range) = &part.bytes {
                if part.path.is_empty() || range.start >= range.end {
                    return Ok(false);
                }
                let [node] = nodes else {
                    return Ok(false);
                };
                let (InlineView::Text(value) | InlineView::Code(value)) = self.inline(node)? else {
                    return Ok(false);
                };
                let Some(text) = value.get(range.clone()) else {
                    return Ok(false);
                };
                let Some(rest) = remaining.strip_prefix(text) else {
                    return Ok(false);
                };
                remaining = rest;
            } else if !self.consume_entry_text(nodes, &mut remaining)? {
                return Ok(false);
            }
        }
        Ok(remaining.is_empty())
    }

    /// Project one validated owner-local slice.
    ///
    /// Invalid references return `Ok(None)` and never produce partial content.
    /// This compatibility projection remains available while wire DTOs retain
    /// owned inline fragments.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when retained content does not resolve in
    /// this store.
    pub fn entry_content_slice(
        self,
        owner: EntryOwner<'store>,
        slice: &EntryContentSlice,
    ) -> Result<Option<Vec<Inline>>, ContentReadError> {
        let Some(mut nodes) = owner.inline_root(&slice.root) else {
            return Ok(None);
        };
        let Some((&last, parents)) = slice.path.split_last() else {
            if slice.bytes.is_some() {
                return Ok(None);
            }
            self.scalar_len(nodes)?;
            return Ok(Some(nodes.to_vec()));
        };
        let mut wrappers = Vec::new();
        for &index in parents {
            let Some(parent) = nodes.get(index) else {
                return Ok(None);
            };
            let Some(children) = self.inline_children(parent)? else {
                return Ok(None);
            };
            nodes = children;
            wrappers.push(parent);
        }
        let Some(node) = nodes.get(last) else {
            return Ok(None);
        };
        let mut selected = if let Some(range) = &slice.bytes {
            if range.start >= range.end {
                return Ok(None);
            }
            let view = self.inline(node)?;
            let (InlineView::Text(value) | InlineView::Code(value)) = view else {
                return Ok(None);
            };
            let Some(_) = value.get(range.clone()) else {
                return Ok(None);
            };
            let source = match node {
                Inline::Text { content } | Inline::Code { content } => *content,
                _ => return Ok(None),
            };
            let start = u32::try_from(range.start)
                .ok()
                .and_then(|offset| source.bytes.start.checked_add(offset));
            let end = u32::try_from(range.end)
                .ok()
                .and_then(|offset| source.bytes.start.checked_add(offset));
            let Some((start, end)) = start.zip(end).filter(|(_, end)| *end <= source.bytes.end)
            else {
                return Ok(None);
            };
            let content = crate::ContentRef {
                atom: source.atom,
                bytes: crate::ContentByteRange { start, end },
            };
            match view {
                InlineView::Code(_) => Inline::Code { content },
                _ => Inline::Text { content },
            }
        } else {
            self.scalar_len(std::slice::from_ref(node))?;
            node.clone()
        };
        for wrapper in wrappers.into_iter().rev() {
            let children = vec![selected];
            selected = match self.inline(wrapper)? {
                InlineView::Strong(_) => Inline::Strong { children },
                InlineView::Emphasis(_) => Inline::Emphasis { children },
                InlineView::Link(link) => Inline::Link {
                    occurrence: link.occurrence(),
                    children,
                },
                _ => return Ok(None),
            };
        }
        Ok(Some(vec![selected]))
    }

    /// Project complete authored forms, failing atomically on invalid bindings.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when retained form content does not resolve
    /// in this store.
    pub fn entry_forms(
        self,
        owner: EntryOwner<'store>,
    ) -> Result<Option<EntryForms<'store>>, ContentReadError> {
        let Some(facts) = owner.facts() else {
            return Ok(None);
        };
        if facts.forms.is_empty() {
            return Ok(Some(EntryForms::Unrecorded));
        }
        if let EntryOwner::Definition(item) = owner
            && facts.forms.len() == item.terms.len()
            && facts.forms.iter().enumerate().all(|(index, form)| {
                matches!(&form.parts[..], [EntryContentSlice {root: EntryInlineRoot::Term {index: term}, path, bytes: None}] if *term == index && path.is_empty())
            })
        {
            for term in &item.terms {
                self.scalar_len(term)?;
            }
            return Ok(Some(EntryForms::Borrowed(&item.terms)));
        }
        let mut forms = Vec::with_capacity(facts.forms.len());
        for form in &facts.forms {
            let Some(form) = self.entry_form(owner, form)? else {
                return Ok(None);
            };
            forms.push(form);
        }
        Ok(Some(EntryForms::Projected(forms)))
    }

    /// Count complete valid form bindings without copying inline content.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when retained form content does not resolve
    /// in this store.
    pub fn entry_validated_form_count(
        self,
        owner: EntryOwner<'store>,
    ) -> Result<Option<usize>, ContentReadError> {
        let Some(facts) = owner.facts() else {
            return Ok(None);
        };
        for form in &facts.forms {
            if !self.entry_form_is_valid(owner, form)? {
                return Ok(None);
            }
        }
        Ok(Some(facts.forms.len()))
    }

    /// Project one ordered form without accepting a partial binding.
    ///
    /// # Errors
    ///
    /// Returns [`ContentReadError`] when retained form content does not resolve
    /// in this store.
    pub fn entry_form(
        self,
        owner: EntryOwner<'store>,
        form: &EntryForm,
    ) -> Result<Option<Cow<'store, [Inline]>>, ContentReadError> {
        if !self.entry_form_is_valid(owner, form)? {
            return Ok(None);
        }
        if let [
            EntryContentSlice {
                root,
                path,
                bytes: None,
            },
        ] = &form.parts[..]
        {
            let Some(nodes) = owner.inline_root(root) else {
                return Ok(None);
            };
            if path.is_empty() {
                return Ok(Some(Cow::Borrowed(nodes)));
            }
            if let [index] = path[..] {
                return Ok(nodes.get(index..index.saturating_add(1)).map(Cow::Borrowed));
            }
        }
        let mut projected = Vec::new();
        for part in &form.parts {
            let Some(nodes) = self.entry_content_slice(owner, part)? else {
                return Ok(None);
            };
            projected.extend(nodes);
        }
        Ok(Some(Cow::Owned(projected)))
    }

    fn entry_form_is_valid(
        self,
        owner: EntryOwner<'store>,
        form: &EntryForm,
    ) -> Result<bool, ContentReadError> {
        if form.parts.is_empty() || !form.parts.windows(2).all(|pair| pair[0].precedes(&pair[1])) {
            return Ok(false);
        }
        for part in &form.parts {
            if !self.entry_slice_is_valid(owner, part)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn entry_slice_is_valid(
        self,
        owner: EntryOwner<'store>,
        slice: &EntryContentSlice,
    ) -> Result<bool, ContentReadError> {
        let Some(mut nodes) = owner.inline_root(&slice.root) else {
            return Ok(false);
        };
        let Some((&last, parents)) = slice.path.split_last() else {
            if slice.bytes.is_some() {
                return Ok(false);
            }
            self.scalar_len(nodes)?;
            return Ok(true);
        };
        for &index in parents {
            let Some(node) = nodes.get(index) else {
                return Ok(false);
            };
            let Some(children) = self.inline_children(node)? else {
                return Ok(false);
            };
            nodes = children;
        }
        let Some(node) = nodes.get(last) else {
            return Ok(false);
        };
        match &slice.bytes {
            None => {
                self.scalar_len(std::slice::from_ref(node))?;
                Ok(true)
            }
            Some(range) if range.start < range.end => match self.inline(node)? {
                InlineView::Text(value) | InlineView::Code(value) => {
                    Ok(value.get(range.clone()).is_some())
                }
                _ => Ok(false),
            },
            Some(_) => Ok(false),
        }
    }

    fn consume_entry_text(
        self,
        nodes: &'store [Inline],
        expected: &mut &str,
    ) -> Result<bool, ContentReadError> {
        for node in nodes {
            let text = match self.inline(node)? {
                InlineView::Text(value) | InlineView::Code(value) => value,
                InlineView::LineBreak => "\n",
                InlineView::Anchor(_) => continue,
                InlineView::Strong(children) | InlineView::Emphasis(children) => {
                    if !self.consume_entry_text(children, expected)? {
                        return Ok(false);
                    }
                    continue;
                }
                InlineView::Link(link) => {
                    if !self.consume_entry_text(link.children(), expected)? {
                        return Ok(false);
                    }
                    continue;
                }
            };
            let Some(rest) = expected.strip_prefix(text) else {
                return Ok(false);
            };
            *expected = rest;
        }
        Ok(true)
    }

    fn inline_children(
        self,
        inline: &'store Inline,
    ) -> Result<Option<&'store [Inline]>, ContentReadError> {
        Ok(match self.inline(inline)? {
            InlineView::Strong(children) | InlineView::Emphasis(children) => Some(children),
            InlineView::Link(link) => Some(link.children()),
            _ => None,
        })
    }
}

impl EntryContentSlice {
    /// Shared non-overlapping source-order check for forms and reference binding.
    pub(crate) fn precedes(&self, right: &Self) -> bool {
        if self.root != right.root {
            return self.root < right.root;
        }
        if self.path == right.path {
            return self
                .bytes
                .as_ref()
                .zip(right.bytes.as_ref())
                .is_some_and(|(a, b)| a.end <= b.start);
        }
        !self.path.starts_with(&right.path)
            && !right.path.starts_with(&self.path)
            && self.path < right.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Document, DocumentIndex, DocumentMeta, EntryKind, LayoutHint, ListKind, NameCase,
        SemanticIndex, SourceCoordinates, SourceFormat, SourceIdentity, SourceKey, SourceRecord,
        ValueDomain,
    };

    fn item(
        fixture: &mut crate::test_support::ContentFixture,
        id: &str,
        kind: EntryKind,
        name: &str,
    ) -> ListItem {
        ListItem {
            layout: crate::ListItemLayout::default(),
            source: None,
            entry: Some(EntryFacts {
                name_bindings: vec![EntryNameBinding {
                    name: 0,
                    evidence: EntryNameEvidence::Declared,
                    occurrences: vec![EntryForm {
                        parts: vec![EntryContentSlice {
                            root: EntryInlineRoot::Block { index: 0 },
                            path: vec![0],
                            bytes: None,
                        }],
                    }],
                }],
                alias_groups: Vec::new(),
                alias_of: None,
                id: id.into(),
                kind,
                case: NameCase::Sensitive,
                names: vec![name.into()],
                value_domain: None,
                forms: vec![EntryForm {
                    parts: vec![EntryContentSlice {
                        root: EntryInlineRoot::Block { index: 0 },
                        path: vec![0],
                        bytes: None,
                    }],
                }],
            }),
            blocks: vec![Block::Paragraph {
                children: vec![fixture.code(name), fixture.text(": Body — unchanged.")],
                layout: LayoutHint::default(),
                source: None,
            }],
        }
    }

    fn list(items: Vec<ListItem>) -> Block {
        Block::List {
            kind: ListKind::Ordered { start: Some(7) },
            items,
            compact: false,
            layout: LayoutHint::default(),
            source: None,
        }
    }

    fn document(content_store: crate::ContentStore, blocks: Vec<Block>) -> Document {
        Document {
            heading: None,
            parser: None,
            sources: vec![SourceRecord {
                key: SourceKey::FIRST,
                identity: SourceIdentity::Anonymous {
                    name: "test".to_owned(),
                },
                format: SourceFormat::Markdown,
                decoded_byte_length: 0,
                content_sha256: None,
                coordinates: SourceCoordinates::DecodedUtf8Bytes,
            }],
            root_source: SourceKey::FIRST,
            content_store,
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks,
            sections: Vec::new(),
        }
    }

    #[test]
    fn ordinary_owners_rebuild_indexes_and_choices_without_rewriting_content() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let mut parent = item(
            &mut fixture,
            "option-color",
            EntryKind::Parameter {
                parameter_kind: crate::ParameterKind::Option,
            },
            "--color",
        );
        let original = parent.blocks.clone();
        parent.blocks.push(list(vec![item(
            &mut fixture,
            "value-auto",
            EntryKind::Value,
            "auto",
        )]));
        parent.entry.as_mut().unwrap().value_domain =
            Some(ValueDomain::Choices { exhaustive: true });
        assert!(parent.has_value_choices());
        let doc = document(fixture.finish(), vec![list(vec![parent])]);
        let diagnostics = crate::validate_document(&doc);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let rebuilt: Document =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(rebuilt, doc);
        let index = SemanticIndex::build(&rebuilt);
        assert_eq!(index.root().len(), 1);
        assert_eq!(index.root()[0].forms, ["--color"]);
        assert_eq!(index.root()[0].children[0].names, ["auto"]);
        assert!(DocumentIndex::build(&rebuilt).contains("value-auto"));
        let Block::List {
            items,
            kind,
            compact,
            ..
        } = &rebuilt.blocks[0]
        else {
            panic!("ordinary list");
        };
        assert_eq!(
            (*kind, *compact),
            (ListKind::Ordered { start: Some(7) }, false)
        );
        assert_eq!(&items[0].blocks[..1], original);
        let mut unannotated = items[0].clone();
        unannotated.entry = None;
        assert_eq!(unannotated.blocks, items[0].blocks);
    }

    #[test]
    fn borrowed_form_count_agrees_with_atomic_projection_for_valid_and_invalid_bindings() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let original = item(&mut fixture, "term-name", EntryKind::Term, "é名");
        let store = fixture.finish();
        let content = store.content();
        for parts in [
            vec![],
            vec![EntryContentSlice {
                root: EntryInlineRoot::Block { index: 0 },
                path: vec![0],
                bytes: None,
            }],
            vec![EntryContentSlice {
                root: EntryInlineRoot::Block { index: 0 },
                path: vec![0],
                bytes: Some(0..2),
            }],
            vec![EntryContentSlice {
                root: EntryInlineRoot::Block { index: 0 },
                path: vec![0],
                bytes: Some(1..2),
            }],
            vec![EntryContentSlice {
                root: EntryInlineRoot::Block { index: 99 },
                path: vec![],
                bytes: None,
            }],
            vec![EntryContentSlice {
                root: EntryInlineRoot::Block { index: 0 },
                path: vec![],
                bytes: Some(0..2),
            }],
        ] {
            let mut item = original.clone();
            item.entry.as_mut().unwrap().forms = vec![EntryForm { parts }];
            let owner = EntryOwner::List(&item);
            assert_eq!(
                content.entry_validated_form_count(owner).unwrap(),
                content
                    .entry_forms(owner)
                    .unwrap()
                    .map(|forms| forms.iter().count())
            );
        }
        let mut item = original;
        item.entry.as_mut().unwrap().forms.clear();
        assert_eq!(
            content
                .entry_validated_form_count(EntryOwner::List(&item))
                .unwrap(),
            Some(0)
        );
    }

    #[test]
    fn form_slices_preserve_link_style_and_reject_invalid_utf8_and_owner_paths() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let mut item = item(&mut fixture, "term-name", EntryKind::Term, "é名");
        let link = fixture.link_text(
            crate::LinkTarget::Document {
                name: "other".into(),
                fragment: None,
            },
            None,
            "é名",
            true,
        );
        let Inline::Link {
            occurrence,
            children: link_children,
        } = link
        else {
            unreachable!();
        };
        let Block::Paragraph { children, .. } = &mut item.blocks[0] else {
            panic!("paragraph");
        };
        children[0] = Inline::Link {
            occurrence,
            children: vec![Inline::Strong {
                children: link_children,
            }],
        };
        let store = fixture.finish();
        let content = store.content();
        let slice = EntryContentSlice {
            root: EntryInlineRoot::Block { index: 0 },
            path: vec![0, 0, 0],
            bytes: Some(0..2),
        };
        let owner = EntryOwner::List(&item);
        let projected = content.entry_content_slice(owner, &slice).unwrap().unwrap();
        let [Inline::Link { children, .. }] = projected.as_slice() else {
            panic!("projected link wrapper");
        };
        let [Inline::Strong { children }] = children.as_slice() else {
            panic!("projected strong wrapper");
        };
        let [code @ Inline::Code { .. }] = children.as_slice() else {
            panic!("projected code leaf");
        };
        assert!(matches!(
            content.inline(code),
            Ok(crate::InlineView::Code("é"))
        ));
        for invalid in [
            EntryContentSlice {
                bytes: Some(1..2),
                ..slice.clone()
            },
            EntryContentSlice {
                bytes: Some(0..999),
                ..slice.clone()
            },
            EntryContentSlice {
                path: vec![0],
                ..slice.clone()
            },
            EntryContentSlice {
                root: EntryInlineRoot::Term { index: 0 },
                ..slice.clone()
            },
            EntryContentSlice {
                root: EntryInlineRoot::Block { index: 1 },
                ..slice.clone()
            },
        ] {
            assert!(
                content
                    .entry_content_slice(owner, &invalid)
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(owner.facts().unwrap().names, ["é名"]);
    }

    #[test]
    fn invalid_form_bindings_are_diagnostics_not_partial_index_facts() {
        for parts in [vec![], vec![0, 0], vec![1, 0], vec![0, 9]] {
            let mut fixture = crate::test_support::ContentFixture::body();
            let mut entry = item(
                &mut fixture,
                "option-probe",
                EntryKind::Parameter {
                    parameter_kind: crate::ParameterKind::Option,
                },
                "--probe",
            );
            entry.entry.as_mut().unwrap().forms[0].parts = parts
                .into_iter()
                .map(|index| EntryContentSlice {
                    root: EntryInlineRoot::Block { index: 0 },
                    path: vec![index],
                    bytes: None,
                })
                .collect();
            let doc = document(fixture.finish(), vec![list(vec![entry])]);
            let diagnostics = crate::validate_document(&doc);
            assert!(diagnostics.iter().any(|d| {
                d.code.as_deref() == Some("ir.invalid-entry-content")
                    && d.impact == crate::DiagnosticImpact::SemanticCoverage
            }));
            assert!(!crate::semantics_complete(&diagnostics));
            let index = SemanticIndex::build(&doc);
            assert_eq!(index.root().len(), 1);
            assert!(index.root()[0].forms.is_empty());
            assert!(index.root()[0].names.is_empty());
        }
    }

    #[test]
    fn explicit_terms_borrow_and_unrecorded_forms_do_not_remove_owners() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let one = fixture.code("one");
        let two = fixture.code("two");
        // The shared fixture root follows final document traversal: explicit
        // definition terms precede the parent's description and nested item.
        let list_item = item(&mut fixture, "parent", EntryKind::Term, "one");
        let child = item(&mut fixture, "child", EntryKind::Value, "auto");
        let mut description = list_item.blocks;
        description.push(list(vec![child]));
        let mut native = DefinitionItem {
            source: None,
            entry: list_item.entry,
            terms: vec![vec![one], vec![two]],
            description,
            layout: crate::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
                ..Default::default()
            },
        };
        let facts = native.entry.as_mut().unwrap();
        facts.names.clear();
        facts.name_bindings.clear();
        facts.forms = vec![EntryForm::term(0), EntryForm::term(1)];
        let store = fixture.finish();
        let content = store.content();
        let owner = EntryOwner::Definition(&native);
        let Some(EntryForms::Borrowed(terms)) = content.entry_forms(owner).unwrap() else {
            panic!("complete terms must borrow")
        };
        assert!(std::ptr::eq(terms, native.terms.as_slice()));
        assert!(
            matches!(content.entry_form(owner, &EntryForm::term(1)).unwrap(), Some(Cow::Borrowed(term)) if std::ptr::eq(term, native.terms[1].as_slice()))
        );
        native.entry.as_mut().unwrap().forms = vec![EntryForm::term(1), EntryForm::term(0)];
        let Some(EntryForms::Projected(forms)) = content
            .entry_forms(EntryOwner::Definition(&native))
            .unwrap()
        else {
            panic!("separate borrowed forms")
        };
        assert!(forms.iter().all(|form| matches!(form, Cow::Borrowed(_))));
        native.entry.as_mut().unwrap().forms.clear();
        assert!(matches!(
            content
                .entry_forms(EntryOwner::Definition(&native))
                .unwrap(),
            Some(EntryForms::Unrecorded)
        ));
        let doc = document(
            store,
            vec![Block::DefinitionList {
                declaration_groups: Vec::new(),
                items: vec![native],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
        );
        let index = SemanticIndex::build(&doc);
        assert!(index.root()[0].forms.is_empty());
        assert_eq!(index.root()[0].children[0].id, "child");
        let diagnostics = crate::validate_document(&doc);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn invalid_names_leave_valid_forms_and_children_intact() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let mut parent = item(&mut fixture, "parent", EntryKind::Command, "run");
        parent.entry.as_mut().unwrap().name_bindings.clear();
        parent.blocks.push(list(vec![item(
            &mut fixture,
            "child",
            EntryKind::Value,
            "auto",
        )]));
        let doc = document(fixture.finish(), vec![list(vec![parent])]);
        let index = SemanticIndex::build(&doc);
        assert!(index.root()[0].names.is_empty());
        assert_eq!(index.root()[0].forms, ["run"]);
        assert_eq!(index.root()[0].children.len(), 1);
        assert!(
            crate::validate_document(&doc)
                .iter()
                .any(|d| d.code.as_deref() == Some("ir.invalid-entry-name-binding"))
        );
    }

    #[test]
    fn empty_group_projection_never_bypasses_name_or_nonempty_group_validation() {
        for valid_names in [false, true] {
            for groups in [Vec::new(), vec![vec!["run".into(), "unbound".into()]]] {
                let mut fixture = crate::test_support::ContentFixture::body();
                let mut owner = item(&mut fixture, "run", EntryKind::Command, "run");
                let facts = owner.entry.as_mut().unwrap();
                facts.alias_groups = groups;
                if !valid_names {
                    facts.name_bindings.clear();
                }
                let doc = document(fixture.finish(), vec![list(vec![owner])]);
                let projected = SemanticIndex::build(&doc);
                assert_eq!(projected.root()[0].id, "run");
                assert_eq!(projected.root()[0].forms, ["run"]);
                assert_eq!(projected.root()[0].names.is_empty(), !valid_names);
                assert!(projected.root()[0].alias_groups.is_empty());
            }
        }
    }
}
