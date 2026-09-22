//! CommonMark-only emphasis of exact inline matches before escaping/layout.
//! Fenced displays remain verbatim; inserting Markdown inside them would lie.
use super::{LocatedStyles, key};
use mant_codec::encode::MarkdownInlineProjection;
use mant_ir::Inline;
use mant_protocol::ExplanationTextRoot;
use std::ops::Range;

impl LocatedStyles<'_> {
    pub(in crate::output) fn markdown_inline(
        &self,
        nodes: &[Inline],
        options: mant_codec::encode::MarkdownFragmentOptions,
    ) -> String {
        mant_codec::encode::render_projected_inline_fragment(
            self.content()
                .expect("retained explanation form has a content projection"),
            nodes,
            options,
            self,
        )
    }
}

impl MarkdownInlineProjection for LocatedStyles<'_> {
    fn scalar_ranges(&self, nodes: &[Inline]) -> &[Range<usize>] {
        self.markdown_matches
            .get(&key(ExplanationTextRoot::Inline(nodes)))
            .map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, marker::PhantomData};

    fn projected_text(value: &str) -> (mant_ir::ContentProjection, Vec<Inline>) {
        let mut builder = mant_ir::ContentStoreBuilder::new();
        let owner = builder.push_owner(
            mant_ir::ContentOwnerKind::Document,
            mant_ir::Provenance::Unknown,
        );
        let root = builder.push_root(
            owner,
            mant_ir::ContentRootKind::Body,
            mant_ir::Provenance::Unknown,
        );
        let content = builder.push_text(
            root,
            value.to_owned(),
            None,
            mant_ir::ContentStyle::default(),
            None,
            None,
            mant_ir::Provenance::Unknown,
        );
        (
            mant_ir::ContentProjection {
                content_store: builder.finish(),
            },
            vec![Inline::Text { content }],
        )
    }

    fn styles<'a>(
        projection: &'a mant_ir::ContentProjection,
        nodes: &'a [Inline],
        matched: bool,
    ) -> LocatedStyles<'a> {
        let root = key(ExplanationTextRoot::Inline(nodes));
        let mut roots = BTreeMap::new();
        roots.insert(
            root,
            vec![super::super::Span {
                chars: 1..4,
                kind: None,
                matched,
            }],
        );
        let mut markdown_matches = BTreeMap::new();
        if matched {
            markdown_matches.insert(root, std::iter::once(1..4).collect());
        }
        LocatedStyles {
            roots,
            markdown_matches,
            names: None,
            content: Some(projection.content()),
            lifetime: PhantomData,
        }
    }

    #[test]
    fn unmarked_roots_have_no_projection_ranges() {
        let (projection, nodes) = projected_text("ALPHA");
        let styles = styles(&projection, &nodes, false);
        assert!(styles.scalar_ranges(&nodes).is_empty());
    }

    #[test]
    fn projection_marks_only_the_borrowed_root_and_preserves_source() {
        let (projection, nodes) = projected_text("ALPHA");
        let other = nodes.clone();
        let styles = styles(&projection, &nodes, true);
        assert_eq!(styles.scalar_ranges(&nodes), std::slice::from_ref(&(1..4)));
        assert!(styles.scalar_ranges(&other).is_empty());
        let options = mant_codec::encode::MarkdownFragmentOptions::default();
        assert_eq!(styles.markdown_inline(&nodes, options), "A**LPH**A");
        assert_eq!(
            mant_codec::encode::render_inline_fragment(projection.content(), &nodes, options),
            "ALPHA"
        );
    }

    #[test]
    fn block_report_decoration_does_not_change_canonical_document_encoding() {
        let (projection, nodes) = projected_text("ALPHA");
        let blocks = [mant_ir::Block::Paragraph {
            children: nodes,
            layout: mant_ir::LayoutHint::default(),
            source: None,
        }];
        let mant_ir::Block::Paragraph { children, .. } = &blocks[0] else {
            unreachable!();
        };
        let styles = styles(&projection, children, true);
        let options = mant_codec::encode::MarkdownFragmentOptions::default();
        assert_eq!(
            mant_codec::encode::render_located_blocks_fragment(
                projection.content(),
                &blocks,
                options,
                Some(&styles),
            ),
            ["A**LPH**A"]
        );
        assert_eq!(
            mant_codec::encode::render_blocks_fragment(projection.content(), &blocks, options),
            ["ALPHA"]
        );
    }
}
