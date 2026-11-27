//! Source-map markers are HTML elements, never lookalike code or escaped text.
use pulldown_cmark::{Event, Parser};
use std::ops::Range;

pub(crate) struct AnchorMarker<'a> {
    pub(super) id: &'a str,
    pub(crate) range: Range<usize>,
}

/// These ranges hide zero-width presentation only; semantic owner ranges are
/// composed independently by the renderer. Code fences/spans are not markers.
pub(crate) fn anchor_markers(markdown: &str) -> Vec<AnchorMarker<'_>> {
    let mut markers = Vec::new();
    let mut scanner = HtmlAnchorScanner::default();
    for (event, range) in
        Parser::new_ext(markdown, pulldown_cmark::Options::ENABLE_MATH).into_offset_iter()
    {
        if matches!(event, Event::Html(_) | Event::InlineHtml(_)) {
            scanner.scan(markdown, range, true, |marker| markers.push(marker));
        }
    }
    markers
}

/// `CommonMark` only identifies HTML ranges, including comments, raw-text
/// elements and complete tags with quoted attributes. Require the exact
/// emitted anchor spelling at a tag boundary, not a substring of that range.
/// An HTML block may split over events. The raw-text context survives
/// intervening Markdown events, but only literal HTML can close it.
#[derive(Default)]
pub(super) struct HtmlAnchorScanner {
    consumed: usize,
    raw_text: Option<RawText>,
}

impl HtmlAnchorScanner {
    pub(super) fn scan<'a>(
        &mut self,
        markdown: &'a str,
        range: Range<usize>,
        legacy: bool,
        mut emit: impl FnMut(AnchorMarker<'a>),
    ) {
        let mut cursor = self.consumed.max(range.start);
        while cursor < range.end {
            if let Some(raw_text) = &mut self.raw_text {
                let Some(end) = raw_text.consume(&markdown[cursor..range.end]) else {
                    break;
                };
                cursor += end;
                self.raw_text = None;
                continue;
            }
            let Some(relative) = markdown[cursor..range.end].find('<') else {
                break;
            };
            let start = cursor + relative;
            let rest = &markdown[start..];
            if let Some(marker) =
                marker_at(rest, start, "<span id=\"", "\"></span>").or_else(|| {
                    legacy
                        .then(|| marker_at(rest, start, "<a id=\"", "\"></a>"))
                        .flatten()
                })
            {
                cursor = marker.range.end;
                emit(marker);
                continue;
            }
            let skip = if let Some(comment) = rest.strip_prefix("<!--") {
                // In an HTML block the comment can contain multiple source
                // lines, each delivered as a separate HTML event.
                if comment.starts_with('>') {
                    5
                } else if comment.starts_with("->") {
                    6
                } else {
                    ["-->", "--!>"]
                        .into_iter()
                        .filter_map(|end| comment.find(end).map(|at| 4 + at + end.len()))
                        .min()
                        .unwrap_or(rest.len())
                }
            } else if let Some(cdata) = rest.strip_prefix("<![CDATA[") {
                cdata.find("]]>").map_or(rest.len(), |at| 9 + at + 3)
            } else if rest.starts_with("<!") || rest.starts_with("<?") {
                rest.find('>').map_or(rest.len(), |at| at + 1)
            } else if let Some((name, closing)) = tag_name(rest) {
                let end = tag_end(rest).unwrap_or(rest.len());
                if !closing {
                    if name.eq_ignore_ascii_case("plaintext") {
                        cursor = markdown.len();
                        break;
                    }
                    // In HTML a trailing slash does not close these elements.
                    self.raw_text = raw_text_element(name).map(RawText::new);
                }
                end
            } else {
                1
            };
            cursor = start + skip;
        }
        self.consumed = cursor;
    }
}

pub(super) fn marker_at<'a>(
    rest: &'a str,
    start: usize,
    prefix: &str,
    suffix: &str,
) -> Option<AnchorMarker<'a>> {
    let id = rest.strip_prefix(prefix)?;
    let id_end = id.find(['<', '>', '\n', '\r', '"'])?;
    if id_end == 0 || !id[id_end..].starts_with(suffix) {
        return None;
    }
    Some(AnchorMarker {
        id: &id[..id_end],
        range: start..start + prefix.len() + id_end + suffix.len(),
    })
}

fn tag_name(rest: &str) -> Option<(&str, bool)> {
    let rest = rest.strip_prefix('<')?;
    let (rest, closing) = rest
        .strip_prefix('/')
        .map_or((rest, false), |rest| (rest, true));
    if !rest.as_bytes().first()?.is_ascii_alphabetic() {
        return None;
    }
    let end = rest.find(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '>' | '/'))?;
    Some((&rest[..end], closing))
}

