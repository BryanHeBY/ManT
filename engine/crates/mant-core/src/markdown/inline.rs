//! Lowers supported Markdown spans and preserves unsupported inline source.

use mant_ast::{Diagnostic, Inline};
use pulldown_cmark::{Event, LinkType, Tag, TagEnd};

use super::{EventCursor, source::MarkdownSource};

pub(super) fn parse_inlines(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    diagnostics: &mut Vec<Diagnostic>,
    end: TagEnd,
) -> (Vec<Inline>, usize) {
    let mut output = Vec::new();
    let mut end_offset = source.raw(&(0..0)).len();

    while let Some((event, range)) = cursor.next() {
        end_offset = range.end;
        match event {
            Event::End(actual) if actual == end => break,
            Event::End(_) => {}
            Event::Text(value) => push_text(&mut output, value.into_string()),
            Event::Code(value) => output.push(Inline::Code {
                value: value.into_string(),
            }),
            Event::SoftBreak => push_text(&mut output, " ".to_owned()),
            Event::HardBreak => output.push(Inline::LineBreak),
            Event::Start(Tag::Strong) => {
                let (children, nested_end) =
                    parse_inlines(cursor, source, diagnostics, TagEnd::Strong);
                end_offset = nested_end;
                output.push(Inline::Strong { children });
            }
            Event::Start(Tag::Emphasis) => {
                let (children, nested_end) =
                    parse_inlines(cursor, source, diagnostics, TagEnd::Emphasis);
                end_offset = nested_end;
                output.push(Inline::Emphasis { children });
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            }) if supported_link(link_type) => {
                let (children, nested_end) =
                    parse_inlines(cursor, source, diagnostics, TagEnd::Link);
                end_offset = nested_end;
                let destination = dest_url.into_string();
                if let Some(target) = destination.strip_prefix('#') {
                    output.push(Inline::SectionReference {
                        target: target.to_owned(),
                        children,
                    });
                } else if let Some(address) = destination.strip_prefix("mailto:") {
                    output.push(Inline::EmailLink {
                        address: address.to_owned(),
                        children,
                    });
                } else {
                    output.push(Inline::ExternalLink {
                        uri: destination,
                        title: (!title.is_empty()).then(|| title.into_string()),
                        children,
                    });
                }
            }
            Event::Start(tag) => {
                let name = unsupported_tag_name(&tag);
                let whole = cursor.consume_balanced(range);
                end_offset = whole.end;
                let raw = source.unsupported_inline(name, whole, diagnostics);
                push_text(&mut output, raw);
            }
            Event::InlineHtml(_)
            | Event::Html(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::FootnoteReference(_)
            | Event::TaskListMarker(_)
            | Event::Rule => {
                let name = unsupported_event_name(&event);
                let raw = source.unsupported_inline(name, range, diagnostics);
                push_text(&mut output, raw);
            }
        }
    }

    (output, end_offset)
}

fn supported_link(link_type: LinkType) -> bool {
    !matches!(link_type, LinkType::WikiLink { .. })
}

fn unsupported_tag_name(tag: &Tag<'_>) -> &'static str {
    match tag {
        Tag::Image { .. } => "image",
        Tag::Strikethrough => "strikethrough",
        Tag::Superscript => "superscript",
        Tag::Subscript => "subscript",
        Tag::Link { .. } => "wiki link",
        _ => "inline construct",
    }
}

fn unsupported_event_name(event: &Event<'_>) -> &'static str {
    match event {
        Event::InlineHtml(_) | Event::Html(_) => "HTML",
        Event::InlineMath(_) | Event::DisplayMath(_) => "math",
        Event::FootnoteReference(_) => "footnote reference",
        Event::TaskListMarker(_) => "task marker",
        Event::Rule => "thematic break",
        _ => "inline construct",
    }
}

fn push_text(output: &mut Vec<Inline>, value: String) {
    if value.is_empty() {
        return;
    }
    if let Some(Inline::Text { value: previous }) = output.last_mut() {
        previous.push_str(&value);
    } else {
        output.push(Inline::Text { value });
    }
}

pub(super) fn inline_text(inlines: &[Inline]) -> String {
    let mut output = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => output.push_str(&inline_text(children)),
            Inline::Anchor { .. } => {}
            Inline::LineBreak => output.push(' '),
        }
    }
    output.trim().to_owned()
}
