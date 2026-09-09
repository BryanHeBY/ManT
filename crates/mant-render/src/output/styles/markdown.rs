//! CommonMark-only emphasis of exact inline matches before escaping/layout.
//! Fenced displays remain verbatim; inserting Markdown inside them would lie.
use super::{LocatedStyles, Span, key, pieces};
use crate::presentation::TextPresentation;
use mant_codec::encode::MarkdownInlineProjection;
use mant_ir::Inline;
use mant_protocol::ExplanationTextRoot;
use std::borrow::Cow;

impl LocatedStyles<'_> {
    pub(in crate::output) fn markdown_inline(
        &self,
        nodes: &[Inline],
        options: mant_codec::encode::MarkdownFragmentOptions,
    ) -> String {
        mant_codec::encode::render_inline_fragment(&self.project(nodes), options)
    }
}

impl MarkdownInlineProjection for LocatedStyles<'_> {
    fn project<'a>(&self, nodes: &'a [Inline]) -> Cow<'a, [Inline]> {
        let spans = self
            .roots
            .get(&key(ExplanationTextRoot::Inline(nodes)))
            .map_or(&[][..], Vec::as_slice);
        // Borrow the overwhelmingly common unmarked root unchanged. Only one
        // marked root is projected at a time, never the complete response/AST.
        if !spans.iter().any(|s| s.matched) {
            return Cow::Borrowed(nodes);
        }
        Cow::Owned(mark(nodes, &mut 0, spans, false))
    }
}

fn mark(nodes: &[Inline], cursor: &mut usize, spans: &[Span], strong: bool) -> Vec<Inline> {
    let mut output = Vec::new();
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => {
                pieces(
                    value,
                    cursor,
                    spans,
                    TextPresentation::default(),
                    &mut |style, text| {
                        let leaf = if matches!(node, Inline::Code { .. }) {
                            Inline::Code { value: text.into() }
                        } else {
                            Inline::Text { value: text.into() }
                        };
                        output.push(if style.matched && !strong {
                            Inline::Strong {
                                children: vec![leaf],
                            }
                        } else {
                            leaf
                        });
                    },
                );
            }
            Inline::Strong { children } => output.push(Inline::Strong {
                children: mark(children, cursor, spans, true),
            }),
            Inline::Emphasis { children } => output.push(Inline::Emphasis {
                children: mark(children, cursor, spans, strong),
            }),
            Inline::Link {
                children,
                target,
                title,
            } => output.push(Inline::Link {
                children: mark(children, cursor, spans, strong),
                target: target.clone(),
                title: title.clone(),
            }),
            Inline::LineBreak => {
                *cursor += 1;
                output.push(Inline::LineBreak);
            }
            Inline::Anchor { .. } => output.push(node.clone()),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_borrows_unmarked_roots_without_copying_inline_content() {
        let nodes = [Inline::Text {
            value: "ALPHA".into(),
        }];
        let styles = LocatedStyles::default();
        let Cow::Borrowed(projected) = styles.project(&nodes) else {
            panic!("unmarked inline roots must stay borrowed");
        };
        assert!(std::ptr::eq(projected.as_ptr(), nodes.as_ptr()));
    }

    #[test]
    fn projection_marks_only_the_borrowed_root_and_preserves_source() {
        let nodes = [Inline::Text {
            value: "ALPHA".into(),
        }];
        let other = nodes.clone();
        let mut styles = LocatedStyles::default();
        styles.roots.insert(
            key(ExplanationTextRoot::Inline(&nodes)),
            vec![Span {
                chars: 1..4,
                kind: None,
                matched: true,
            }],
        );
        assert!(matches!(styles.project(&nodes), Cow::Owned(_)));
        assert!(matches!(styles.project(&other), Cow::Borrowed(_)));
        let options = mant_codec::encode::MarkdownFragmentOptions::default();
        assert_eq!(styles.markdown_inline(&nodes, options), "A**LPH**A");
        assert_eq!(styles.markdown_inline(&other, options), "ALPHA");
        assert_eq!(
            mant_codec::encode::render_inline_fragment(&nodes, options),
            "ALPHA"
        );
    }

    #[test]
    fn block_report_decoration_does_not_change_canonical_document_encoding() {
        let blocks = [mant_ir::Block::Paragraph {
            children: vec![Inline::Text {
                value: "ALPHA".into(),
            }],
            layout: mant_ir::LayoutHint::default(),
            source: None,
        }];
        let mant_ir::Block::Paragraph { children, .. } = &blocks[0] else {
            unreachable!();
        };
        let mut styles = LocatedStyles::default();
        styles.roots.insert(
            key(ExplanationTextRoot::Inline(children)),
            vec![Span {
                chars: 1..4,
                kind: None,
                matched: true,
            }],
        );
        let options = mant_codec::encode::MarkdownFragmentOptions::default();
        assert_eq!(
            mant_codec::encode::render_located_blocks_fragment(&blocks, options, Some(&styles)),
            ["A**LPH**A"]
        );
        assert_eq!(
            mant_codec::encode::render_blocks_fragment(&blocks, options),
            ["ALPHA"]
        );
    }
}
