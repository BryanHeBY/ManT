//! Accepted local destinations in a composed Markdown report.
use std::{collections::HashSet, ops::Range};

mod html;

/// Prepare and encode one bounded page of source-qualified Markdown fragments.
///
/// The callback encodes the same already-projected page using
/// [`crate::encode::ScopedMarkdownFragments::in_report_scope`]. It receives
/// the IDs of actual anchors when missing link destinations require a new
/// pass. This accounts for output decisions such as fenced carriers, inline
/// fallback and repeated supports without indexing omitted source content.
/// The generated ID namespace is reserved for scoped fragments; host HTML
/// using the same ID on a different element does not authorize an ambiguous
/// local link. Host markup must not imitate the exact generated empty span.
#[must_use]
pub fn render_scoped_report_page(render: impl Fn(Option<&HashSet<String>>) -> String) -> String {
    let page = render(None);
    let navigation = PageNavigation::scan(&page);
    if navigation.complete {
        return navigation.unique(page);
    }

    // Omit unresolvable link wrappers inside the fragment encoder so its
    // phrasing and hard-row rules still apply to the visible link labels.
    let page = render(Some(&navigation.anchors));
    let navigation = PageNavigation::scan(&page);
    if navigation.complete {
        return navigation.unique(page);
    }

    // A changed Markdown seam can make an anchor disappear when wrappers
    // are omitted. In that case publish the page with local links inactive.
    let page = render(Some(&HashSet::new()));
    PageNavigation::scan(&page).unique(page)
}

struct PageNavigation {
    anchors: HashSet<String>,
    duplicates: Vec<Range<usize>>,
    complete: bool,
}

impl PageNavigation {
    fn scan(markdown: &str) -> Self {
        let mut anchors = HashSet::new();
        let mut duplicates = Vec::new();
        let (markers, links) = html::report_navigation(markdown);
        for marker in markers {
            if !anchors.insert(marker.id.to_owned()) {
                duplicates.push(marker.range);
            }
        }
        Self {
            complete: links.is_subset(&anchors),
            anchors,
            duplicates,
        }
    }

