//! Map literal source markers and Markdown links to the live report HTML tree.
use crate::encode::anchors::{AnchorMarker, marker_at};
use dom_query::Document;
use html5ever::{ParseOpts, QualName, local_name, ns, parse_fragment, tendril::TendrilSink};
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::{collections::HashSet, fmt::Write as _};

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
    let document = parse_fragment(
        Document::fragment_sink(),
        ParseOpts::default(),
        QualName::new(None, ns!(html), local_name!("div")),
        Vec::new(),
        true,
    )
    .one(html);
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

fn probe_attribute(markdown: &str) -> String {
    const BASE: &str = "data-mant-report-probe-";
    let folded = markdown.to_ascii_lowercase();
    let used: HashSet<usize> = folded
        .match_indices(BASE)
        .filter_map(|(start, _)| {
            let value = &folded[start + BASE.len()..];
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
        .collect();
    let id = (0..=used.len())
        .find(|id| !used.contains(id))
        .expect("one more candidate than used probe IDs");
    format!("{BASE}{id}-end")
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
            }) if dest_url.starts_with('#') => {
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
        let Some(anchor) = marker_at(&markdown[start..], start, PREFIX, SUFFIX) else {
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
    document: &Document,
    probe: &str,
    anchors: &[AnchorMarker<'_>],
    links: &[String],
) -> (Vec<bool>, HashSet<String>, HashSet<String>) {
    let mut stack = vec![document.root()];
    let mut live = vec![false; anchors.len()];
    let mut live_links = HashSet::new();
    let mut host_ids = HashSet::new();
    let link_prefix = format!("#{probe}-link-");
    while let Some(node) = stack.pop() {
        if let Some(id) = node.attr("id") {
            let index = node
                .attr(probe)
                .and_then(|attr| attr.parse::<usize>().ok())
                .filter(|&index| {
                    node.has_name("span") && anchors.get(index).is_some_and(|a| a.id == id.as_ref())
                });
            if let Some(index) = index {
                live[index] = true;
            } else {
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
        // Template fragments are not reachable by document fragment links.
        stack.extend(node.children_it(false));
    }
    (live, live_links, host_ids)
}
