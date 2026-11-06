//! Original effective definitions, not equal URI/title strings, admit recovery.
mod controls;
mod extensions;
mod matrix;

use mant_ir::{
    Document, Inline, LinkTarget, SourceSpan,
    visit::{Visit, walk_inline},
};
use std::ops::Range;

#[derive(Default)]
struct Reading {
    text: String,
    links: Vec<(LinkTarget, Option<String>, String)>,
}
impl<'a> Visit<'a> for Reading {
    fn visit_inline(&mut self, inline: &'a Inline) {
        match inline {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                self.text.push_str(value);
            }
            Inline::LineBreak {} => self.text.push('\n'),
            Inline::Link {
                target,
                title,
                children,
            } => self.links.push((
                target.clone(),
                title.clone(),
                mant_ir::inline_plain_text(children),
            )),
            Inline::Strong { .. } | Inline::Emphasis { .. } | Inline::Anchor { .. } => {}
        }
        walk_inline(self, inline);
    }
}
fn reading(document: &Document) -> Reading {
    let mut result = Reading::default();
    result.visit_document(document);
    result
}
fn parse(source: &str) -> Document {
    let document = super::parse_markdown(source, None).unwrap().document;
    let restored: Document =
        serde_json::from_str(&serde_json::to_string(&document).unwrap()).unwrap();
    assert_eq!(restored, document, "JSON string round trip: {source:?}");
    document
}
fn findings<'a>(document: &'a Document, code: &str) -> Vec<&'a mant_ir::Diagnostic> {
    document
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some(code))
        .collect()
}
fn decoded_findings(document: &Document) -> Vec<&mant_ir::Diagnostic> {
    findings(document, "markdown.control-characters")
        .into_iter()
        .filter(|diagnostic| diagnostic.message.contains("decoded"))
        .collect()
}
fn span_range(span: SourceSpan) -> Range<usize> {
    let range = span.byte_range.expect("original byte range");
    range.start.get() as usize..range.end.get() as usize
}
fn position(source: &str, end: usize) -> (u32, u32) {
    let bytes = source.as_bytes();
    let mut at = 0;
    let mut line = 1;
    let mut column = 1;
    while at < end {
        match bytes[at] {
            b'\r' => {
                at += usize::from(bytes.get(at + 1) == Some(&b'\n'));
                line += 1;
                column = 1;
            }
            b'\n' => {
                line += 1;
                column = 1;
            }
            _ => column += 1,
        }
        at += 1;
    }
    (line, column)
}
fn assert_span(source: &str, span: SourceSpan, expected: Range<usize>) {
    assert_eq!(span_range(span), expected);
    assert!(source.get(expected.clone()).is_some());
    assert_eq!((span.line, span.column), position(source, expected.start));
    assert_eq!((span.end_line, span.end_column), {
        let (line, column) = position(source, expected.end);
        (Some(line), Some(column))
    });
}
fn assert_two_reads(document: &Document, text: &str, links: usize) {
    let generated_heading = document.heading.is_none();
    for options in [
        crate::encode::MarkdownOptions::default(),
        crate::encode::MarkdownOptions {
            preserve_anchors: false,
            preserve_semantics: true,
        },
    ] {
        let mut current = document.clone();
        for _ in 0..2 {
            let query = mant_ir::ResolvedContent {
                label: "reference binding".into(),
                address: None,
                document: Some(current),
                tldr: None,
            };
            // Addressable bytes intentionally include optional raw-HTML
            // destinations; compare their shared encoder without treating
            // unsupported HTML source as a portable semantic round trip.
            let addressable = crate::encode::MarkdownOptions::ADDRESSABLE;
            assert_eq!(
                crate::encode::render_addressable_markdown_with_options(&query, addressable).text(),
                crate::encode::render_markdown_with_options(&query, addressable)
            );
            let artifact = crate::encode::render_addressable_markdown_with_options(&query, options);
            let markdown = crate::encode::render_markdown_with_options(&query, options);
            assert_eq!(artifact.text(), markdown);
            current = parse(&markdown);
            if generated_heading {
                // Complete exports give a heading-less input the query label.
                // Verify that known wrapper separately, then compare only the
                // original authored roots; never remove an authored heading.
                let heading = current.heading.take().expect("generated query heading");
                assert_eq!(heading.plain_text(), "reference binding");
            }
            let actual = reading(&current);
            assert_eq!(actual.text, text, "{markdown:?}");
            assert_eq!(actual.links.len(), links, "{markdown:?}");
            // Source/diagnostic positions refer to this round's Markdown.
            for diagnostic in &current.diagnostics {
                if let Some(span) = diagnostic.source {
                    let range = span_range(span);
                    assert_span(&markdown, span, range);
                }
            }
        }
    }
}
