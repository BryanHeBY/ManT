//! Map literal source markers and Markdown links to the live report HTML tree.
use crate::encode::anchors::{AnchorMarker, marker_at};
use html5ever::{ParseOpts, QualName, local_name, ns, parse_fragment, tendril::TendrilSink};
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::{collections::HashSet, fmt::Write as _};

mod tree;
use tree::{Node, ReportTree};

const PREFIX: &str = "<span id=\"";
const SUFFIX: &str = "\"></span>";

pub(super) fn report_navigation(markdown: &str) -> (Vec<AnchorMarker<'_>>, HashSet<String>) {
    // These temporary attributes allow the HTML tree to identify the exact
    // candidate occurrence. They are never used to render the returned page.
    let probe = probe_attribute(markdown);
    let (html, candidates, links) = render_probes(markdown, &probe);
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

fn probe_attribute(markdown: &str) -> String {
    let folded = markdown.to_ascii_lowercase();
    let mut used: HashSet<usize> = observed_probe_ids(&folded).collect();
    // Attribute names are never entity decoded, but a host's href may be.
    // Check those decoded attributes before using any synthetic link receipt.
    if markdown.contains('&') {
        let mut plain_html = String::new();
        pulldown_cmark::html::push_html(
            &mut plain_html,
            Parser::new_ext(markdown, Options::ENABLE_MATH),
        );
        let document = parse_report(plain_html);
        for node in ReportTree::elements(&document) {
            if node.has_name("a")
                && let Some(href) = node.attr("href")
            {
                used.extend(observed_probe_ids(href));
            }
        }
    }
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

fn render_probes<'a>(
    markdown: &'a str,
    probe: &str,
) -> (String, Vec<AnchorMarker<'a>>, Vec<String>) {
    let mut anchors = Vec::new();
    let mut links = Vec::new();
    let events = Parser::new_ext(markdown, Options::ENABLE_MATH).into_offset_iter();
    let mut html = String::new();
    pulldown_cmark::html::push_html(
        &mut html,
        events.map(|(event, range)| match event {
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) if dest_url.strip_prefix('#').is_some_and(scoped_id) => {
                let receipt = format!("#{probe}-link-{}", links.len());
                links.push(dest_url[1..].to_owned());
                Event::Start(Tag::Link {
                    link_type,
                    dest_url: receipt.into(),
                    title,
                    id,
                })
            }
            Event::Html(ref value) | Event::InlineHtml(ref value)
                if value.as_ref() == &markdown[range.clone()] =>
            {
                let Some(value) = mark_raw(markdown, range.start, value, probe, &mut anchors)
                else {
                    return event;
                };
                if matches!(event, Event::Html(_)) {
                    Event::Html(value.into())
                } else {
                    Event::InlineHtml(value.into())
                }
            }
            _ => event,
        }),
    );
    (html, anchors, links)
}

fn mark_raw<'a>(
    markdown: &'a str,
    start: usize,
    value: &str,
    probe: &str,
    anchors: &mut Vec<AnchorMarker<'a>>,
) -> Option<String> {
    let mut output = String::new();
    let mut copied = 0;
    for (offset, _) in value.match_indices(PREFIX) {
        let start = start + offset;
        let Some(anchor) = marker_at(&markdown[start..], start, PREFIX, SUFFIX)
            .filter(|anchor| scoped_id(anchor.id))
        else {
            continue;
        };
        let end = offset + PREFIX.len() + anchor.id.len() + 1;
        if end >= value.len() {
            continue;
        }
        output.push_str(&value[copied..end]);
        write!(output, " {probe}={}", anchors.len()).expect("write to String");
        copied = end;
        anchors.push(anchor);
    }
    if copied == 0 {
        return None;
    }
    output.push_str(&value[copied..]);
    Some(output)
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
    let link_prefix = format!("#{probe}-link-");
    for node in ReportTree::elements(document) {
        if let Some(id) = node.attr("id") {
            let index = node
                .attr(probe)
                .and_then(|attr| attr.parse::<usize>().ok())
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
                .attr("href")
                .and_then(|attr| attr.strip_prefix(&link_prefix)?.parse::<usize>().ok())
                .and_then(|index| links.get(index))
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
