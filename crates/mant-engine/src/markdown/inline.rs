//! Lowers supported Markdown spans and preserves unsupported inline source.

use mant_ir::{Diagnostic, Inline};
use pulldown_cmark::{Event, LinkType, Tag, TagEnd};

use super::{EventCursor, source::MarkdownSource};

pub(super) fn parse_inlines(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    diagnostics: &mut Vec<Diagnostic>,
    end: TagEnd,
) -> (Vec<Inline>, usize) {
    parse_inline_sequence(cursor, source, diagnostics, Some(end))
}

/// Parse the inline-only event stream emitted for a tight list item.
pub(super) fn parse_inline_run(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<Inline>, usize) {
    parse_inline_sequence(cursor, source, diagnostics, None)
}

pub(super) fn starts_inline_run(event: &Event<'_>) -> bool {
    match event {
        Event::Text(_)
        | Event::Code(_)
        | Event::SoftBreak
        | Event::HardBreak
        | Event::InlineHtml(_)
        | Event::Html(_)
        | Event::InlineMath(_)
        | Event::DisplayMath(_)
        | Event::FootnoteReference(_)
        | Event::TaskListMarker(_) => true,
        Event::Start(tag) => matches!(
            tag,
            Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough
                | Tag::Link { .. }
                | Tag::Image { .. }
                | Tag::Superscript
                | Tag::Subscript
        ),
        Event::End(_) | Event::Rule => false,
    }
}

fn parse_inline_sequence(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    diagnostics: &mut Vec<Diagnostic>,
    expected_end: Option<TagEnd>,
) -> (Vec<Inline>, usize) {
    let mut output = Vec::new();
    let mut end_offset = 0;

    while let Some((event, _)) = cursor.peek() {
        if expected_end.is_none() && !starts_inline_run(event) {
            break;
        }
        let (event, range) = cursor.next().expect("peeked event remains available");
        end_offset = range.end;
        match event {
            Event::End(actual) if Some(actual) == expected_end => break,
            Event::End(_) => {}
            Event::Text(value) => push_text(&mut output, value.into_string()),
            Event::Code(value) => output.push(Inline::Code {
                value: value.into_string(),
            }),
            Event::SoftBreak => push_text(&mut output, " ".to_owned()),
            Event::HardBreak => output.push(Inline::LineBreak),
            Event::Start(tag @ (Tag::Strong | Tag::Emphasis)) if !cursor.try_descend() => {
                let name = unsupported_tag_name(&tag);
                let whole = cursor.consume_balanced(range);
                end_offset = whole.end;
                let raw = source.unsupported_inline(name, whole, diagnostics);
                push_text(&mut output, raw);
            }
            Event::Start(Tag::Strong) => {
                let (children, nested_end) =
                    parse_inlines(cursor, source, diagnostics, TagEnd::Strong);
                cursor.ascend();
                end_offset = nested_end;
                output.push(Inline::Strong { children });
            }
            Event::Start(Tag::Emphasis) => {
                let (children, nested_end) =
                    parse_inlines(cursor, source, diagnostics, TagEnd::Emphasis);
                cursor.ascend();
                end_offset = nested_end;
                output.push(Inline::Emphasis { children });
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            }) if supported_link(link_type) && cursor.try_descend() => {
                let (children, nested_end) =
                    parse_inlines(cursor, source, diagnostics, TagEnd::Link);
                cursor.ascend();
                end_offset = nested_end;
                let destination = dest_url.into_string();
                let title = (!title.is_empty()).then(|| title.into_string());
                if let Some(target) = destination.strip_prefix('#') {
                    output.push(Inline::Link {
                        target: mant_ir::LinkTarget::Section { id: target.into() },
                        title,
                        children,
                    });
                } else if let Some(address) = destination.strip_prefix("mailto:") {
                    output.push(Inline::Link {
                        target: mant_ir::LinkTarget::Email {
                            address: address.to_owned(),
                        },
                        title,
                        children,
                    });
                } else if let Some((name, fragment)) = markdown_document_reference(&destination) {
                    output.push(Inline::Link {
                        target: mant_ir::LinkTarget::Document { name, fragment },
                        title,
                        children,
                    });
                } else {
                    output.push(Inline::Link {
                        target: mant_ir::LinkTarget::External { uri: destination },
                        title,
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
            Event::InlineMath(_) | Event::DisplayMath(_) => {
                let raw = source.unsupported_inline("math", range, diagnostics);
                push_text(&mut output, unescape_commonmark_punctuation(&raw));
            }
            Event::InlineHtml(_)
            | Event::Html(_)
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

fn unescape_commonmark_punctuation(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\' && characters.peek().is_some_and(char::is_ascii_punctuation) {
            output.push(characters.next().expect("peeked punctuation remains"));
        } else {
            output.push(character);
        }
    }
    output
}

fn markdown_document_reference(destination: &str) -> Option<(String, Option<String>)> {
    let (path, fragment) = destination
        .split_once('#')
        .map_or((destination, None), |(path, fragment)| {
            (path, (!fragment.is_empty()).then(|| fragment.to_owned()))
        });
    if path.contains(['\\', '?']) || path.starts_with('/') || path.chars().any(char::is_control) {
        return None;
    }
    let physical = std::path::Path::new(path);
    let extension = physical.extension()?.to_str()?;
    if !extension.eq_ignore_ascii_case("md") && !extension.eq_ignore_ascii_case("markdown") {
        return None;
    }
    let filename = physical.file_stem()?.to_str()?;
    if filename.is_empty() {
        return None;
    }
    let parent = physical
        .parent()
        .unwrap_or_else(|| std::path::Path::new(""));
    let logical = parent.join(filename).to_str()?.replace('\\', "/");
    let valid = logical.split('/').all(|component| {
        !component.is_empty()
            && (matches!(component, "." | "..") || !component.chars().any(char::is_control))
    });
    valid.then_some((logical, fragment))
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
        Tag::Link { .. } => "link",
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
            | Inline::Link { children, .. } => output.push_str(&inline_text(children)),
            Inline::Anchor { .. } => {}
            Inline::LineBreak => output.push(' '),
        }
    }
    output.trim().to_owned()
}
