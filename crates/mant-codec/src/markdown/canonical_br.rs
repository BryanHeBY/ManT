//! Decode only the exporter's explicit leading hard-row block spelling.
//!
//! `CommonMark` treats a leading `br` tag as an HTML block and does not parse its
//! following Markdown phrasing. A canonical `<br />` first line opts into the
//! hard-row contract; all other HTML in that block must also be attribute-free
//! line-break tags. Other HTML blocks retain exact source and diagnostics.

use pulldown_cmark::{
    BrokenLink, Event, LinkType, OffsetIter, Options, Parser, RefDefs, Tag, TagEnd,
};

use super::{SpannedEvent, inline::is_html_line_break};

#[cfg(test)]
mod tests;

/// Stream ordinary events and retain their full-document reference context.
pub(crate) fn parse_events(
    source: &str,
    options: Options,
) -> impl Iterator<Item = SpannedEvent<'_>> {
    // Match CommonMark's bare-CR line endings without changing byte offsets.
    // The upstream HTML scanner otherwise includes later lines in comments.
    // Only this owned normalization path needs to collect before returning.
    if super::source::physical_lines(source).any(|line| line.ends_with('\r')) {
        let normalized = super::source::physical_lines(source)
            .map(|line| {
                line.strip_suffix('\r')
                    .map_or_else(|| line.to_owned(), |body| format!("{body}\n"))
            })
            .collect::<String>();
        SourceEvents::Normalized(
            CanonicalEvents::new(&normalized, options)
                .map(|(event, range)| (event.into_static(), range))
                .collect::<Vec<_>>()
                .into_iter(),
        )
    } else {
        SourceEvents::Borrowed(Box::new(CanonicalEvents::new(source, options)))
    }
}

enum SourceEvents<'a> {
    Borrowed(Box<CanonicalEvents<'a>>),
    Normalized(std::vec::IntoIter<SpannedEvent<'static>>),
}

impl<'a> Iterator for SourceEvents<'a> {
    type Item = SpannedEvent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Borrowed(events) => events.next(),
            Self::Normalized(events) => events.next(),
        }
    }
}

struct CanonicalEvents<'a> {
    source: &'a str,
    events: OffsetIter<'a>,
    options: Options,
    pending: std::vec::IntoIter<SpannedEvent<'a>>,
    reference_fuel: usize,
}

impl<'a> CanonicalEvents<'a> {
    fn new(source: &'a str, options: Options) -> Self {
        Self {
            source,
            events: Parser::new_ext(source, options).into_offset_iter(),
            options,
            pending: Vec::new().into_iter(),
            // parse.rs::fetch_link_type_url_title bounds destination/title
            // expansion this way. Local parsers have their own fuel; also
            // share one quota across all canonical blocks so a long global
            // definition cannot expand independently in every short block.
            // Spent copying work is not refunded when HTML validation fails.
            reference_fuel: source.len().max(100_000),
        }
    }
}

impl<'a> Iterator for CanonicalEvents<'a> {
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
        .then(|| {
            decode_block(
                &block[1..block.len() - 1],
                self.options,
                self.source,
                self.events.reference_definitions(),
                &mut self.reference_fuel,
            )
        })
        .flatten();
        self.pending = decoded.unwrap_or(block).into_iter();
        self.pending.next()
    }
}

fn decode_block(
    events: &[SpannedEvent<'_>],
    options: Options,
    source: &str,
    references: &RefDefs<'_>,
    reference_fuel: &mut usize,
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
        // firstpass.rs::append_html_line emits LF separately and omits its
        // preceding CR. Recover that authored byte so multi-line reference
        // labels keep the same newline range as the ordinary paragraph parser.
        let mut original = range.clone();
        let raw = if raw.as_ref() == "\n"
            && source.get(original.start.saturating_sub(1)..original.start) == Some("\r")
        {
            original.start -= 1;
            &source[original.clone()]
        } else {
            raw.as_ref()
        };
        let start = phrasing.len();
        phrasing.push_str(raw);
        segments.push((start..phrasing.len(), original));
    }

    // First-pass RefDefs preserves definition order and Unicode case folding;
    // BrokenLink.reference already has the parser's whitespace normalization.
    // Resolve against that original table, never reparse or append definitions.
    let resolve = |link: BrokenLink<'_>| {
        if *reference_fuel == 0 {
            return None;
        }
        let definition = references.get(&link.reference)?;
        let title = definition.title.as_ref();
        let cost = definition
            .dest
            .len()
            .saturating_add(title.map_or(0, |title| title.len()));
        *reference_fuel = reference_fuel.saturating_sub(cost);
        Some((
            definition.dest.clone().into_static(),
            title.cloned().unwrap_or_else(|| "".into()).into_static(),
        ))
    };
    let mut parsed =
        Parser::new_with_broken_link_callback(&phrasing, options, Some(resolve)).into_offset_iter();
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
            document_reference(event).into_static()
        };
        output.push((event, map_range(range, &segments)));
    }
    ended.then_some(output)
}

/// Callback resolutions use the same document's definitions, not an external
/// fallback. Restore the syntax types that upstream marks Unknown for callbacks.
fn document_reference(mut event: Event<'_>) -> Event<'_> {
    if let Event::Start(Tag::Link { link_type, .. } | Tag::Image { link_type, .. }) = &mut event {
        *link_type = match *link_type {
            LinkType::ReferenceUnknown => LinkType::Reference,
            LinkType::CollapsedUnknown => LinkType::Collapsed,
            LinkType::ShortcutUnknown => LinkType::Shortcut,
            other => other,
        };
    }
    event
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
