//! Project source-qualified navigation from the actually rendered report page.
use super::detached_navigation::{OPTIONS, scope_graph};
use mant_ir::{DocumentAddress, LinkTarget, MarkdownOrigin, ResolvedContent};
use mant_protocol::{ExplanationOptions, ExplanationQuery, ScopeExplanation};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::{collections::HashMap, ops::Range};

struct Link {
    label: String,
    target: String,
    title: String,
}

pub(super) struct Page<'a> {
    markdown: &'a str,
    anchors: HashMap<String, usize>,
    quotes: Vec<Range<usize>>,
    links: Vec<Link>,
    visible: String,
}

impl<'a> Page<'a> {
    pub(super) fn read(markdown: &'a str) -> Self {
        let mut page = Self {
            markdown,
            anchors: HashMap::new(),
            quotes: Vec::new(),
            links: Vec::new(),
            visible: String::new(),
        };
        let mut link = None::<Link>;
        for (event, range) in Parser::new_ext(markdown, Options::ENABLE_MATH).into_offset_iter() {
            match event {
                Event::Start(Tag::BlockQuote(_)) => page.quotes.push(range),
                Event::Start(Tag::Link {
                    dest_url, title, ..
                }) => {
                    assert!(link.is_none());
                    link = Some(Link {
                        target: dest_url.into_string(),
                        title: title.into_string(),
                        label: String::new(),
                    });
                }
                Event::End(TagEnd::Link) => page.links.push(link.take().unwrap()),
                Event::Text(text) | Event::Code(text) => {
                    page.visible.push_str(&text);
                    if let Some(link) = &mut link {
                        link.label.push_str(&text);
                    }
                }
                Event::Html(_) | Event::InlineHtml(_) => {
                    if let Some(id) = markdown[range.clone()]
                        .strip_prefix("<span id=\"")
                        .and_then(|html| html.split_once("\">").map(|(id, _)| id))
                    {
                        assert!(
                            page.anchors.insert(id.to_owned(), range.start).is_none(),
                            "duplicate active destination {id:?}\n{markdown}"
                        );
                    }
                }
                _ => {}
            }
        }
        for link in &page.links {
            if let Some(id) = link.target.strip_prefix('#') {
                assert!(
                    page.anchors.contains_key(id),
                    "dangling destination {id:?}\n{markdown}"
                );
            }
        }
        page
    }

    pub(super) fn local_links(&self) -> usize {
        self.links
            .iter()
            .filter(|link| link.target.starts_with('#'))
            .count()
    }

    pub(super) fn source_anchors(&self, document: usize, original: &str) -> usize {
        self.anchors
            .keys()
            .filter(|id| is_source_id(id, document, original))
            .count()
    }

    pub(super) fn source_links(&self, document: usize, original: &str) -> usize {
        self.links
            .iter()
            .filter(|link| {
                link.target
                    .strip_prefix('#')
                    .is_some_and(|id| is_source_id(id, document, original))
            })
            .count()
    }

    pub(super) fn link_count(&self, label: &str) -> usize {
        self.links.iter().filter(|link| link.label == label).count()
    }

    pub(super) fn link(&self, label: &str) -> &str {
        let links = self
            .links
            .iter()
            .filter(|link| link.label == label)
            .collect::<Vec<_>>();
        let [link] = links.as_slice() else {
            panic!("{label:?}\n{}", self.markdown)
        };
        &link.target
    }

    fn assert_owner(&self, link: &str, expected: &str, other: &[&str]) {
        let target = self.link(link).strip_prefix('#').unwrap();
        let anchor = self.anchors[target];
        let quote = self
            .quotes
            .iter()
            .find(|quote| quote.contains(&anchor))
            .unwrap();
        let body = &self.markdown[quote.clone()];
        assert!(
            body.contains(expected),
            "{link:?} targets the wrong body\n{body}"
        );
        for unwanted in other {
            assert!(
                !body.contains(unwanted),
                "{link:?} targets the wrong source\n{body}"
            );
        }
    }
}

fn is_source_id(id: &str, document: usize, original: &str) -> bool {
    id.strip_prefix("mant-report-")
        .and_then(|tail| tail.split_once('-'))
        .is_some_and(|(index, code)| {
            index.parse() == Ok(document)
                && code.len() == original.len() * 2
                && code
                    .as_bytes()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .zip(original.bytes())
                    .all(|(hex, byte)| {
                        std::str::from_utf8(hex)
                            .ok()
                            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                            == Some(byte)
                    })
        })
}

fn source(markdown: &str, path: &str, origin: MarkdownOrigin) -> ResolvedContent {
    let mut content = mant_loader::load_markdown_text(markdown, None).unwrap();
    content.address = Some(DocumentAddress::Markdown {
        path: path.into(),
        origin,
    });
    content.label = "Same title".into();
    assert_eq!(
        mant_ir::validate_document(content.document.as_ref().unwrap()),
        []
    );
    content
}