fn tag_end(rest: &str) -> Option<usize> {
    let mut quote = None;
    for (at, ch) in rest.bytes().enumerate().skip(1) {
        match (quote, ch) {
            (Some(expected), actual) if expected == actual => quote = None,
            (None, b'\'' | b'"') => quote = Some(ch),
            (None, b'>') => return Some(at + 1),
            _ => {}
        }
    }
    None
}

fn raw_text_element(name: &str) -> Option<&'static str> {
    [
        "script", "style", "xmp", "iframe", "noembed", "noframes", "noscript", "textarea", "title",
    ]
    .into_iter()
    .find(|element| name.eq_ignore_ascii_case(element))
}

struct RawText {
    name: &'static str,
    escaped_script: bool,
    double_escaped_script: bool,
}

impl RawText {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            escaped_script: false,
            double_escaped_script: false,
        }
    }

    fn consume(&mut self, html: &str) -> Option<usize> {
        // Script may use HTML's double escaped state. Once that occurs, the
        // next closing spelling leaves that state rather than closing the tag.
        let script = self.name == "script";
        for (at, ch) in html.char_indices() {
            let tail = &html[at..];
            if script && ch == '<' && tail.starts_with("<!--") {
                self.escaped_script = true;
            } else if script && ch == '-' && tail.starts_with("-->") {
                self.escaped_script = false;
                self.double_escaped_script = false;
            } else if ch == '<' {
                if raw_tag_boundary(&tail[1..], "/", self.name) {
                    if !self.double_escaped_script {
                        return tag_end(tail).map(|end| at + end);
                    }
                    self.double_escaped_script = false;
                } else if self.escaped_script && raw_tag_boundary(&tail[1..], "", self.name) {
                    self.double_escaped_script = true;
                }
            }
        }
        None
    }
}

fn raw_tag_boundary(tail: &str, prefix: &str, name: &str) -> bool {
    let Some(tail) = tail.strip_prefix(prefix) else {
        return false;
    };
    tail.get(..name.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
        && tail
            .as_bytes()
            .get(name.len())
            .is_some_and(|ch| ch.is_ascii_whitespace() || matches!(*ch, b'>' | b'/'))
}

#[cfg(test)]
mod tests {
    use super::anchor_markers;

    #[test]
    fn scanner_indexes_only_complete_empty_inline_anchors() {
        for tag in ["span", "a"] {
            let marker = format!("<{tag} id=\"node\"></{tag}>");
            let text = format!("before{marker}after");
            let ranges: Vec<_> = anchor_markers(&text).into_iter().map(|m| m.range).collect();
            assert_eq!(ranges, vec![6..6 + marker.len()]);
            for retained in [
                format!("before<{tag} id=\"node\">payload</{tag}>after"),
                format!("before<{tag} id=\"node\"after"),
                format!("\u{60}{marker}\u{60}"),
                format!("\\{marker}"),
                format!("~~~html\n{marker}\n~~~"),
            ] {
                assert!(anchor_markers(&retained).is_empty(), "{retained}");
            }
        }
    }

    #[test]
    fn scanner_orders_current_and_legacy_markers_in_one_html_block() {
        let text = "<br />\n<a id=\"first\"></a><span id=\"second\"></span>\n";
        assert_eq!(
            anchor_markers(text)
                .into_iter()
                .map(|marker| marker.id)
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    #[test]
    fn scanner_skips_lookalikes_inside_comments_attributes_and_raw_text() {
        let fake = r#"<span id="fake"></span><a id="legacy-fake"></a>"#;
        let real = r#"<span id="real"></span><span id="next"></span>"#;
        for hidden in [
            format!("before <!-- {fake} --> "),
            format!("<br />\n<!--\n{fake}\n-->\n"),
            format!("<i title='{fake}'>label</i> "),
            format!("<br />\n<div title='quoted\n{fake}\nvalue'>label</div>\n"),
            format!("<script>\n{fake}\n</script>\n\n"),
            format!("<StYlE>{fake}</sTyLe>\n\n"),
            format!("Label <textarea>{fake}</TEXTAREA> "),
            format!("<script><!--<script></script>\n{fake}\n</script>\n\n"),
        ] {
            let markdown = format!("{hidden}{real} after");
            assert_eq!(
                anchor_markers(&markdown)
                    .into_iter()
                    .map(|marker| marker.id)
                    .collect::<Vec<_>>(),
                ["real", "next"],
                "{markdown}"
            );
        }
    }
}