    fn unique(self, markdown: String) -> String {
        if self.duplicates.is_empty() {
            return markdown;
        }
        let mut output = String::with_capacity(markdown.len());
        let mut cursor = 0;
        // Keep the first accepted destination for repeated quotations.
        // Preserve zero-width HTML framing so neighboring delimiters do
        // not coalesce when the duplicate identifier disappears.
        for duplicate in self.duplicates {
            output.push_str(&markdown[cursor..duplicate.start]);
            output.push_str("<span></span>");
            cursor = duplicate.end;
        }
        output.push_str(&markdown[cursor..]);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulldown_cmark::{Event, Parser};

    #[test]
    fn accepted_markers_deduplicate_raw_html_and_keep_literal_lookalikes() {
        let marker = r#"<span id="mant-report-0-61"></span>"#;
        let markdown = format!(
            "> <br />\n> {marker}{marker}\n\n> [jump](#mant-report-0-61) `{marker}`\n\n> ```\n> {marker}\n> ```"
        );
        let navigation = PageNavigation::scan(&markdown);
        assert!(navigation.complete);
        assert_eq!(
            navigation.anchors,
            HashSet::from(["mant-report-0-61".into()])
        );
        assert_eq!(navigation.duplicates.len(), 1);
        let markdown = navigation.unique(markdown);
        assert!(markdown.contains(&format!("{marker}<span></span>")));
        assert_eq!(
            Parser::new(&markdown)
                .filter(|event| matches!(event, Event::Code(value) if value.as_ref() == marker))
                .count(),
            1
        );
        assert!(markdown.contains(&format!("> ```\n> {marker}\n> ```")));
    }

    #[test]
    fn hidden_markup_cannot_keep_a_scoped_link_or_remove_its_visible_target() {
        let marker = r#"<span id="mant-report-0-61"></span>"#;
        let hidden = format!("before <!-- {marker} --> <i title='{marker}'>copy</i>");
        let missing = render_scoped_report_page(|anchors| {
            let label = if let Some(anchors) = anchors {
                assert!(!anchors.contains("mant-report-0-61"));
                "jump"
            } else {
                "[jump](#mant-report-0-61)"
            };
            format!("{hidden} {label}")
        });
        assert_eq!(missing, format!("{hidden} jump"));
        let visible =
            format!("{hidden}\n\n> <br />\n> {marker}{marker}\n\n[jump](#mant-report-0-61)");
        let retained = render_scoped_report_page(|anchors| {
            assert!(anchors.is_none());
            visible.clone()
        });
        assert_eq!(
            retained,
            format!("{hidden}\n\n> <br />\n> {marker}<span></span>\n\n[jump](#mant-report-0-61)")
        );
    }

    #[test]
    fn report_navigation_observes_html_token_boundaries_and_foreign_elements() {
        const ID: &str = "mant-report-0-61";
        const MARKER: &str = r#"<span id="mant-report-0-61"></span>"#;
        for leading in [
            "<div>\n<textarea>note\n</textarea\n>\n</div>".to_owned(),
            "<div>\n<textarea>note\n</textarea data-ignored=\"a>\n b\"\n>\n</div>".to_owned(),
            "<svg><title/></svg>".to_owned(),
            "<svg><title></title></svg>".to_owned(),
            format!("<svg><![CDATA[{MARKER}]]></svg>"),
            format!("<template><div><template>{MARKER}</template></div></template>"),
            format!("<div>\n<!--\n\n`-->`\n\n<div>\n{MARKER}\n-->\n</div>"),
            format!("<svg><foreignObject><textarea>{MARKER}</textarea></foreignObject></svg>"),
        ] {
            let markdown = format!("{leading}\n\n{MARKER}{MARKER}\n\n[jump](#{ID})");
            let navigation = PageNavigation::scan(&markdown);
            assert!(navigation.complete, "{leading}");
            assert_eq!(
                navigation.anchors,
                HashSet::from([ID.to_owned()]),
                "{leading}"
            );
            assert_eq!(navigation.duplicates.len(), 1, "{leading}");
            let last_marker = markdown.rfind(MARKER).unwrap();
            assert_eq!(
                navigation.duplicates[0],
                last_marker..last_marker + MARKER.len()
            );
            let rendered = render_scoped_report_page(|anchors| {
                assert!(anchors.is_none(), "{leading}");
                markdown.clone()
            });
            assert_eq!(
                rendered,
                format!("{leading}\n\n{MARKER}<span></span>\n\n[jump](#{ID})")
            );
        }
    }

    #[test]
    fn inert_host_links_do_not_remove_a_reachable_scoped_jump() {
        let host = "<template>\n\n[decoy](#missing)\n\n</template>";
        let marker = r#"<span id="mant-report-0-61"></span>"#;
        let markdown = format!("{host}\n\n{marker}\n\n[jump](#mant-report-0-61)");
        let rendered = render_scoped_report_page(|anchors| {
            assert!(anchors.is_none());
            markdown.clone()
        });
        assert_eq!(rendered, markdown);
    }

    #[test]
    fn a_conflicting_host_id_is_not_rewritten_or_used_as_the_scoped_target() {
        const ID: &str = "mant-report-0-61";
        let host = format!("<div id='{ID}'>Host</div>");
        let marker = r#"<span id="mant-report-0-61"></span>"#;
        let markdown = render_scoped_report_page(|anchors| {
            let jump = if anchors.is_none_or(|ids| ids.contains(ID)) {
                "[jump](#mant-report-0-61)"
            } else {
                "jump"
            };
            format!("{host}\n\n{marker}\n\n{jump}")
        });
        assert_eq!(markdown, format!("{host}\n\n{marker}\n\njump"));
    }

    #[test]
    fn host_navigation_and_html_are_not_scoped_or_deduplicated() {
        const HOST: &str = r#"<div id="contents">Contents</div>

[Contents](#contents)

<span id="details"></span><span id="details"></span>"#;
        const MARKER: &str = r#"<span id="mant-report-0-61"></span>"#;
        let markdown = format!("{HOST}\n\n{MARKER}{MARKER}\n\n[jump](#mant-report-0-61)");
        assert_eq!(
            render_scoped_report_page(|anchors| {
                assert!(anchors.is_none());
                markdown.clone()
            }),
            format!("{HOST}\n\n{MARKER}<span></span>\n\n[jump](#mant-report-0-61)")
        );
    }

    #[test]
    fn decoded_host_href_cannot_stand_in_for_a_markdown_link_receipt() {
        const HOST: &str = r#"<a href="&#35;data-mant-report-probe&#45;0-end-link-0">host</a>

<template>

[decoy](#mant-report-0-62)

</template>"#;
        const MARKER: &str = r#"<span id="mant-report-0-61"></span>"#;
        let markdown = format!("{HOST}\n\n{MARKER}\n\n[jump](#mant-report-0-61)");
        assert_eq!(
            render_scoped_report_page(|anchors| {
                assert!(anchors.is_none());
                markdown.clone()
            }),
            markdown
        );
    }

    #[test]
    fn rendered_link_receipts_cannot_be_borrowed_from_host_hrefs() {
        const INERT: &str = "<template>\n\n[decoy](#mant-report-0-62)\n\n</template>";
        const MARKER: &str = r#"<span id="mant-report-0-61"></span>"#;
        const HOST_DEST: &str = "#data-mant-report-probe-0-end-link-0";
        for host in [
            r"[host](#data\-mant-report-probe-0-end-link-0)",
            "[host][ref]\n\n[ref]: <&#35;data-mant-report-probe-0-end-link-0>",
            r##"<a data-mant-report-probe-0-end=l0 href="#data-mant-report-probe-0-end-link-0">host</a>"##,
            r#"<a DATA-MANT-REPORT-PROBE-0-END=l0 href="&#35;data-mant-report-probe-0-end-link-0">host</a>"#,
        ] {
            let markdown = format!("{host}\n\n{INERT}\n\n{MARKER}\n\n[jump](#mant-report-0-61)");
            let (_, links) = html::report_navigation(&markdown);
            assert_eq!(links, HashSet::from(["mant-report-0-61".to_owned()]));
            let out = render_scoped_report_page(|scope| {
                assert!(scope.is_none(), "{host}");
                markdown.clone()
            });
            assert_eq!(out, markdown, "{host}");
            // Host source is byte-identical; its normalized Markdown
            // destination remains external to the generated report namespace.
            if host.starts_with('[') {
                assert!(Parser::new(&out).any(|event| matches!(
                    event,
                    Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) if dest_url.as_ref() == HOST_DEST
                )));
            }
        }
    }

    #[test]
    fn at_report_keeps_reachable_jump_under_equivalent_host_markdown() {
        // AT frozen report DOM matrix: host spelling × inert context ×
        // link syntax × unrelated ampersand. Every case has one real jump.
        const MARKER: &str = r#"<span id="mant-report-0-61"></span>"#;
        for host in [
            "#data-mant-report-probe-0-end-link-0",
            r"#data\-mant-report-probe-0-end-link-0",
            r"#data-mant\-report-probe-0-end-link-0",
            r"\#data\-mant-report-probe-0-end-link-0",
            "#data&#45;mant-report-probe-0-end-link-0",
            "#d&#97;ta-mant-report-probe-0-end-link-0",
            "#contents",
            r"#data\-mant-report-probe-0-end-link-1",
        ] {
            for hidden in [
                "<template>\n\n[decoy](#mant-report-0-62)\n\n</template>",
                "<template><div>\n\n[decoy](#mant-report-0-62)\n\n</div></template>",
                "`[decoy](#mant-report-0-62)`",
                "ordinary paragraph",
            ] {
                for host_link in [
                    format!("[host]({host})"),
                    format!("[host][ref]\n\n[ref]: {host}"),
                    format!("[host](<{host}>)"),
                ] {
                    for extra in ["", "\n\nA & B"] {
                        let page = format!(
                            "{host_link}\n\n{hidden}{extra}\n\n{MARKER}\n\n[jump](#mant-report-0-61)"
                        );
                        assert_eq!(
                            render_scoped_report_page(|anchors| {
                                assert!(anchors.is_none(), "{page}");
                                page.clone()
                            }),
                            page
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn fenced_closing_spelling_cannot_steal_a_visible_scoped_target() {
        let marker = r#"<span id="mant-report-0-61"></span>"#;
        let markdown = format!(
            "Label <textarea>\n\n~~~html\n</textarea>\n~~~\n\n{marker}\n\n</textarea>\n\n{marker}[jump](#mant-report-0-61)"
        );
        let live = crate::encode::anchors::anchor_markers(&markdown);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].range.start, markdown.rfind(marker).unwrap());
        assert_eq!(
            render_scoped_report_page(|anchors| {
                assert!(anchors.is_none());
                markdown.clone()
            }),
            markdown
        );
    }
}
