//! Decode only the exporter's explicit leading hard-row block spelling.
//!
//! `CommonMark` treats a leading `br` tag as an HTML block and does not parse its
//! following Markdown phrasing. A canonical `<br />` first line opts into the
//! hard-row contract; all other HTML in that block must also be attribute-free
//! line-break tags. Other HTML blocks retain exact source and diagnostics.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::{SpannedEvent, inline::is_html_line_break};

/// Stream ordinary events directly, buffering only the current HTML block.
pub(crate) fn decode_events<'a>(
    events: impl Iterator<Item = SpannedEvent<'a>>,
    options: Options,
) -> impl Iterator<Item = SpannedEvent<'a>> {
    CanonicalEvents {
        events,
        options,
        pending: Vec::new().into_iter(),
    }
}

struct CanonicalEvents<'a, I> {
    events: I,
    options: Options,
    pending: std::vec::IntoIter<SpannedEvent<'a>>,
}

impl<'a, I: Iterator<Item = SpannedEvent<'a>>> Iterator for CanonicalEvents<'a, I> {
    type Item = SpannedEvent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(event) = self.pending.next() {
            return Some(event);
        }
        let first = self.events.next()?;
        if !matches!(first.0, Event::Start(Tag::HtmlBlock)) {
            return Some(first);
        }
        let mut block = vec![first];
        for event in self.events.by_ref() {
            let ended = matches!(event.0, Event::End(TagEnd::HtmlBlock));
            block.push(event);
            if ended {
                break;
            }
        }
        let decoded = matches!(
            block.last().map(|event| &event.0),
            Some(Event::End(TagEnd::HtmlBlock))
        )
        .then(|| decode_block(&block[1..block.len() - 1], self.options))
        .flatten();
        self.pending = decoded.unwrap_or(block).into_iter();
        self.pending.next()
    }
}

fn decode_block(
    events: &[SpannedEvent<'_>],
    options: Options,
) -> Option<Vec<SpannedEvent<'static>>> {
    let (Event::Html(first), _) = events.first()? else {
        return None;
    };
    if first.lines().next()? != "<br />" {
        return None;
    }

    // A temporary ordinary character makes CommonMark parse phrasing instead
    // of an HTML block. It is removed from events before lowering, never
    // written to IR or an artifact. Map every offset back to original spans,
    // including list prefixes omitted by the HTML event's text payload.
    let mut phrasing = String::from("x");
    let mut segments = Vec::with_capacity(events.len());
    for (event, range) in events {
        let Event::Html(raw) = event else {
            return None;
        };
        let start = phrasing.len();
        phrasing.push_str(raw);
        segments.push((start..phrasing.len(), range.clone()));
    }

    let mut parsed = Parser::new_ext(&phrasing, options).into_offset_iter();
    let (Event::Start(Tag::Paragraph), first_range) = parsed.next()? else {
        return None;
    };
    let (Event::Text(marker), _) = parsed.next()? else {
        return None;
    };
    if marker.as_ref() != "x" {
        return None;
    }
    let mut output = vec![(
        Event::Start(Tag::Paragraph),
        map_range(first_range, &segments),
    )];
    let mut ended = false;
    let mut break_tag = false;
    for (event, range) in parsed {
        if ended {
            return None;
        }
        if break_tag && matches!(event, Event::SoftBreak) {
            // The source newline following a br tag is its spelling, not a
            // second executed break or a separator character.
            break_tag = false;
            continue;
        }
        break_tag = false;
        match &event {
            Event::End(TagEnd::Paragraph) => ended = true,
            Event::Html(raw) | Event::InlineHtml(raw) if !is_html_line_break(raw) => {
                return None;
            }
            Event::Start(tag) if !inline_tag(tag) => return None,
            Event::Rule => return None,
            _ => {}
        }
        let event = if matches!(event, Event::Html(_) | Event::InlineHtml(_)) {
            // Validation above accepted only attribute-free br HTML. Emit
            // the same hard-row event for both the reader and visible search.
            break_tag = true;
            Event::HardBreak
        } else {
            event.into_static()
        };
        output.push((event, map_range(range, &segments)));
    }
    ended.then_some(output)
}

fn inline_tag(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Link { .. }
            | Tag::Image { .. }
            | Tag::Superscript
            | Tag::Subscript
    )
}

fn map_range(
    range: std::ops::Range<usize>,
    segments: &[(std::ops::Range<usize>, std::ops::Range<usize>)],
) -> std::ops::Range<usize> {
    let offset = |position: usize, start: bool| {
        let index = segments
            .partition_point(|(parsed, _)| {
                parsed.start < position || (start && parsed.start == position)
            })
            .saturating_sub(1);
        let (parsed, original) = &segments[index];
        original
            .start
            .saturating_add(position.saturating_sub(parsed.start).min(parsed.len()))
            .min(original.end)
    };
    offset(range.start, true)..offset(range.end, false)
}
