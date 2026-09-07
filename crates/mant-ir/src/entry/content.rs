//! Borrowed semantic owners and references into their authoritative content.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Block, DefinitionItem, EntryFacts, Inline, ListItem};

/// The content owner of one semantic entry; no body is copied into the index.
#[derive(Debug, Clone, Copy)]
pub enum EntryOwner<'a> {
    /// Native definition terms and their description.
    Definition(&'a DefinitionItem),
    /// An ordinary item whose Markdown content remains unchanged.
    List(&'a ListItem),
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
    /// Facts attached to this owner, if it is addressable.
    #[must_use]
    pub const fn facts(self) -> Option<&'a EntryFacts> {
        match self {
            Self::Definition(item) => item.identity.as_ref(),
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
    /// Native owners without explicit bindings retain their original terms.
    #[must_use]
    pub fn forms(self) -> Option<std::borrow::Cow<'a, [Vec<Inline>]>> {
        let facts = self.facts()?;
        if facts.forms.is_empty() {
            return match self {
                Self::Definition(item) => Some(std::borrow::Cow::Borrowed(&item.terms)),
                Self::List(_) => None,
            };
        }
        facts
            .forms
            .iter()
            .map(|form| self.form(form))
            .collect::<Option<Vec<_>>>()
            .map(std::borrow::Cow::Owned)
    }

    /// Project one ordered form without accepting a partial binding.
    #[must_use]
    pub fn form(self, form: &EntryForm) -> Option<Vec<Inline>> {
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
        Some(parts.into_iter().flatten().collect())
    }
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
        DefinitionCase, DefinitionRole, Document, DocumentIndex, DocumentMeta, DocumentSource,
        LayoutHint, ListKind, SemanticIndex, SourceFormat, ValueDomain,
    };

    fn item(id: &str, role: DefinitionRole, name: &str) -> ListItem {
        ListItem {
            entry: Some(EntryFacts {
                name_bindings: Vec::new(),
                alias_groups: Vec::new(),
                alias_of: None,
                id: id.into(),
                role,
                case: DefinitionCase::Sensitive,
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
            kind: ListKind::Ordered,
            start: Some(7),
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
        let mut parent = item("option-color", DefinitionRole::Option, "--color");
        let original = parent.blocks.clone();
        parent.blocks.push(list(vec![item(
            "value-auto",
            DefinitionRole::Value,
            "auto",
        )]));
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
        assert_eq!(index.root()[0].children[0].aliases, ["auto"]);
        assert!(DocumentIndex::build(&rebuilt).contains("value-auto"));
        let Block::List {
            items,
            kind,
            start,
            compact,
            ..
        } = &rebuilt.blocks[0]
        else {
            panic!("ordinary list");
        };
        assert_eq!(
            (*kind, *start, *compact),
            (ListKind::Ordered, Some(7), false)
        );
        assert_eq!(&items[0].blocks[..1], original);
        let mut unannotated = items[0].clone();
        unannotated.entry = None;
        assert_eq!(unannotated.blocks, items[0].blocks);
    }

    #[test]
    fn form_slices_preserve_link_style_and_reject_invalid_utf8_and_owner_paths() {
        let mut item = item("term-name", DefinitionRole::Term, "é名");
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
            let mut entry = item("option-probe", DefinitionRole::Option, "--probe");
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
            assert!(SemanticIndex::build(&doc).root().is_empty());
        }
        assert!(crate::is_semantic_completeness_diagnostic(
            "ir.invalid-entry-content"
        ));
    }
}
