//! Operation-local mappings of validated source bindings, never name searches.
use std::{collections::HashMap, marker::PhantomData, ops::Range};

use mant_ir::{
    Block, Document, EntryContentSlice, EntryInlineRoot, EntryKind, EntryOwner, Inline,
    visit::{self, Visit},
};

/// A half-open Unicode scalar range within one original owner-local inline root.
/// Styling wrappers and zero-width anchors consume no positions; `LineBreak`
/// consumes one. Replacing a control scalar by U+FFFD preserves these positions.
/// Renderer-generated indentation, separators and quoting are not in this domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootTextRange {
    /// Original term or direct paragraph/preformatted block, not a form ordinal.
    pub root: EntryInlineRoot,
    /// Scalar offsets within this root's visible text.
    pub chars: Range<usize>,
}

/// Project a checked UTF-8 source slice without cloning its text or wrappers.
/// Invalid paths, split UTF-8 scalars and out-of-range references return None.
/// This performs coordinate validation, not semantic name validation; callers
/// must first obtain validated names before treating a slice as a name binding.
#[must_use]
pub fn project_content_slice(
    owner: EntryOwner<'_>,
    slice: &EntryContentSlice,
) -> Option<RootTextRange> {
    let mut nodes = owner.inline_root(&slice.root)?;
    let mut start = 0;
    for (depth, &index) in slice.path.iter().enumerate() {
        start += scalar_len(nodes.get(..index)?);
        let node = nodes.get(index)?;
        nodes = if depth + 1 == slice.path.len() {
            std::slice::from_ref(node)
        } else {
            match node {
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => children,
                _ => return None,
            }
        };
    }
    let length = if let Some(bytes) = &slice.bytes {
        if slice.path.is_empty() || bytes.start >= bytes.end {
            return None;
        }
        let [Inline::Text { value } | Inline::Code { value }] = nodes else {
            return None;
        };
        let selected = value.get(bytes.clone())?;
        start += value.get(..bytes.start)?.chars().count();
        selected.chars().count()
    } else {
        scalar_len(nodes)
    };
    Some(RootTextRange {
        root: slice.root.clone(),
        chars: start..start + length,
    })
}

pub(super) fn scalar_len(nodes: &[Inline]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Inline::Text { value } | Inline::Code { value } => value.chars().count(),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => scalar_len(children),
            Inline::LineBreak => 1,
            Inline::Anchor { .. } => 0,
        })
        .sum()
}

/// One validated semantic-name span in an original inline root.
/// No query match or equivalence is implied by this ordinary display binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineNameRange {
    /// Unicode scalar offsets within the supplied root.
    pub chars: Range<usize>,
    /// Full source-neutral kind, including parameter subtype.
    pub kind: EntryKind,
}

/// Borrow-scoped lookup prepared once for an immutable rendering operation.
///
/// Keys identify actual inline containers, not text or names, so equal strings
/// in other owners never acquire a type color. Addresses are never dereferenced
/// or serialized. The borrow prevents caching the map beyond the source tree.
/// This does not rebuild the semantic index and has no explain response limits.
#[derive(Debug, Default)]
pub struct EntryStyleMap<'a> {
    roots: HashMap<usize, Vec<InlineNameRange>>,
    source: PhantomData<&'a Inline>,
}

impl<'a> EntryStyleMap<'a> {
    /// Collect all validated ordinary name bindings from a document.
    #[must_use]
    pub fn for_document(document: &'a Document) -> Self {
        let mut map = Self::default();
        map.visit_document(document);
        map.normalize();
        map
    }

    /// Collect bindings for already materialized content, including nested owners.
    #[must_use]
    pub fn for_blocks(blocks: &'a [Block]) -> Self {
        let mut map = Self::default();
        for block in blocks {
            map.visit_block(block);
        }
        map.normalize();
        map
    }

    /// Collect bindings within a materialized section and its descendants.
    #[must_use]
    pub fn for_section(section: &'a mant_ir::Section) -> Self {
        let mut map = Self::default();
        map.visit_section(section);
        map.normalize();
        map
    }

    /// Exact-root lookup; empty/unbound containers return no semantic decoration.
    #[must_use]
    pub fn ranges(&self, nodes: &[Inline]) -> &[InlineNameRange] {
        if nodes.is_empty() {
            return &[];
        }
        self.roots
            .get(&(nodes.as_ptr() as usize))
            .map_or(&[], Vec::as_slice)
    }

