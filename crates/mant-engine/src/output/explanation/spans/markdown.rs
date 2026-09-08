//! CommonMark-only emphasis of exact inline matches before escaping/layout.
//! Fenced displays remain verbatim; inserting Markdown inside them would lie.
use super::{LocatedStyles, Span, key, pieces};
use mant_ir::Inline;
use mant_protocol::{ExplanationTextRoot, TextPresentation};

impl LocatedStyles<'_> {
    pub(crate) fn markdown_inline(
        &self,
        nodes: &[Inline],
        options: crate::MarkdownOptions,
    ) -> String {
        let spans = self
            .roots
            .get(&key(ExplanationTextRoot::Inline(nodes)))
            .map_or(&[][..], Vec::as_slice);
        // Borrow the overwhelmingly common unmarked root unchanged. Only one
        // marked root is projected at a time, never the complete response/AST.
        if !spans.iter().any(|s| s.matched) {
            return crate::output::markdown::inline::render_inline(nodes, options);
        }
        let marked = mark(nodes, &mut 0, spans, false);
        crate::output::markdown::inline::render_inline(&marked, options)
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
