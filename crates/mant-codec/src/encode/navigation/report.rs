//! Accepted local destinations in a composed Markdown report.
use crate::encode::anchors::markers_in_html;
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::{collections::HashSet, ops::Range};

/// Prepare and encode one bounded page of source-qualified Markdown fragments.
///
/// The callback encodes the same already-projected page using
/// [`crate::encode::ScopedMarkdownFragments::in_report_scope`]. It receives
/// the IDs of actual anchors when missing link destinations require a new
/// pass. This accounts for output decisions such as fenced carriers, inline
/// fallback and repeated supports without indexing omitted source content.
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
        let mut links = HashSet::new();
        for (event, range) in Parser::new_ext(markdown, Options::ENABLE_MATH).into_offset_iter() {
            match event {
                Event::Start(Tag::Link { dest_url, .. }) => {
                    if let Some(id) = dest_url.strip_prefix('#') {
                        links.insert(id.to_owned());
                    }
                }
                Event::Html(_) | Event::InlineHtml(_) => {
                    for anchor in markers_in_html(markdown, range, "<span id=\"", "\"></span>") {
                        if !anchors.insert(anchor.id.to_owned()) {
                            duplicates.push(anchor.range);
                        }
                    }
                }
                _ => {}
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

    #[test]
    fn accepted_markers_deduplicate_raw_html_and_keep_literal_lookalikes() {
        let marker = r#"<span id="scope-id"></span>"#;
        let markdown = format!(
            "> <br />\n> {marker}{marker}\n\n> [jump](#scope-id) `{marker}`\n\n> ```\n> {marker}\n> ```"
        );
        let navigation = PageNavigation::scan(&markdown);
        assert!(navigation.complete);
        assert_eq!(navigation.anchors, HashSet::from(["scope-id".into()]));
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
}
