//! Map literal source markers and Markdown links to the live report HTML tree.
use crate::encode::anchors::{AnchorMarker, marker_at};
use html5ever::{ParseOpts, QualName, local_name, ns, parse_fragment, tendril::TendrilSink};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::{cell::Cell, collections::HashSet, fmt, fmt::Write as _, ops::Range};

mod tree;
use tree::{Node, ReportTree};

const PREFIX: &str = "<span id=\"";
const SUFFIX: &str = "\"></span>";

pub(super) fn report_navigation(markdown: &str) -> (Vec<AnchorMarker<'_>>, HashSet<String>) {
    if !has_candidates(markdown) {
        return (Vec::new(), HashSet::new());
    }

    let (html, candidates, links, probe) = render_probes(markdown);
    if candidates.is_empty() && links.is_empty() {
        return (candidates, HashSet::new());
    }
    let document = parse_report(html);
    let (live, links, host_ids) = observe_document(&document, &probe, &candidates, &links);
    let markers = candidates
        .into_iter()
        .zip(live)
        .filter_map(|(candidate, live)| {
            (live && !host_ids.contains(candidate.id)).then_some(candidate)
        })
        .collect();
    (markers, links)
}

fn parse_report(html: String) -> Vec<Node> {
    parse_fragment(
        ReportTree::default(),
        ParseOpts::default(),
        QualName::new(None, ns!(html), local_name!("div")),
        Vec::new(),
        true,
    )
    .one(html)
}

const PROBE_PREFIX: &str = "data-mant-report-probe-";

fn observed_probe_ids(input: &str) -> impl Iterator<Item = usize> {
    input.match_indices(PROBE_PREFIX).filter_map(|(start, _)| {
        let value = &input[start + PROBE_PREFIX.len()..];
        let digits = value
            .as_bytes()
            .iter()
            .take(20)
            .take_while(|b| b.is_ascii_digit())
            .count();
        value[digits..]
            .starts_with("-end")
            .then(|| value[..digits].parse().ok())
            .flatten()
    })
}

fn probe_attribute(html: &str) -> String {
    // HTML tokenization folds ASCII case in attribute names but does not
    // decode character references there. Reserve names over the final
    // serialized input, where Markdown escapes have already been resolved.
    let folded = html.to_ascii_lowercase();
    let used: HashSet<_> = observed_probe_ids(&folded).collect();
    let id = (0..=used.len())
        .find(|id| !used.contains(id))
        .expect("one more candidate than used probe IDs");
    format!("{PROBE_PREFIX}{id}-end")
}

fn scoped_id(id: &str) -> bool {
    let Some((source, encoded)) = id
        .strip_prefix("mant-report-")
        .and_then(|id| id.split_once('-'))
    else {
        return false;
    };
    !source.is_empty()
        && (source == "0" || !source.starts_with('0'))
        && source.bytes().all(|b| b.is_ascii_digit())
        && source.parse::<usize>().is_ok()
        && encoded.len().is_multiple_of(2)
        && encoded
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}

fn markdown_events(markdown: &str) -> impl Iterator<Item = (Event<'_>, Range<usize>)> {
    Parser::new_ext(markdown, Options::ENABLE_MATH).into_offset_iter()
}

fn raw_markers<'a, 'v>(
    markdown: &'a str,
    start: usize,
    value: &'v str,
) -> impl Iterator<Item = (usize, AnchorMarker<'a>)> + use<'a, 'v> {
    value.match_indices(PREFIX).filter_map(move |(offset, _)| {
        let position = start + offset;
        let anchor = marker_at(&markdown[position..], position, PREFIX, SUFFIX)
            .filter(|anchor| scoped_id(anchor.id))?;
        let end = offset + PREFIX.len() + anchor.id.len() + 1;
        (end < value.len()).then_some((end, anchor))
    })
}

fn scoped_link(url: &str) -> bool {
    url.strip_prefix('#').is_some_and(scoped_id)
}

fn has_candidates(markdown: &str) -> bool {
    // A generated marker is literal. A CommonMark link with a fragment
    // destination (even an escaped or reference destination) needs '[';
    // fragment-only URLs are not autolinks.
    if !markdown.contains(PREFIX) && !markdown.contains('[') {
        return false;
    }

    let mut image_depth = 0;
    markdown_events(markdown).any(|(event, range)| match event {
        Event::Start(Tag::Image { .. }) => {
            image_depth += 1;
            false
        }
        Event::End(TagEnd::Image) => {
            image_depth -= 1;
            false
        }
        Event::Start(Tag::Link { dest_url, .. }) => image_depth == 0 && scoped_link(&dest_url),
        Event::Html(value) | Event::InlineHtml(value)
            if image_depth == 0 && value.as_ref() == &markdown[range.clone()] =>
        {
            raw_markers(markdown, range.start, &value).next().is_some()
        }
        _ => false,
    })
}

/// Record offsets in the real serializer instead of rewriting Markdown Link
/// events (which also occur inside image alt text). The HTML proof gets the
/// same tags and hrefs, with an extra private attribute only on candidate tags.
struct HtmlOutput<'a> {
    html: String,
    written: &'a Cell<usize>,
}

impl fmt::Write for HtmlOutput<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.html.push_str(value);
        self.written.set(self.html.len());
        Ok(())
    }
}

