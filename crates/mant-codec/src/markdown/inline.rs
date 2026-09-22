//! Lowers supported Markdown spans and preserves unsupported inline source.

use mant_ir::{ContentRootKind, Diagnostic, Inline};
use pulldown_cmark::{Event, LinkType, Tag, TagEnd};

use super::{
    EventCursor,
    content::{InlineRoot, MarkdownContent},
    source::MarkdownSource,
};

pub(super) fn parse_inlines(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    content: &mut MarkdownContent,
    diagnostics: &mut Vec<Diagnostic>,
    end: TagEnd,
    kind: ContentRootKind,
) -> (Vec<Inline>, usize) {
    let mut root = content.root(kind, None);
    parse_inline_sequence(cursor, source, &mut root, diagnostics, Some(end))
}

/// Parse the inline-only event stream emitted for a tight list item.
pub(super) fn parse_inline_run(
    cursor: &mut EventCursor<'_>,
    source: &MarkdownSource<'_>,
    content: &mut MarkdownContent,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<Inline>, usize) {
    let mut root = content.root(ContentRootKind::Body, None);
    parse_inline_sequence(cursor, source, &mut root, diagnostics, None)
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
    root: &mut InlineRoot<'_>,
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
            Event::Text(value) => push_text(
                &mut output,
                root,
                value.into_string(),
                Some(source.span(&range)),
            ),
            Event::Code(value) => {
                output.push(root.code(value.into_string(), Some(source.span(&range))));
            }
            Event::SoftBreak => {
                push_text(&mut output, root, " ".to_owned(), Some(source.span(&range)));
            }
            Event::HardBreak => output.push(root.hard_break(Some(source.span(&range)))),
            Event::Start(tag @ (Tag::Strong | Tag::Emphasis)) if !cursor.try_descend() => {
                let name = unsupported_tag_name(&tag);
                let whole = cursor.consume_balanced(range);
                end_offset = whole.end;
                let raw = source.unsupported_inline(name, whole.clone(), diagnostics);
                push_text(&mut output, root, raw, Some(source.span(&whole)));
            }
            Event::Start(Tag::Strong) => {
                let (children, nested_end) = root.with_strong(|root| {
                    parse_inline_sequence(cursor, source, root, diagnostics, Some(TagEnd::Strong))
                });
                cursor.ascend();
                end_offset = nested_end;
                output.push(Inline::Strong { children });
            }
            Event::Start(Tag::Emphasis) => {
                let (children, nested_end) = root.with_emphasis(|root| {
                    parse_inline_sequence(cursor, source, root, diagnostics, Some(TagEnd::Emphasis))
                });
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
                let destination = dest_url.into_string();
                let title = (!title.is_empty()).then(|| title.into_string());
                let occurrence = root.push_link(
                    mant_ir::LinkTarget::from_uri(&destination),
                    title,
                    Some(source.span(&range)),
                );
                let (children, nested_end) = root.with_link(occurrence, |root| {
                    parse_inline_sequence(cursor, source, root, diagnostics, Some(TagEnd::Link))
                });
                cursor.ascend();
                end_offset = nested_end;
                output.push(Inline::Link {
                    occurrence,
                    children,
                });
            }
            Event::Start(tag) => {
                let name = unsupported_tag_name(&tag);
                let whole = cursor.consume_balanced(range);
                end_offset = whole.end;
                let raw = source.unsupported_inline(name, whole.clone(), diagnostics);
                push_text(&mut output, root, raw, Some(source.span(&whole)));
            }
            Event::InlineMath(_) | Event::DisplayMath(_) => {
                let raw = source.unsupported_inline("math", range.clone(), diagnostics);
                push_text(
                    &mut output,
                    root,
                    unescape_commonmark_punctuation(&raw),
                    Some(source.span(&range)),
                );
            }
            Event::InlineHtml(_)
            | Event::Html(_)
            | Event::FootnoteReference(_)
            | Event::TaskListMarker(_)
            | Event::Rule => {
                let name = unsupported_event_name(&event);
                let raw = source.unsupported_inline(name, range.clone(), diagnostics);
                push_text(&mut output, root, raw, Some(source.span(&range)));
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

fn push_text(
    output: &mut Vec<Inline>,
    root: &mut InlineRoot<'_>,
    value: String,
    source: Option<mant_ir::SourceSpan>,
) {
    if value.is_empty() {
        return;
    }
    if let Some(Inline::Text { content }) = output.last_mut() {
        root.merge_text(content, &value);
    } else {
        output.push(root.text(value, source));
    }
}

pub(super) fn inline_text(content: &MarkdownContent, inlines: &[Inline]) -> String {
    content.inline_text(inlines)
}
