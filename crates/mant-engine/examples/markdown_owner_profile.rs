//! Supply validated wire observations for the owner-content comparator.

use std::io::{self, Write};

use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{Block, Document, Inline, ResolvedContent, validate_document};
use mant_protocol::QueryBundle;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy)]
enum Boundary {
    Paragraphs,
    SectionHeadingBody,
    RootHeadingBody,
    ParentBodyChildHeading,
}

impl Boundary {
    const ALL: [Self; 4] = [
        Self::Paragraphs,
        Self::SectionHeadingBody,
        Self::RootHeadingBody,
        Self::ParentBodyChildHeading,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Paragraphs => "paragraphs",
            Self::SectionHeadingBody => "section-heading-body",
            Self::RootHeadingBody => "root-heading-body",
            Self::ParentBodyChildHeading => "parent-body-child-heading",
        }
    }

    fn markdown(self, left: &str, right: &str) -> String {
        match self {
            Self::Paragraphs => format!("# Catalog\n\n## Body\n\n{left}\n\n{right}\n"),
            Self::SectionHeadingBody => format!("# Catalog\n\n## {left}\n\n{right}\n"),
            Self::RootHeadingBody => format!("# {left}\n\n{right}\n"),
            Self::ParentBodyChildHeading => {
                format!("# Catalog\n\n## Parent\n\n{left}\n\n### {right}\n\nAFTER\n")
            }
        }
    }

    fn adjacent_content(self, document: &mut Document) -> (&mut [Inline], &mut [Inline]) {
        match self {
            Self::Paragraphs => {
                let (left, right) = document.sections[0].blocks.split_at_mut(1);
                (paragraph(&mut left[0]), paragraph(&mut right[0]))
            }
            Self::SectionHeadingBody => {
                let section = &mut document.sections[0];
                (
                    &mut section.heading.content,
                    paragraph(&mut section.blocks[0]),
                )
            }
            Self::RootHeadingBody => (
                &mut document.heading.as_mut().expect("document heading").content,
                paragraph(&mut document.blocks[0]),
            ),
            Self::ParentBodyChildHeading => {
                let parent = &mut document.sections[0];
                (
                    paragraph(&mut parent.blocks[0]),
                    &mut parent.children[0].heading.content,
                )
            }
        }
    }
}

fn paragraph(block: &mut Block) -> &mut [Inline] {
    let Block::Paragraph { children, .. } = block else {
        panic!("expected a paragraph in the bounded mutation source");
    };
    children
}

fn text(content: &mut [Inline]) -> &mut String {
    let [Inline::Text { value }] = content else {
        panic!("expected one nonempty text run in each mutation owner");
    };
    value
}

fn transfer(boundary: Boundary, document: &mut Document, glyph: &str, left_to_right: bool) {
    let (left, right) = boundary.adjacent_content(document);
    let (left, right) = (text(left), text(right));
    // Move the whole chosen grapheme. The source loader supplies both
    // original owners, and the mutation does not modify layout or topology.
    if left_to_right {
        assert!(left.ends_with(glyph));
        left.truncate(left.len() - glyph.len());
        right.insert_str(0, glyph);
    } else {
        assert!(right.starts_with(glyph));
        right.drain(..glyph.len());
        left.push_str(glyph);
    }
    assert!(!left.is_empty() && !right.is_empty());
}

fn observe(content: &ResolvedContent, markdown: Option<&str>) -> Result<(ResolvedContent, Value)> {
    assert_eq!(
        validate_document(content.document.as_ref().expect("source document")),
        []
    );
    let encoded = serde_json::to_string(&QueryBundle::from(content))?;
    let bundle: QueryBundle = serde_json::from_str(&encoded)?;
    assert_eq!(serde_json::to_string(&bundle)?, encoded);
    let transported: ResolvedContent = bundle.into();
    assert_eq!(
        validate_document(transported.document.as_ref().expect("transported document")),
        []
    );
    let result = json!({
        "json": encoded,
        "plain": mant_render::render_query_text(&transported),
        "markdown": markdown,
    });
    Ok((transported, result))
}

fn chain(content: &ResolvedContent) -> Result<Value> {
    let (transported, direct) = observe(content, None)?;
    let mut stages = serde_json::Map::from_iter([("roff".to_owned(), direct)]);
    for (options, routes) in [
        (MarkdownOptions::default(), ["default-1", "default-2"]),
        (
            MarkdownOptions::ADDRESSABLE,
            ["addressable-1", "addressable-2"],
        ),
    ] {
        let mut current = transported.clone();
        for route in routes {
            let markdown = render_markdown_with_options(&current, options);
            let parsed = mant_loader::load_markdown_text(&markdown, None)?;
            let (transported, record) = observe(&parsed, Some(&markdown))?;
            stages.insert(route.to_owned(), record);
            current = transported;
        }
    }
    Ok(Value::Object(stages))
}

fn main() -> Result<()> {
    let mut output = io::BufWriter::new(io::stdout().lock());
    for boundary in Boundary::ALL {
        for left_to_right in [true, false] {
            let direction = if left_to_right {
                "left-to-right"
            } else {
                "right-to-left"
            };
            for (payload, glyph) in [("ascii", "X"), ("cjk", "界"), ("combining", "e\u{301}")] {
                let (left, right) = if left_to_right {
                    (format!("LEFT{glyph}"), "RIGHT".to_owned())
                } else {
                    ("LEFT".to_owned(), format!("{glyph}RIGHT"))
                };
                let markdown = boundary.markdown(&left, &right);
                let original = mant_loader::load_markdown_text(&markdown, None)?;
                let mut changed = original.clone();
                transfer(
                    boundary,
                    changed.document.as_mut().expect("parsed document"),
                    glyph,
                    left_to_right,
                );
                let record = json!({
                    "boundary": boundary.name(), "direction": direction, "payload": payload,
                    "source": markdown, "control": chain(&original)?, "mutant": chain(&changed)?,
                });
                writeln!(output, "{record}")?;
            }
        }
    }
    output.flush()?;
    Ok(())
}
