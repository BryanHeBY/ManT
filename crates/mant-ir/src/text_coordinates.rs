//! Original content coordinates shared by producers, queries and renderers.
use crate::{EntryContentSlice, EntryInlineRoot, EntryOwner, Inline};
use std::ops::Range;

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
        start += inline_scalar_len(nodes.get(..index)?);
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
        inline_scalar_len(nodes)
    };
    Some(RootTextRange {
        root: slice.root.clone(),
        chars: start..start + length,
    })
}

/// Count original Unicode scalars: wrappers and anchors add no positions,
/// while each authored hard line break occupies one scalar position.
#[must_use]
pub fn inline_scalar_len(nodes: &[Inline]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Inline::Text { value } | Inline::Code { value } => value.chars().count(),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => inline_scalar_len(children),
            Inline::LineBreak => 1,
            Inline::Anchor { .. } => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Block, DefinitionItem, DefinitionLayout, LayoutHint, LinkTarget, ListItem, ListItemLayout,
    };

    fn nodes() -> Vec<Inline> {
        vec![
            Inline::Text {
                value: "前".into()
            },
            Inline::Anchor {
                id: "zero".into(),
                fragment_aliases: vec![],
                owner_source: None,
            },
            Inline::LineBreak,
            Inline::Link {
                target: LinkTarget::External {
                    uri: "https://example.com".into(),
                },
                title: None,
                children: vec![Inline::Strong {
                    children: vec![Inline::Code {
                        value: "é名-param".into(),
                    }],
                }],
            },
            Inline::Emphasis { children: vec![] },
        ]
    }

    #[test]
    fn nested_source_bytes_project_into_original_scalars_for_both_owners() {
        let item = ListItem {
            blocks: vec![Block::Paragraph {
                children: nodes(),
                layout: LayoutHint::default(),
                source: None,
            }],
            layout: ListItemLayout::default(),
            source: None,
            entry: None,
        };
        let definition = DefinitionItem {
            terms: vec![nodes()],
            description: vec![],
            layout: DefinitionLayout::default(),
            source: None,
            entry: None,
        };
        for (owner, root) in [
            (EntryOwner::List(&item), EntryInlineRoot::Block { index: 0 }),
            (
                EntryOwner::Definition(&definition),
                EntryInlineRoot::Term { index: 0 },
            ),
        ] {
            let slice = EntryContentSlice {
                root: root.clone(),
                path: vec![3, 0, 0],
                bytes: Some(0..5),
            };
            assert_eq!(
                project_content_slice(owner, &slice),
                Some(RootTextRange {
                    root: root.clone(),
                    chars: 2..4,
                })
            );
            assert_eq!(inline_scalar_len(owner.inline_root(&root).unwrap()), 10);
            let whole = EntryContentSlice {
                root,
                path: vec![],
                bytes: None,
            };
            assert_eq!(project_content_slice(owner, &whole).unwrap().chars, 0..10);
        }
    }

    #[test]
    fn invalid_byte_ranges_and_structural_paths_never_become_coordinates() {
        let definition = DefinitionItem {
            terms: vec![nodes()],
            description: vec![],
            layout: DefinitionLayout::default(),
            source: None,
            entry: None,
        };
        let owner = EntryOwner::Definition(&definition);
        for (path, bytes) in [
            (vec![3, 0, 0], Some(1..5)), // Split the initial UTF-8 scalar.
            (vec![3, 0, 0], Some(0..3)), // Split the following scalar.
            (vec![3, 0, 0], Some(0..100)),
            (vec![3, 0, 0], Some(2..2)),
            (vec![3], Some(0..1)), // A wrapper is not a byte-addressable leaf.
            (vec![3, 0, 0, 0], None),
            (vec![5], None),
            (vec![], Some(0..1)),
        ] {
            let slice = EntryContentSlice {
                root: EntryInlineRoot::Term { index: 0 },
                path,
                bytes,
            };
            assert_eq!(project_content_slice(owner, &slice), None, "{slice:?}");
        }
    }

    #[test]
    fn empty_wrappers_anchors_and_hard_breaks_have_distinct_lengths() {
        assert_eq!(inline_scalar_len(&[]), 0);
        assert_eq!(
            inline_scalar_len(&[
                Inline::Anchor {
                    id: "target".into(),
                    fragment_aliases: vec![],
                    owner_source: None
                },
                Inline::Strong { children: vec![] },
                Inline::Emphasis {
                    children: vec![Inline::LineBreak]
                },
                Inline::Text {
                    value: "e\u{301}👩‍💻".into()
                },
            ]),
            6
        ); // One break + two combining scalars + three emoji scalars.
    }
}
