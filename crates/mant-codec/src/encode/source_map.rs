//! Forward projections from authoritative logical roots into rendered Markdown.

use std::ops::Range;

use mant_ir::ContentRootKey;
use pulldown_cmark::{Event, Parser};

use crate::markdown_mapping::{InlineMappingKind, map_inline_characters};

#[derive(Debug, Clone)]
pub(super) struct RenderedRootRange {
    pub(super) root: ContentRootKey,
    pub(super) markdown: Range<usize>,
}

/// Project one root-relative logical byte range into canonical Markdown bytes.
///
/// The encoder records only ranges it actually rendered.  A placement is
/// returned only when parsing that range reproduces the complete canonical
/// logical root exactly; display overrides and other non-bijective
/// presentations therefore degrade to no Markdown projection rather than a
/// guessed coordinate.
pub(super) fn project(
    markdown: &str,
    rendered: &[RenderedRootRange],
    root: ContentRootKey,
    logical_text: &str,
    logical: Range<usize>,
) -> Vec<Range<usize>> {
    if logical.start >= logical.end || logical_text.get(logical.clone()).is_none() {
        return Vec::new();
    }

    rendered
        .iter()
        .filter(|candidate| candidate.root == root)
        .flat_map(|candidate| {
            project_placement(
                markdown,
                candidate.markdown.clone(),
                logical_text,
                logical.clone(),
            )
        })
        .collect()
}

fn project_placement(
    markdown: &str,
    placement: Range<usize>,
    logical_text: &str,
    logical: Range<usize>,
) -> Vec<Range<usize>> {
    let Some(fragment) = markdown.get(placement.clone()) else {
        return Vec::new();
    };
    let mut visible = String::new();
    let mut characters = Vec::new();
    for (event, source) in Parser::new(fragment).into_offset_iter() {
        let source = placement.start + source.start..placement.start + source.end;
        match event {
            Event::Text(value) => push_mapped(
                markdown,
                &value,
                include_markdown_escape(markdown, placement.start, source, &value),
                InlineMappingKind::Text,
                &mut visible,
                &mut characters,
            ),
            Event::Code(value) | Event::InlineMath(value) | Event::DisplayMath(value) => {
                push_mapped(
                    markdown,
                    &value,
                    source,
                    InlineMappingKind::Code,
                    &mut visible,
                    &mut characters,
                );
            }
            Event::SoftBreak | Event::HardBreak => {
                let start = visible.len();
                visible.push('\n');
                characters.push((start..visible.len(), source));
            }
            Event::Start(_)
            | Event::End(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::TaskListMarker(_)
            | Event::Rule => {}
        }
    }
    if visible != logical_text {
        return Vec::new();
    }

    let mut projected = Vec::<Range<usize>>::new();
    for (visible, markdown) in characters {
        if visible.end <= logical.start || logical.end <= visible.start {
            continue;
        }
        if let Some(previous) = projected.last_mut()
            && markdown.start <= previous.end
        {
            previous.end = previous.end.max(markdown.end);
        } else {
            projected.push(markdown);
        }
    }
    projected
}

/// pulldown-cmark may report an escaped punctuation event beginning after
/// its backslash. That backslash still belongs to the one displayed scalar.
fn include_markdown_escape(
    markdown: &str,
    placement_start: usize,
    source: Range<usize>,
    value: &str,
) -> Range<usize> {
    let Some(first) = value.chars().next() else {
        return source;
    };
    if !first.is_ascii_punctuation()
        || source.start <= placement_start
        || markdown.as_bytes().get(source.start).copied() != Some(first as u8)
    {
        return source;
    }
    let preceding = markdown.as_bytes()[placement_start..source.start]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count();
    if preceding % 2 == 1 {
        source.start - 1..source.end
    } else {
        source
    }
}

fn push_mapped(
    markdown: &str,
    value: &str,
    source: Range<usize>,
    kind: InlineMappingKind,
    visible: &mut String,
    characters: &mut Vec<(Range<usize>, Range<usize>)>,
) {
    for character in map_inline_characters(markdown, value, source, kind) {
        let start = visible.len();
        visible.push(character.value);
        characters.push((start..visible.len(), character.source));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_logical_ranges_through_style_delimiters() {
        let markdown = "**alpha**";
        let rendered = [RenderedRootRange {
            root: ContentRootKey::FIRST,
            markdown: 0..markdown.len(),
        }];
        assert_eq!(
            project(markdown, &rendered, ContentRootKey::FIRST, "alpha", 1..4,).as_slice(),
            std::slice::from_ref(&(3..6))
        );
    }

    #[test]
    fn escaped_character_projection_includes_its_markdown_escape() {
        let markdown = "a\\*b";
        let rendered = [RenderedRootRange {
            root: ContentRootKey::FIRST,
            markdown: 0..markdown.len(),
        }];
        assert_eq!(
            project(markdown, &rendered, ContentRootKey::FIRST, "a*b", 1..2,).as_slice(),
            std::slice::from_ref(&(1..3))
        );
    }

    #[test]
    fn mismatched_display_never_guesses_a_projection() {
        let rendered = [RenderedRootRange {
            root: ContentRootKey::FIRST,
            markdown: 0..7,
        }];
        assert!(project("visible", &rendered, ContentRootKey::FIRST, "logical", 0..7,).is_empty());
    }
}
