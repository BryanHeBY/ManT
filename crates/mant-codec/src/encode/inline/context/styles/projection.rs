//! Compare accepted local phrasing with one actual parse of the whole sequence.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

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
}

pub(super) fn equivalent<'a>(pieces: impl Iterator<Item = &'a str>, whole: &str) -> bool {
    let mut expected = Projection::default();
    for piece in pieces {
        let Some(piece) = project(piece) else {
            return false;
        };
        expected.append(piece);
    }
    #[cfg(test)]
    super::WHOLE_PARSES.with(|count| count.set(count.get() + 1));
    project(whole).is_some_and(|actual| actual == expected)
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
    let mut result = Projection::default();
    let mut strong = 0;
    let mut emphasis = 0;
    let mut owners = Vec::new();
    for event in Parser::new(&framed) {
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
            Event::InlineHtml(html) if html.as_ref() == "<br>" => result.text("\n", 0, None),
            Event::InlineHtml(html) => result.html.push((result.bytes, html.into_string())),
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
            _ => return None,
        }
    }
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
    use super::equivalent;

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
}
