//! Compare accepted local phrasing with one actual parse of the whole sequence.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::serialization::Framing;

#[derive(Clone, Copy)]
pub(super) enum Style {
    Strong,
    Emphasis,
}

impl Style {
    pub(super) fn from_marker(marker: &str) -> Self {
        if marker.len() == 2 {
            Self::Strong
        } else {
            Self::Emphasis
        }
    }

    const fn bit(self) -> u8 {
        match self {
            Self::Strong => 1,
            Self::Emphasis => 2,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
struct Run {
    text: String,
    styles: u8,
    link: Option<usize>,
}

#[derive(Clone, Default, Eq, PartialEq)]
struct Projection {
    runs: Vec<Run>,
    html: Vec<(usize, String)>,
    links: Vec<(String, String)>,
    bytes: usize,
}

/// A link retains the accepted label meaning before bracket serialization.
/// Metadata comes from its typed source, never from reparsing a candidate URL.
#[derive(Clone)]
pub(in crate::encode::inline) struct LinkMeaning(Projection);

pub(in crate::encode::inline) fn link_meaning(
    label: &str,
    destination: &str,
    title: Option<&str>,
) -> Option<LinkMeaning> {
    let meaning = project(label)?;
    // CommonMark has no nested Link ownership. The reader's existing wrapper
    // policy decides transparent links before this encoder is reached.
    if !meaning.links.is_empty() {
        return None;
    }
    let mut linked = Projection {
        links: vec![(destination.to_owned(), title.unwrap_or_default().to_owned())],
        html: meaning.html,
        ..Projection::default()
    };
    for run in meaning.runs {
        for character in run.text.chars() {
            let owner = (character != '\n').then_some(0);
            linked.text(character.encode_utf8(&mut [0; 4]), run.styles, owner);
        }
    }
    Some(LinkMeaning(linked))
}

pub(in crate::encode::inline) fn matches_link(meaning: &LinkMeaning, source: &str) -> bool {
    project(source).is_some_and(|actual| actual == meaning.0)
}

impl Projection {
    fn text(&mut self, text: &str, styles: u8, link: Option<usize>) {
        if text.is_empty() {
            return;
        }
        self.bytes += text.len();
        if let Some(last) = self.runs.last_mut()
            && last.styles == styles
            && last.link == link
        {
            last.text.push_str(text);
        } else {
            self.runs.push(Run {
                text: text.into(),
                styles,
                link,
            });
        }
    }

    fn append(&mut self, other: Self) {
        let owners = self.links.len();
        self.links.extend(other.links);
        self.html.extend(
            other
                .html
                .into_iter()
                .map(|(offset, html)| (self.bytes + offset, html)),
        );
        for run in other.runs {
            self.text(&run.text, run.styles, run.link.map(|link| link + owners));
        }
    }

    fn with_style(self, source: &str, style: Style) -> Self {
        // CommonMark flanking uses Unicode whitespace. Raw edge whitespace
        // cannot carry this outer style, but its exact scalars survive outside
        // the wrapper. Code padding and escaped whitespace inside the accepted
        // child remain independent and retain their original style/code bits.
        let leading = source.len() - source.trim_start_matches(char::is_whitespace).len();
        let trailing = source.len() - source.trim_end_matches(char::is_whitespace).len();
        let end = self.bytes.saturating_sub(trailing);
        let mut output = Self {
            html: self.html,
            links: self.links,
            ..Self::default()
        };
        let mut offset = 0;
        for run in self.runs {
            for character in run.text.chars() {
                #[cfg(test)]
                super::work::PROJECTED_SCALARS.with(|visits| visits.set(visits.get() + 1));
                // Hard rows have no style/link owner in this projection.
                // Applying an outer style must not turn that boundary into
                // authored text or change the accepted child code/link bits.
                let styles = if character != '\n' && (leading..end).contains(&offset) {
                    run.styles | style.bit()
                } else {
                    run.styles
                };
                output.text(character.encode_utf8(&mut [0; 4]), styles, run.link);
                offset += character.len_utf8();
            }
        }
        output
    }
}

/// Expected meaning belongs to the accepted child before its new wrapper.
/// Parsing a slice of `whole` here would certify that candidate against itself:
/// outer delimiters can re-pair the child's formerly valid nested delimiters.
#[cfg(test)]
pub(super) fn equivalent<'a>(
    pieces: impl Iterator<Item = (&'a str, Option<Style>)>,
    whole: &str,
) -> bool {
    equivalent_framed(pieces, whole, Framing::Raw)
}

#[cfg(test)]
pub(super) fn equivalent_framed<'a>(
    pieces: impl Iterator<Item = (&'a str, Option<Style>)>,
    whole: &str,
    framing: Framing,
) -> bool {
    equivalent_with_meanings(
        pieces.map(|(source, style)| (source, style, None)),
        whole,
        framing,
    )
}

pub(super) fn equivalent_pieces(
    pieces: &[super::InlinePiece<'_>],
    choices: Option<&[usize]>,
    whole: &str,
    framing: Framing,
) -> bool {
    if pieces
        .iter()
        .any(|piece| matches!(piece.accepted, super::AcceptedMeaning::UnsupportedLink))
    {
        return false;
    }
    equivalent_with_meanings(
        pieces.iter().enumerate().map(|(index, piece)| {
            (
                piece.rendered.as_str(),
                choices
                    .and_then(|choices| super::styles::marker(piece, choices[index]))
                    .map(Style::from_marker),
                match &piece.accepted {
                    super::AcceptedMeaning::Phrasing => None,
                    super::AcceptedMeaning::Link(meaning) => Some(meaning),
                    super::AcceptedMeaning::UnsupportedLink => {
                        unreachable!("unsupported links were rejected")
                    }
                },
            )
        }),
        whole,
        framing,
    )
}

fn equivalent_with_meanings<'a>(
    pieces: impl Iterator<Item = (&'a str, Option<Style>, Option<&'a LinkMeaning>)>,
    whole: &str,
    framing: Framing,
) -> bool {
    let mut expected = Projection::default();
    for (source, style, accepted) in pieces {
        let piece = if let Some(meaning) = accepted {
            meaning.0.clone()
        } else if let Some(piece) = project(source) {
            piece
        } else {
            return false;
        };
        expected.append(match style {
            Some(style) => piece.with_style(source, style),
            None => piece,
        });
    }
    #[cfg(test)]
    super::work::WHOLE_PARSES.with(|count| count.set(count.get() + 1));
    let actual = match framing {
        Framing::Raw => project(whole),
        Framing::Rows { .. } => project_rows(whole),
    };
    actual.is_some_and(|actual| actual == expected)
}

/// Inspect the bytes actually received by the reader. In particular, do not
/// add the X/Z whitespace guards used for accepted child phrasing: entities at
/// an author's row edge have already changed the delimiter scanner's flanks.
fn project_rows(source: &str) -> Option<Projection> {
    #[cfg(test)]
    super::work::PARSED_BYTES.with(|bytes| bytes.set(bytes.get() + source.len()));
    project_events(
        crate::markdown::canonical_br::parse_events(source, pulldown_cmark::Options::empty())
            .map(|(event, _)| event),
    )
}

/// Reject an unsafe wrapper before sequence scoring can discard safe inner
/// styles. Each of the two marker choices is checked once per source piece.
pub(super) fn preserves_child(source: &str, style: Style, candidate: &str) -> bool {
    let Some(expected) = project(source) else {
        return false;
    };
    project(candidate).is_some_and(|actual| actual == expected.with_style(source, style))
}

fn project(source: &str) -> Option<Projection> {
    // A phrasing stream is not a block root. Guard both edges and encode
    // its existing hard rows as inline breaks; block-prefix/row framing is
    // applied by the caller after the accepted phrasing is returned.
    let mut framed = String::with_capacity(source.len() + 4);
    framed.push_str("X ");
    append_phrasing_rows(&mut framed, source);
    framed.push_str(" Z");
    #[cfg(test)]
    super::work::PARSED_BYTES.with(|bytes| bytes.set(bytes.get() + framed.len()));
    let mut result = project_events(Parser::new(&framed))?;
    let first = result.runs.first_mut()?;
    if first.styles != 0 || first.link.is_some() || !first.text.starts_with("X ") {
        return None;
    }
    first.text.drain(..2);
    let last = result.runs.last_mut()?;
    if last.styles != 0 || last.link.is_some() || !last.text.ends_with(" Z") {
        return None;
    }
    last.text.truncate(last.text.len() - 2);
    result.runs.retain(|run| !run.text.is_empty());
    result.bytes -= 4;
    for (offset, _) in &mut result.html {
        *offset = offset.checked_sub(2)?;
    }
    Some(result)
}

fn project_events<'a>(events: impl Iterator<Item = Event<'a>>) -> Option<Projection> {
    let mut result = Projection::default();
    let mut strong = 0;
    let mut emphasis = 0;
    let mut owners = Vec::new();
    for event in events {
        let code = u8::from(matches!(&event, Event::Code(_))) << 2;
        match event {
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong -= 1,
            Event::Start(Tag::Emphasis) => emphasis += 1,
            Event::End(TagEnd::Emphasis) => emphasis -= 1,
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                owners.push(result.links.len());
                result
                    .links
                    .push((dest_url.into_string(), title.into_string()));
            }
            Event::End(TagEnd::Link) => {
                owners.pop();
            }
            Event::Text(text) | Event::Code(text) => {
                // Code is a separate semantic bit; its literal delimiters
                // must never be confused with generated style punctuation.
                let styles = u8::from(strong > 0) | (u8::from(emphasis > 0) << 1) | code;
                result.text(&text, styles, owners.last().copied());
            }
            Event::InlineHtml(html) if matches!(html.as_ref(), "<br>" | "<br />") => {
                result.text("\n", 0, None);
            }
            Event::InlineHtml(html) => result.html.push((result.bytes, html.into_string())),
            Event::HardBreak => result.text("\n", 0, None),
            Event::SoftBreak
                if result
                    .runs
                    .last()
                    .is_some_and(|run| run.text.ends_with('\n')) => {}
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
            _ => return None,
        }
    }
    Some(result)
}

/// Hard rows have inline syntax at both the local and complete parse
/// boundaries. Raw consecutive newlines would create a Markdown block gap
/// that the final row encoder never emits for this accepted phrasing.
pub(super) fn append_phrasing_rows(output: &mut String, source: &str) {
    for row in source.split_inclusive('\n') {
        output.push_str(row.strip_suffix('\n').unwrap_or(row));
        if row.ends_with('\n') {
            output.push_str("<br>");
        }
    }
}

#[cfg(test)]
mod tests;
