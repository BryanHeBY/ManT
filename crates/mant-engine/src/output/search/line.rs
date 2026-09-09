//! Map one canonical Markdown line and its original match ranges into visible text.
use mant_codec::markdown_mapping::{InlineMappingKind, map_inline_characters};
use pulldown_cmark::{Event, Parser};
use std::ops::Range;

pub(super) fn render_search_line(
    markdown: &str,
    highlights: &[Range<usize>],
) -> (String, Vec<Range<usize>>) {
    let mut rendered = String::with_capacity(markdown.len());
    let mut rendered_highlights = Vec::new();
    for (event, source) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Text(value) | Event::InlineMath(value) | Event::DisplayMath(value) => {
                append_mapped_text(
                    markdown,
                    &value,
                    source,
                    InlineMappingKind::Text,
                    highlights,
                    &mut rendered,
                    &mut rendered_highlights,
                );
            }
            Event::Code(value) => append_mapped_text(
                markdown,
                &value,
                source,
                InlineMappingKind::Code,
                highlights,
                &mut rendered,
                &mut rendered_highlights,
            ),
            Event::SoftBreak | Event::HardBreak => {
                let start = rendered.len();
                rendered.push(' ');
                if highlights
                    .iter()
                    .any(|range| ranges_overlap(range, &source))
                {
                    rendered_highlights.push(start..rendered.len());
                }
            }
            Event::TaskListMarker(checked) => {
                rendered.push_str(if checked { "[x] " } else { "[ ] " });
            }
            Event::Rule => rendered.push_str("---"),
            Event::Start(_)
            | Event::End(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_) => {}
        }
    }
    let visible_end = rendered.trim_end().len();
    rendered.truncate(visible_end);
    rendered_highlights.retain(|range| range.start < visible_end);
    for range in &mut rendered_highlights {
        range.end = range.end.min(visible_end);
    }
    (rendered, rendered_highlights)
}

fn append_mapped_text(
    markdown: &str,
    value: &str,
    source: Range<usize>,
    kind: InlineMappingKind,
    highlights: &[Range<usize>],
    rendered: &mut String,
    rendered_highlights: &mut Vec<Range<usize>>,
) {
    for character in map_inline_characters(markdown, value, source, kind) {
        let visible_start = rendered.len();
        rendered.push(character.value);
        if highlights
            .iter()
            .any(|range| ranges_overlap(range, &character.source))
        {
            rendered_highlights.push(visible_start..rendered.len());
        }
    }
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}
