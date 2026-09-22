//! Original content coordinates shared by producers, queries and renderers.
use crate::{ContentContext, EntryContentSlice, EntryInlineRoot, EntryOwner, Inline};
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
///
/// # Panics
///
/// Panics if a retained key or range does not resolve in `content`.
#[must_use]
pub fn project_content_slice(
    content: ContentContext<'_>,
    owner: EntryOwner<'_>,
    slice: &EntryContentSlice,
) -> Option<RootTextRange> {
    content
        .project_content_slice(owner, slice)
        .expect("entry content resolves in its authoritative content store")
}

/// Count original Unicode scalars: wrappers and anchors add no positions,
/// while each authored hard line break occupies one scalar position.
///
/// # Panics
///
/// Panics if a retained key or range does not resolve in `content`, or the
/// scalar count overflows `usize`.
#[must_use]
pub fn inline_scalar_len(content: ContentContext<'_>, nodes: &[Inline]) -> usize {
    content
        .scalar_len(nodes)
        .expect("inline content resolves in its authoritative content store")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ContentFixture;
    use crate::{
        Block, DefinitionItem, DefinitionLayout, LayoutHint, LinkTarget, ListItem, ListItemLayout,
    };

    fn nodes(fixture: &mut ContentFixture) -> Vec<Inline> {
        let text = fixture.text("前");
        let anchor = fixture.anchor("zero");
        let line_break = fixture.hard_break();
        let link = fixture.link_text(
            LinkTarget::External {
                uri: "https://example.com".into(),
            },
            None,
            "é名-param",
            true,
        );
        let Inline::Link {
            occurrence,
            children,
        } = link
        else {
            unreachable!();
        };
        vec![
            text,
            anchor,
            line_break,
            Inline::Link {
                occurrence,
                children: vec![Inline::Strong { children }],
            },
            Inline::Emphasis { children: vec![] },
        ]
    }

    #[test]
    fn nested_source_bytes_project_into_original_scalars_for_both_owners() {
        let mut fixture = ContentFixture::body();
        let nodes = nodes(&mut fixture);
        let store = fixture.finish();
        let content = store.content();
        let item = ListItem {
            blocks: vec![Block::Paragraph {
                children: nodes.clone(),
                layout: LayoutHint::default(),
                source: None,
            }],
            layout: ListItemLayout::default(),
            source: None,
            entry: None,
        };
        let definition = DefinitionItem {
            terms: vec![nodes],
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
                project_content_slice(content, owner, &slice),
                Some(RootTextRange {
                    root: root.clone(),
                    chars: 2..4,
                })
            );
            assert_eq!(
                inline_scalar_len(content, owner.inline_root(&root).unwrap()),
                10
            );
            let whole = EntryContentSlice {
                root,
                path: vec![],
                bytes: None,
            };
            assert_eq!(
                project_content_slice(content, owner, &whole).unwrap().chars,
                0..10
            );
        }
    }

    #[test]
    fn invalid_byte_ranges_and_structural_paths_never_become_coordinates() {
        let mut fixture = ContentFixture::body();
        let nodes = nodes(&mut fixture);
        let store = fixture.finish();
        let content = store.content();
        let definition = DefinitionItem {
            terms: vec![nodes],
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
            assert_eq!(
                project_content_slice(content, owner, &slice),
                None,
                "{slice:?}"
            );
        }
    }

    #[test]
    fn empty_wrappers_anchors_and_hard_breaks_have_distinct_lengths() {
        let mut fixture = ContentFixture::body();
        let anchor = fixture.anchor("target");
        let line_break = fixture.hard_break();
        let text = fixture.text("e\u{301}👩‍💻");
        let store = fixture.finish();
        let content = store.content();
        assert_eq!(inline_scalar_len(content, &[]), 0);
        assert_eq!(
            inline_scalar_len(
                content,
                &[
                    anchor,
                    Inline::Strong { children: vec![] },
                    Inline::Emphasis {
                        children: vec![line_break]
                    },
                    text,
                ]
            ),
            6
        ); // One break + two combining scalars + three emoji scalars.
    }
}