fn explain(sources: &[ResolvedContent], options: ExplanationOptions) -> ScopeExplanation {
    let graph = scope_graph(sources);
    let result = mant_query::explain_scope(
        mant_query::QueryScopeView::new(&graph, sources).unwrap(),
        &ExplanationQuery {
            entry: "--term".into(),
            options,
        },
    )
    .unwrap();
    let decoded = serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
    assert_eq!(result, decoded);
    decoded
}

const ALPHA: &str = concat!(
    "# alpha\n\n## Options\n\n",
    "- `--term`: ALPHABODY [here-alpha](#option-term \"same destination\") ",
    "[elsewhere](#option-target) [web](https://example.test/#option-term) ",
    "[manual](man:printf(1)) [relative](peer.md#option-term) ",
    "`<span id=\"mant-report-0-6f7074696f6e2d7465726d\"></span>`\n",
    "- `--target`: ANOTHERBODY\n",
);
const BETA: &str = "# beta\n\n## Options\n\n- `--term`: BETABODY [here-beta](#option-term)\n";
const GAMMA: &str = concat!(
    "# beta\n\n## Options\n\n",
    "- `--target`, `--term`: GAMMABODY [here-gamma](#option-target)\n",
    "\nA prose mention of --term and [options](#options).\n"
);

#[test]
fn actual_markdown_sources_have_independent_page_destinations() {
    let sources = [
        source(ALPHA, "résumé dir/leaf", MarkdownOrigin::Documents),
        source(BETA, "résumé-dir/leaf", MarkdownOrigin::Documents),
        source(
            GAMMA,
            "résumé dir/leaf",
            MarkdownOrigin::Source {
                name: "résumé-dir".into(),
            },
        ),
    ];
    for count in 1..=sources.len() {
        let report = explain(&sources[..count], ExplanationOptions::default());
        assert_eq!(report.documents.len(), count);
        assert!(
            report
                .documents
                .iter()
                .all(|source| source.semantics_complete)
        );
        assert_eq!(
            report
                .evidence
                .iter()
                .take(count)
                .map(|e| e.evidence.outline.node.id())
                .collect::<Vec<_>>(),
            ["option-term", "option-term", "option-target"][..count]
        );
        let markdown =
            mant_render::render_scope_explanation_markdown_with_options(&report, OPTIONS);
        let page = Page::read(&markdown);
        assert_eq!(page.anchors.len(), count, "{markdown}");
        assert_eq!(page.local_links(), count);
        assert_eq!(page.link_count("elsewhere"), 0);
        assert!(page.visible.contains("elsewhere"), "{markdown}");
        assert!(
            page.visible
                .contains("<span id=\"mant-report-0-6f7074696f6e2d7465726d\"></span>")
        );
        assert!(!page.visible.contains("ANOTHERBODY"), "{markdown}");
        page.assert_owner("here-alpha", "ALPHABODY", &["BETABODY", "GAMMABODY"]);
        if count > 1 {
            page.assert_owner("here-beta", "BETABODY", &["ALPHABODY", "GAMMABODY"]);
            assert_ne!(page.link("here-alpha"), page.link("here-beta"));
        }
        if count > 2 {
            page.assert_owner("here-gamma", "GAMMABODY", &["ALPHABODY", "BETABODY"]);
            assert!(page.visible.contains("A prose mention of --term"));
            assert_eq!(page.link_count("options"), 0);
        }
        assert_eq!(page.link("web"), "https://example.test/#option-term");
        assert_eq!(page.link("relative"), "peer.md#option-term");
        assert_eq!(
            page.links
                .iter()
                .find(|link| link.label == "here-alpha")
                .unwrap()
                .title,
            "same destination"
        );
        // Manual wrappers follow the regular fragment encoder on this surface.
        assert!(page.visible.contains("manual"));
        assert_eq!(page.links.iter().any(|l| l.target == "man:printf(1)"), {
            let clean = mant_render::render_scope_explanation_markdown(&report);
            Page::read(&clean)
                .links
                .iter()
                .any(|l| l.target == "man:printf(1)")
        });
        let clean = mant_render::render_scope_explanation_markdown(&report);
        let clean = Page::read(&clean);
        assert!(clean.anchors.is_empty());
        assert_eq!(clean.local_links(), 0);
        assert_eq!(clean.visible, page.visible);
        let plain = mant_render::render_scope_explanation_text(&report);
        let decorated = mant_render::render_scope_explanation_text_with(&report, |_, text| {
            if text.is_empty() {
                String::new()
            } else {
                format!("\x1b[34m{text}\x1b[0m")
            }
        });
        assert_eq!(
            decorated.replace("\x1b[34m", "").replace("\x1b[0m", ""),
            plain
        );
        assert!(!plain.contains("](#"));
        assert!(plain.contains("ALPHABODY"));
    }
}

