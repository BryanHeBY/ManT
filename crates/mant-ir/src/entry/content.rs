//! Borrowed semantic owners and references into their authoritative content.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

use crate::{Block, DefinitionItem, EntryFacts, Inline, ListItem};

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
    Borrowed(&'a [crate::DefinitionTerm]),
    /// Explicit forms, borrowing complete roots and materializing only slices
    /// that actually require reconstruction of text or style wrappers.
    Projected(Vec<Cow<'a, [Inline]>>),
}

impl EntryForms<'_> {
    /// Iterate complete forms in their declared order, without cloning content.
    pub fn iter(&self) -> impl Iterator<Item = &[Inline]> {
        let (terms, projected): (&[crate::DefinitionTerm], &[Cow<'_, [Inline]>]) = match self {
            Self::Unrecorded => (&[], &[]),
            Self::Borrowed(terms) => (terms, &[]),
            Self::Projected(forms) => (&[], forms),
        };
        terms
            .iter()
            .map(|term| term.content.as_slice())
            .chain(projected.iter().map(AsRef::as_ref))
    }

    /// Materialize forms only at a boundary that needs owned output.
    #[must_use]
    pub fn into_owned(self) -> Vec<Vec<Inline>> {
        match self {
            Self::Unrecorded => Vec::new(),
            Self::Borrowed(terms) => terms.iter().map(|term| term.content.clone()).collect(),
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
    /// Compare visible form text without constructing styled inline copies.
    /// Used by exact binding validation, independently of lookup case policy.
    pub(crate) fn form_text_equals(self, form: &EntryForm, mut expected: &str) -> bool {
        if form.parts.is_empty() || !form.parts.windows(2).all(|pair| pair[0].precedes(&pair[1])) {
            return false;
        }
        for part in &form.parts {
            let Some(mut nodes) = self.inline_root(&part.root) else {
                return false;
            };
            for (depth, &index) in part.path.iter().enumerate() {
                let Some(node) = nodes.get(index) else {
                    return false;
                };
                if depth + 1 == part.path.len() {
                    nodes = std::slice::from_ref(node);
                } else {
                    nodes = match node {
                        Inline::Strong { children }
                        | Inline::Emphasis { children }
                        | Inline::Link { children, .. } => children,
                        _ => return false,
                    };
                }
            }
            if let Some(range) = &part.bytes {
                if part.path.is_empty() || range.start >= range.end {
                    return false;
                }
                let [
                    Inline::Text { value }
                    | Inline::Code { value }
                    | Inline::Equation { value, .. },
                ] = nodes
                else {
                    return false;
                };
                let Some(text) = value.get(range.clone()) else {
                    return false;
                };
                let Some(rest) = expected.strip_prefix(text) else {
                    return false;
                };
                expected = rest;
            } else if !consume_text(nodes, &mut expected) {
                return false;
            }
        }
        expected.is_empty()
    }

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
                Self::Definition(item) => {
                    item.terms.get(*index).map(|term| term.content.as_slice())
                }
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

    /// Borrow a complete owner-local root with its exceptional row origins.
    #[must_use]
    pub fn inline_content_root(
        self,
        root: &EntryInlineRoot,
    ) -> Option<crate::InlineContentRef<'a>> {
        match root {
            EntryInlineRoot::Term { index } => match self {
                Self::Definition(item) => item
                    .terms
                    .get(*index)
                    .map(crate::DefinitionTerm::inline_content),
                Self::List(_) => None,
            },
            EntryInlineRoot::Block { index } => match self.blocks().get(*index)? {
                Block::Paragraph {
                    children,
                    inline_layout,
                    ..
                }
                | Block::Preformatted {
                    children,
                    inline_layout,
                    ..
                } => Some(crate::InlineContentRef {
                    content: children,
                    layout: inline_layout,
                }),
                _ => None,
            },
        }
    }

    /// Project one validated slice; invalid references never produce partial text.
    #[must_use]
    pub fn content_slice(self, slice: &EntryContentSlice) -> Option<Vec<Inline>> {
        let mut nodes = self.inline_root(&slice.root)?;
        let Some((&last, parents)) = slice.path.split_last() else {
            return slice.bytes.is_none().then(|| nodes.to_vec());
        };
        let mut wrappers = Vec::new();
        for &index in parents {
            let parent = nodes.get(index)?;
            nodes = match parent {
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => children,
                _ => return None,
            };
            wrappers.push(parent);
        }
        let node = nodes.get(last)?;
        let mut selected = if let Some(range) = &slice.bytes {
            if range.start >= range.end {
                return None;
            }
            let value = match node {
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => value.get(range.clone())?,
                _ => return None,
            };
            match node {
                Inline::Code { .. } | Inline::Equation { .. } => Inline::Code {
                    value: value.into(),
                },
                _ => Inline::Text {
                    value: value.into(),
                },
            }
        } else {
            node.clone()
        };
        for wrapper in wrappers.into_iter().rev() {
            let children = vec![selected];
            selected = match wrapper {
                Inline::Strong { .. } => Inline::Strong { children },
                Inline::Emphasis { .. } => Inline::Emphasis { children },
                Inline::Link { target, title, .. } => Inline::Link {
                    target: target.clone(),
                    title: title.clone(),
                    children,
                },
                _ => return None,
            };
        }
        Some(vec![selected])
    }

    /// Project complete authored forms, failing atomically on invalid bindings.
    /// Empty bindings are unknown even when displayed native terms exist.
    /// An absent owner or any invalid form returns `None`, never a partial set.
    #[must_use]
    pub fn forms(self) -> Option<EntryForms<'a>> {
        let facts = self.facts()?;
        if facts.forms.is_empty() {
            return Some(EntryForms::Unrecorded);
        }
        if let Self::Definition(item) = self
            && facts.forms.len() == item.terms.len()
            && facts.forms.iter().enumerate().all(|(index, form)| {
                matches!(&form.parts[..], [EntryContentSlice {root: EntryInlineRoot::Term {index: term}, path, bytes: None}] if *term == index && path.is_empty())
            })
        {
            return Some(EntryForms::Borrowed(&item.terms));
        }
        facts
            .forms
            .iter()
            .map(|form| self.form(form))
            .collect::<Option<Vec<_>>>()
            .map(EntryForms::Projected)
    }

    /// Layout of a form that selects one contiguous interval of a real root.
    /// Composite noncontiguous labels have no source row geometry to inherit.
    #[must_use]
    pub fn form_inline_layout(self, form: &EntryForm) -> Option<crate::InlineLayout> {
        let mut parts = form.parts.iter();
        let first = crate::project_content_slice(self, parts.next()?)?;
        let mut end = first.chars.end;
        for part in parts {
            let next = crate::project_content_slice(self, part)?;
            if next.root != first.root || next.chars.start != end {
                return None;
            }
            end = next.chars.end;
        }
        self.inline_content_root(&first.root)?
            .sliced_layout(first.chars.start..end)
    }

    /// Count complete valid form bindings without copying inline content or names.
    ///
    /// This has the same atomic validity rule as [`Self::forms`]; invalid bindings
    /// yield `None`, and an explicitly unrecorded form set yields zero.
    #[must_use]
    pub fn validated_form_count(self) -> Option<usize> {
        let forms = &self.facts()?.forms;
        forms
            .iter()
            .all(|form| self.form_is_valid(form))
            .then_some(forms.len())
    }

    fn form_is_valid(self, form: &EntryForm) -> bool {
        !form.parts.is_empty()
            && form.parts.windows(2).all(|pair| pair[0].precedes(&pair[1]))
            && form.parts.iter().all(|part| self.slice_is_valid(part))
    }

    fn slice_is_valid(self, slice: &EntryContentSlice) -> bool {
        let Some(mut nodes) = self.inline_root(&slice.root) else {
            return false;
        };
        let Some((&last, parents)) = slice.path.split_last() else {
            return slice.bytes.is_none();
        };
        for &index in parents {
            let Some(
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. },
            ) = nodes.get(index)
            else {
                return false;
            };
            nodes = children;
        }
        let Some(node) = nodes.get(last) else {
            return false;
        };
        match &slice.bytes {
            None => true,
            Some(range) if range.start < range.end => match node {
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => value.get(range.clone()).is_some(),
                _ => false,
            },
            Some(_) => false,
        }
    }

    /// Project one ordered form without accepting a partial binding.
    #[must_use]
    pub fn form(self, form: &EntryForm) -> Option<Cow<'a, [Inline]>> {
        if !self.form_is_valid(form) {
            return None;
        }
        if let [
            EntryContentSlice {
                root,
                path,
                bytes: None,
            },
        ] = &form.parts[..]
        {
            let nodes = self.inline_root(root)?;
            if path.is_empty() {
                return Some(Cow::Borrowed(nodes));
            }
            if let [index] = path[..] {
                return nodes.get(index..index.checked_add(1)?).map(Cow::Borrowed);
            }
        }
        let parts = form
            .parts
            .iter()
            .map(|part| self.content_slice(part))
            .collect::<Option<Vec<_>>>()?;
        Some(Cow::Owned(parts.into_iter().flatten().collect()))
    }
}

fn consume_text(nodes: &[Inline], expected: &mut &str) -> bool {
    for node in nodes {
        let text = match node {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                value.as_str()
            }
            Inline::LineBreak { .. } => "\n",
            Inline::Anchor { .. } => continue,
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                if !consume_text(children, expected) {
                    return false;
                }
                continue;
            }
        };
        let Some(rest) = expected.strip_prefix(text) else {
            return false;
        };
        *expected = rest;
    }
    true
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
mod tests;
