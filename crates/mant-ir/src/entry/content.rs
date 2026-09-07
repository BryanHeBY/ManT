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
    /// Compare visible form text without constructing styled inline copies.
    /// Used by exact binding validation, independently of lookup case policy.
    pub(crate) fn form_text_equals(self, form: &EntryForm, mut expected: &str) -> bool {
        if form.parts.is_empty()
            || !form
                .parts
                .windows(2)
                .all(|pair| precedes(&pair[0], &pair[1]))
        {
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
                let [Inline::Text { value } | Inline::Code { value }] = nodes else {
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
                Inline::Text { value } | Inline::Code { value } => value.get(range.clone())?,
                _ => return None,
            };
            match node {
                Inline::Code { .. } => Inline::Code {
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

    /// Project one ordered form without accepting a partial binding.
    #[must_use]
    pub fn form(self, form: &EntryForm) -> Option<Cow<'a, [Inline]>> {
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
        if form.parts.is_empty()
            || !form
                .parts
                .windows(2)
                .all(|pair| precedes(&pair[0], &pair[1]))
        {
            return None;
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
            Inline::Text { value } | Inline::Code { value } => value.as_str(),
            Inline::LineBreak => "\n",
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

fn precedes(left: &EntryContentSlice, right: &EntryContentSlice) -> bool {
    if left.root != right.root {
        return left.root < right.root;
    }
    if left.path == right.path {
        return left
            .bytes
            .as_ref()
            .zip(right.bytes.as_ref())
            .is_some_and(|(a, b)| a.end <= b.start);
    }
    !left.path.starts_with(&right.path)
        && !right.path.starts_with(&left.path)
        && left.path < right.path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Document, DocumentIndex, DocumentMeta, DocumentSource, EntryKind, LayoutHint, ListKind,
        NameCase, SemanticIndex, SourceFormat, ValueDomain,
    };

    fn item(id: &str, kind: EntryKind, name: &str) -> ListItem {
        ListItem {
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
                children: vec![
                    Inline::Code { value: name.into() },
                    Inline::Text {
                        value: ": Body — unchanged.".into(),
                    },
                ],
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

    fn document(blocks: Vec<Block>) -> Document {
        Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Markdown,
                path: None,
            },
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks,
            sections: Vec::new(),
        }
    }

    #[test]
    fn ordinary_owners_rebuild_indexes_and_choices_without_rewriting_content() {
        let mut parent = item(
            "option-color",
            EntryKind::Parameter {
                parameter_kind: crate::ParameterKind::Option,
            },
            "--color",
        );
        let original = parent.blocks.clone();
        parent
            .blocks
            .push(list(vec![item("value-auto", EntryKind::Value, "auto")]));
        parent.entry.as_mut().unwrap().value_domain =
            Some(ValueDomain::Choices { exhaustive: true });
        assert!(parent.has_value_choices());
        let doc = document(vec![list(vec![parent])]);
        assert!(crate::validate_document(&doc).is_empty());
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
    fn form_slices_preserve_link_style_and_reject_invalid_utf8_and_owner_paths() {
        let mut item = item("term-name", EntryKind::Term, "é名");
        let Block::Paragraph { children, .. } = &mut item.blocks[0] else {
            panic!("paragraph");
        };
        children[0] = Inline::Link {
            target: crate::LinkTarget::Document {
                name: "other".into(),
                fragment: None,
            },
            title: None,
            children: vec![Inline::Strong {
                children: vec![Inline::Code {
                    value: "é名".into(),
                }],
            }],
        };
        let slice = EntryContentSlice {
            root: EntryInlineRoot::Block { index: 0 },
            path: vec![0, 0, 0],
            bytes: Some(0..2),
        };
        let owner = EntryOwner::List(&item);
        assert!(
            matches!(owner.content_slice(&slice).unwrap().as_slice(), [Inline::Link { children, .. }]
            if matches!(children.as_slice(), [Inline::Strong { children }] if matches!(children.as_slice(), [Inline::Code { value }] if value == "é")))
        );
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
            assert!(owner.content_slice(&invalid).is_none());
        }
        assert_eq!(owner.facts().unwrap().names, ["é名"]);
    }

    #[test]
    fn invalid_form_bindings_are_diagnostics_not_partial_index_facts() {
        for parts in [vec![], vec![0, 0], vec![1, 0], vec![0, 9]] {
            let mut entry = item(
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
            let doc = document(vec![list(vec![entry])]);
            assert!(
                crate::validate_document(&doc)
                    .iter()
                    .any(|d| d.code.as_deref() == Some("ir.invalid-entry-content"))
            );
            let index = SemanticIndex::build(&doc);
            assert_eq!(index.root().len(), 1);
            assert!(index.root()[0].forms.is_empty());
            assert!(index.root()[0].names.is_empty());
        }
        assert!(crate::is_semantic_completeness_diagnostic(
            "ir.invalid-entry-content"
        ));
    }

    #[test]
    fn explicit_terms_borrow_and_unrecorded_forms_do_not_remove_owners() {
        let list_item = item("parent", EntryKind::Term, "one");
        let mut native = DefinitionItem {
            source: None,
            entry: list_item.entry,
            terms: vec![
                vec![Inline::Code {
                    value: "one".into(),
                }],
                vec![Inline::Code {
                    value: "two".into(),
                }],
            ],
            description: vec![list(vec![item("child", EntryKind::Value, "auto")])],
            layout: crate::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
            },
        };
        let facts = native.entry.as_mut().unwrap();
        facts.names.clear();
        facts.name_bindings.clear();
        facts.forms = vec![EntryForm::term(0), EntryForm::term(1)];
        let owner = EntryOwner::Definition(&native);
        let Some(EntryForms::Borrowed(terms)) = owner.forms() else {
            panic!("complete terms must borrow")
        };
        assert!(std::ptr::eq(terms, native.terms.as_slice()));
        assert!(
            matches!(owner.form(&EntryForm::term(1)), Some(Cow::Borrowed(term)) if std::ptr::eq(term, native.terms[1].as_slice()))
        );
        native.entry.as_mut().unwrap().forms = vec![EntryForm::term(1), EntryForm::term(0)];
        let Some(EntryForms::Projected(forms)) = EntryOwner::Definition(&native).forms() else {
            panic!("separate borrowed forms")
        };
        assert!(forms.iter().all(|form| matches!(form, Cow::Borrowed(_))));
        native.entry.as_mut().unwrap().forms.clear();
        assert!(matches!(
            EntryOwner::Definition(&native).forms(),
            Some(EntryForms::Unrecorded)
        ));
        let doc = document(vec![Block::DefinitionList {
            items: vec![native],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }]);
        let index = SemanticIndex::build(&doc);
        assert!(index.root()[0].forms.is_empty());
        assert_eq!(index.root()[0].children[0].id, "child");
        assert!(crate::validate_document(&doc).is_empty());
    }

    #[test]
    fn invalid_names_leave_valid_forms_and_children_intact() {
        let mut parent = item("parent", EntryKind::Command, "run");
        parent.entry.as_mut().unwrap().name_bindings.clear();
        parent
            .blocks
            .push(list(vec![item("child", EntryKind::Value, "auto")]));
        let doc = document(vec![list(vec![parent])]);
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
}