fn render_probes(markdown: &str) -> (String, Vec<AnchorMarker<'_>>, Vec<String>, String) {
    let mut anchors = Vec::new();
    let mut links = Vec::new();
    let mut insertions = Vec::new();
    let mut pending_link = None;
    let mut image_depth = 0;
    let written = Cell::new(0);
    let mut output = HtmlOutput {
        html: String::new(),
        written: &written,
    };
    let events = markdown_events(markdown).map(|(event, range)| {
        if let Some(index) = pending_link.take() {
            // pulldown emits a complete '<a ...>' for this Start event.
            insertions.push((written.get() - 1, 'l', index));
        }
        match &event {
            Event::Start(Tag::Image { .. }) => image_depth += 1,
            Event::End(TagEnd::Image) => image_depth -= 1,
            Event::Start(Tag::Link { dest_url, .. })
                if image_depth == 0 && scoped_link(dest_url) =>
            {
                pending_link = Some(links.len());
                links.push(dest_url[1..].to_owned());
            }
            Event::Html(value) | Event::InlineHtml(value)
                if image_depth == 0 && value.as_ref() == &markdown[range.clone()] =>
            {
                for (offset, anchor) in raw_markers(markdown, range.start, value) {
                    insertions.push((written.get() + offset, 'a', anchors.len()));
                    anchors.push(anchor);
                }
            }
            _ => {}
        }
        event
    });
    pulldown_cmark::html::write_html_fmt(&mut output, events).expect("write to String");

    let probe = probe_attribute(&output.html);
    let mut html = String::with_capacity(output.html.len());
    let mut copied = 0;
    for (offset, kind, index) in insertions {
        debug_assert_eq!(output.html.as_bytes()[offset], b'>');
        html.push_str(&output.html[copied..offset]);
        write!(html, " {probe}={kind}{index}").expect("write to String");
        copied = offset;
    }
    html.push_str(&output.html[copied..]);
    (html, anchors, links, probe)
}

fn observe_document(
    document: &[Node],
    probe: &str,
    anchors: &[AnchorMarker<'_>],
    links: &[String],
) -> (Vec<bool>, HashSet<String>, HashSet<String>) {
    let mut live = vec![false; anchors.len()];
    let mut live_links = HashSet::new();
    let mut host_ids = HashSet::new();
    for node in ReportTree::elements(document) {
        if let Some(id) = node.attr("id") {
            let index = node
                .attr(probe)
                .and_then(|attr| attr.strip_prefix('a')?.parse::<usize>().ok())
                .filter(|&index| {
                    node.has_name("span") && anchors.get(index).is_some_and(|a| a.id == id)
                });
            if let Some(index) = index {
                live[index] = true;
            } else if scoped_id(id) {
                // Host IDs are not ours to rewrite, and ambiguous links
                // must not take the reader to an unrelated element.
                host_ids.insert(id.to_string());
            }
        }
        if node.has_name("a")
            && let Some(link) = node
                .attr(probe)
                .and_then(|attr| attr.strip_prefix('l')?.parse::<usize>().ok())
                .and_then(|index| links.get(index))
                .filter(|link| {
                    node.attr("href").and_then(|href| href.strip_prefix('#')) == Some(link)
                })
        {
            live_links.insert(link.clone());
        }
    }
    (live, live_links, host_ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_gate_compares_parsed_scoped_links_and_literal_visible_markers() {
        const MARKER: &str = r#"<span id="mant-report-0-61"></span>"#;
        for source in [
            "ordinary A & B".to_owned(),
            "[external](https://a.test/?x=1&y=2)".to_owned(),
            format!("[unrelated](#contents) `{MARKER}`"),
            format!("~~~html\n{MARKER}\n~~~"),
            "![alt [not a jump](#mant-report-0-61)](pic.png)".to_owned(),
        ] {
            assert!(!has_candidates(&source), "{source}");
            let (markers, links) = report_navigation(&source);
            assert!(markers.is_empty() && links.is_empty(), "{source}");
        }

        for source in [
            MARKER,
            "[jump](#mant-report-0-61)",
            "[jump](<&#35;mant&#45;report-0-61>)",
            "[jump][target]\n\n[target]: <#mant\\-report-0-61>",
        ] {
            assert!(has_candidates(source), "{source}");
        }
    }

    #[test]
    fn proof_preserves_serialized_href_and_image_alt_as_written() {
        const ID: &str = "mant-report-0-61";
        let markdown = format!("![before [inner](#{ID}) after](pic.png) [jump](#{ID} \"Title\")");
        let mut expected = String::new();
        pulldown_cmark::html::push_html(
            &mut expected,
            markdown_events(&markdown).map(|(event, _)| event),
        );
        let (html, anchors, links, probe) = render_probes(&markdown);
        assert!(anchors.is_empty());
        assert_eq!(links, [ID]);
        assert_eq!(html.replace(&format!(" {probe}=l0"), ""), expected);
        let (_, links, _) = observe_document(&parse_report(html), &probe, &anchors, &links);
        assert_eq!(links, HashSet::from([ID.into()]));
    }

    #[test]
    fn fragment_observes_fostered_and_reparented_elements_but_not_template_contents() {
        let document = parse_report(
            "<table><span id='fostered'></span><tr><td id='cell'>x</table>             <b><p><span id='reparented'></span></b></p>             <template><span id='inert'></span></template>             <math><annotation-xml encoding='text/html'><span id='math-child'></span></annotation-xml></math>"
                .to_owned(),
        );
        let ids: HashSet<_> = ReportTree::elements(&document)
            .filter_map(|node| node.attr("id"))
            .collect();
        assert_eq!(
            ids,
            HashSet::from(["fostered", "cell", "reparented", "math-child"])
        );
    }
}