#[test]
fn only_materialized_owners_contribute_page_targets() {
    let sources = [
        source(ALPHA, "alpha", MarkdownOrigin::Documents),
        source(BETA, "beta", MarkdownOrigin::Documents),
        source(GAMMA, "gamma", MarkdownOrigin::Documents),
    ];
    for (offset, name, owner) in [
        (0, "here-alpha", "ALPHABODY"),
        (1, "here-beta", "BETABODY"),
        (2, "here-gamma", "GAMMABODY"),
    ] {
        let report = explain(
            &sources,
            ExplanationOptions {
                offset,
                limit: 1,
                ..Default::default()
            },
        );
        assert_eq!(report.counts.direct_entry.returned, 1);
        let markdown =
            mant_render::render_scope_explanation_markdown_with_options(&report, OPTIONS);
        let page = Page::read(&markdown);
        assert_eq!(page.anchors.len(), 1);
        assert_eq!(page.local_links(), 1);
        page.assert_owner(name, owner, &[]);
        assert_eq!(page.link_count("elsewhere"), 0);
    }
    let tiny = explain(
        &sources,
        ExplanationOptions {
            content_bytes: 64,
            ..Default::default()
        },
    );
    assert!(tiny.truncation.content);
    let markdown = mant_render::render_scope_explanation_markdown_with_options(&tiny, OPTIONS);
    let page = Page::read(&markdown);
    assert!(page.anchors.is_empty());
    assert_eq!(page.local_links(), 0);
}

#[test]
fn shared_parent_and_child_context_is_emitted_once_on_the_page() {
    const NESTED: &str = concat!(
        "# Same title\n\n## Options\n\n",
        "- `--term`: PARENTBODY [down](#destination-child)\n",
        "  - `--term`: CHILDBODY [up](#destination-parent) [child-self](#destination-child)\n",
    );
    let mut nested = mant_loader::load_markdown_text(NESTED, None).unwrap();
    nested.address = Some(DocumentAddress::Markdown {
        path: "nested".into(),
        origin: MarkdownOrigin::Documents,
    });
    let document = nested.document.as_mut().unwrap();
    let mant_ir::Block::List { items, .. } = &mut document.sections[0].blocks[0] else {
        panic!("outer list");
    };
    let [
        mant_ir::Block::Paragraph { children, .. },
        mant_ir::Block::List { items: inner, .. },
    ] = items[0].blocks.as_mut_slice()
    else {
        panic!("one nested entry")
    };
    children.push(mant_ir::Inline::anchor("destination-parent"));
    let mant_ir::Block::Paragraph { children, .. } = &mut inner[0].blocks[0] else {
        panic!("child body");
    };
    children.push(mant_ir::Inline::anchor("destination-child"));
    let alias = "É.Exact&%20";
    let alias_marker = mant_ir::Inline::anchor_with_aliases(
        "alias-target",
        vec![
            alias.into(),
            "mant-report-0-64657374696e6174696f6e2d6368696c64".into(),
        ],
    );
    children.push(mant_ir::Inline::Link {
        target: LinkTarget::Section {
            id: "alias-target".into(),
        },
        title: None,
        children: vec![
            alias_marker,
            mant_ir::Inline::Text {
                value: "authored-alias".into(),
            },
        ],
    });
    assert_eq!(mant_ir::validate_document(document), []);

    let sources = [nested, source(BETA, "beta", MarkdownOrigin::Documents)];
    let report = explain(&sources, ExplanationOptions::default());
    assert_eq!(report.counts.direct_entry.returned, 3);
    assert_eq!(report.documents[0].supports.len(), 1);
    let markdown = mant_render::render_scope_explanation_markdown_with_options(&report, OPTIONS);
    let page = Page::read(&markdown);
    assert_eq!(page.visible.matches("PARENTBODY").count(), 1, "{markdown}");
    assert_eq!(page.visible.matches("CHILDBODY").count(), 1, "{markdown}");
    for label in ["up", "down", "child-self", "authored-alias"] {
        page.assert_owner(label, "PARENTBODY", &["BETABODY"]);
    }
    assert_eq!(page.link("down"), page.link("child-self"));
    assert_ne!(page.link("down"), page.link("up"));
    assert_ne!(page.link("down"), page.link("authored-alias"));
    assert_eq!(page.source_anchors(0, alias), 1);
    assert_eq!(
        page.source_anchors(0, "mant-report-0-64657374696e6174696f6e2d6368696c64"),
        1
    );

    let report = explain(
        &sources,
        ExplanationOptions {
            offset: 1,
            limit: 1,
            ..Default::default()
        },
    );
    let markdown = mant_render::render_scope_explanation_markdown_with_options(&report, OPTIONS);
    let page = Page::read(&markdown);
    assert_eq!(page.link_count("up"), 0);
    assert!(page.visible.contains("up"));
    assert!(!page.visible.contains("PARENTBODY"));
    page.assert_owner("child-self", "CHILDBODY", &["BETABODY"]);
    page.assert_owner("authored-alias", "CHILDBODY", &["BETABODY"]);
}
