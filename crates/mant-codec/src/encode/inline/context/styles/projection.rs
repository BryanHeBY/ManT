//! Compare accepted local phrasing with one actual parse of the whole sequence.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::Framing;

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

#[derive(Eq, PartialEq)]
struct Run {
    text: String,
    styles: u8,
    link: Option<usize>,
}

#[derive(Default, Eq, PartialEq)]
struct Projection {
    runs: Vec<Run>,
    html: Vec<(usize, String)>,
    links: Vec<(String, String)>,
    bytes: usize,
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
                super::PROJECTED_SCALARS.with(|visits| visits.set(visits.get() + 1));
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

pub(super) fn equivalent_framed<'a>(
    pieces: impl Iterator<Item = (&'a str, Option<Style>)>,
    whole: &str,
    framing: Framing,
) -> bool {
    let mut expected = Projection::default();
    for (source, style) in pieces {
        let Some(piece) = project(source) else {
            return false;
        };
        expected.append(match style {
            Some(style) => piece.with_style(source, style),
            None => piece,
        });
    }
    #[cfg(test)]
    super::WHOLE_PARSES.with(|count| count.set(count.get() + 1));
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
    super::PARSED_BYTES.with(|bytes| bytes.set(bytes.get() + source.len()));
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
    super::PARSED_BYTES.with(|bytes| bytes.set(bytes.get() + framed.len()));
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
mod tests {
    fn equivalent<'a>(pieces: impl Iterator<Item = &'a str>, whole: &str) -> bool {
        super::equivalent(pieces.map(|piece| (piece, None)), whole)
    }

    #[test]
    fn outer_style_cannot_certify_reinterpreted_child_delimiters() {
        for (child, corrupted) in [
            (r"_`a`_b\*__", r"**_`a`_b\*__**"),
            (
                r"_[a](https://a.test)_b\*__",
                r"**_[a](https://a.test)_b\*__**",
            ),
        ] {
            assert!(!super::equivalent(
                [(child, Some(super::Style::Strong))].into_iter(),
                corrupted,
            ));
        }
        assert!(super::equivalent(
            [(" `a` ", Some(super::Style::Strong))].into_iter(),
            " **`a`** ",
        ));
        assert!(!super::equivalent(
            [(" `a` ", Some(super::Style::Strong))].into_iter(),
            "** `a` **",
        ));
    }

    #[test]
    fn complete_check_rejects_remote_pairing_changed_styles_and_link_occurrences() {
        assert!(!equivalent(
            ["**$((**", "*arithmetic_expression*"].into_iter(),
            "**$((***arithmetic_expression*"
        ));
        assert!(!equivalent(["**A**", "_B_"].into_iter(), "_A_**B**"));
        assert!(!equivalent(
            ["[A](https://one.test)", "[B](https://one.test)"].into_iter(),
            "[AB](https://one.test)"
        ));
        assert!(!equivalent(
            ["[A](https://one.test \"first\")"].into_iter(),
            "[A](https://one.test \"second\")"
        ));
        assert!(!equivalent(["`A`"].into_iter(), "A"));
        assert!(equivalent(
            ["**--arg=**", "_reuse_level_"].into_iter(),
            "**--arg=**_reuse_level_"
        ));
    }

    #[test]
    fn final_bytes_reject_row_entity_flanks_and_unprotected_image_openers() {
        let framing = super::Framing::Rows {
            manual_links: false,
            interruptible: true,
        };
        // Independent accepted source children, before the final delimiter
        // choices and row-edge rewrite. Restoring the old raw-only validator
        // would admit the damaged first sequence; removing seam protection
        // would admit the image in the second sequence.
        assert!(!super::equivalent_framed(
            [("_a ***b***_", None), (" ", None)].into_iter(),
            "_a _**b**__&#32;",
            framing,
        ));
        assert!(!super::equivalent_framed(
            [("!", None), ("[a](https://a.test)", None)].into_iter(),
            "![a](https://a.test)",
            framing,
        ));
        assert!(super::equivalent_framed(
            [("!", None), ("[a](https://a.test)", None)].into_iter(),
            "\\![a](https://a.test)",
            framing,
        ));
    }
}