    fn owner(&mut self, owner: EntryOwner<'a>) {
        let Some(names) = owner.validated_names() else {
            return;
        };
        if names.is_empty() {
            return;
        }
        let facts = owner.facts().expect("validated owner has facts");
        for binding in &facts.name_bindings {
            for occurrence in &binding.occurrences {
                // Name validation has already accepted the complete occurrence;
                // this projection cannot accept just a partial malformed name.
                let ranges = occurrence
                    .parts
                    .iter()
                    .map(|part| project_content_slice(owner, part))
                    .collect::<Option<Vec<_>>>();
                let Some(ranges) = ranges else {
                    continue;
                };
                for range in ranges {
                    if range.chars.is_empty() {
                        continue;
                    }
                    let nodes = owner
                        .inline_root(&range.root)
                        .expect("projected root exists");
                    self.roots
                        .entry(nodes.as_ptr() as usize)
                        .or_default()
                        .push(InlineNameRange {
                            chars: range.chars,
                            kind: facts.kind,
                        });
                }
            }
        }
    }

    fn normalize(&mut self) {
        for ranges in self.roots.values_mut() {
            ranges.sort_by_key(|range| (range.chars.start, range.chars.end));
            let mut kept = 0;
            for index in 0..ranges.len() {
                if kept > 0 && ranges[kept - 1].chars.end >= ranges[index].chars.start {
                    // All bindings on one root belong to the same real owner.
                    ranges[kept - 1].chars.end =
                        ranges[kept - 1].chars.end.max(ranges[index].chars.end);
                } else {
                    ranges.swap(kept, index);
                    kept += 1;
                }
            }
            ranges.truncate(kept);
        }
    }
}

impl<'a> Visit<'a> for EntryStyleMap<'a> {
    fn visit_definition_item(&mut self, item: &'a mant_ir::DefinitionItem) {
        self.owner(EntryOwner::Definition(item));
        visit::walk_definition_item(self, item);
    }
    fn visit_list_item(&mut self, item: &'a mant_ir::ListItem) {
        self.owner(EntryOwner::List(item));
        visit::walk_list_item(self, item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::{
        EntryFacts, EntryForm, EntryNameBinding, EntryNameEvidence, LayoutHint, ListItem, NameCase,
    };

    fn item() -> ListItem {
        ListItem {
            layout: mant_ir::ListItemLayout::default(),
            source: None,
            blocks: vec![Block::Paragraph {
                children: vec![
                    Inline::Text {
                        value: "前\n".into(),
                    },
                    Inline::Strong {
                        children: vec![Inline::Code {
                            value: "é名-param".into(),
                        }],
                    },
                    Inline::Text {
                        value: " é名-param ordinary".into(),
                    },
                ],
                layout: LayoutHint::default(),
                source: None,
            }],
            entry: Some(EntryFacts {
                id: "term-name".into(),
                kind: EntryKind::Term,
                case: NameCase::Sensitive,
                names: vec!["é名".into()],
                alias_groups: vec![],
                alias_of: None,
                value_domain: None,
                forms: vec![EntryForm {
                    parts: vec![slice(None)],
                }],
                name_bindings: vec![EntryNameBinding {
                    name: 0,
                    evidence: EntryNameEvidence::Declared,
                    occurrences: vec![EntryForm {
                        parts: vec![slice(Some(0..5))],
                    }],
                }],
            }),
        }
    }

    fn slice(bytes: Option<Range<usize>>) -> EntryContentSlice {
        EntryContentSlice {
            root: EntryInlineRoot::Block { index: 0 },
            path: vec![1, 0],
            bytes,
        }
    }

    #[test]
    fn maps_nested_utf8_slices_without_styling_equal_body_text() {
        let item = item();
        let owner = EntryOwner::List(&item);
        assert_eq!(
            project_content_slice(owner, &slice(Some(0..5)))
                .unwrap()
                .chars,
            2..4
        );
        let mut map = EntryStyleMap::default();
        map.owner(owner);
        assert_eq!(
            map.ranges(
                owner
                    .inline_root(&EntryInlineRoot::Block { index: 0 })
                    .unwrap()
            ),
            [InlineNameRange {
                chars: 2..4,
                kind: EntryKind::Term
            }]
        );
        let other = item.clone();
        assert!(
            map.ranges(
                EntryOwner::List(&other)
                    .inline_root(&EntryInlineRoot::Block { index: 0 })
                    .unwrap()
            )
            .is_empty()
        );
    }

    #[test]
    fn rejects_invalid_unicode_and_whole_name_binding_atomically() {
        let mut item = item();
        assert!(project_content_slice(EntryOwner::List(&item), &slice(Some(1..5))).is_none());
        item.entry.as_mut().unwrap().name_bindings[0].occurrences[0].parts[0].bytes = Some(0..2);
        let owner = EntryOwner::List(&item);
        let mut map = EntryStyleMap::default();
        map.owner(owner);
        assert!(map.roots.is_empty());
    }
}
