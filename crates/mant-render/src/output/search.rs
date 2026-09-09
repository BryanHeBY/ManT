//! Presents structure-aware search results for terminals and language models.

use mant_ir::EntryKind;
use std::ops::Range;

use mant_protocol::{OutlineNodeReference, OutlineTrail, QuerySearch};

mod context;
mod line;
mod markdown;
use context::{
    context_group_end, merged_context, occurrence_line_ranges, text_group_coordinates,
    truncated_occurrence_summary,
};
use line::render_search_line;
pub use markdown::render_search_markdown;

/// Semantic roles in the grep-like search presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchTextRole {
    /// Ordinary prose and punctuation.
    Plain,
    /// The logical document label.
    Document,
    /// A rendered Markdown coordinate or result count.
    Coordinate,
    /// A stable semantic-node path.
    Path,
    /// A document, section, or tldr node title.
    Heading,
    /// A semantic entry title.
    Definition(EntryKind),
    /// Text that matched the search query.
    Match,
    /// Secondary guides and context markers.
    Muted,
}

#[cfg(test)]
fn render_search_line_text(markdown: &str) -> String {
    render_search_line(markdown, &[]).0
}

/// Render grep-like results with stable Markdown coordinates and node paths.
#[must_use]
pub fn render_search_text(search: &QuerySearch) -> String {
    render_search_text_with(search, |_, value| value.to_owned())
}

/// Render grep-like search text through a semantic span decorator.
///
/// The callback may add terminal styling around a span, but must preserve its
/// visible text. This keeps layout, Markdown projection, and match boundaries
/// identical between coloured and uncoloured frontends.
#[must_use]
pub fn render_search_text_with(
    search: &QuerySearch,
    decorate: impl FnMut(SearchTextRole, &str) -> String,
) -> String {
    let label = document_label(search);
    let mut output = SearchTextRenderer::new(decorate);
    if search.total == 0 {
        output.plain("No matches for \"");
        output.push(SearchTextRole::Match, &search.query.pattern);
        output.plain("\" in ");
        output.push(SearchTextRole::Document, &label);
        output.plain(".");
        return output.finish();
    }
    if search.matches.is_empty() {
        output.plain("No matching lines returned at offset ");
        output.push(SearchTextRole::Coordinate, &search.offset.to_string());
        output.plain(" for \"");
        output.push(SearchTextRole::Match, &search.query.pattern);
        output.plain("\" in ");
        output.push(SearchTextRole::Document, &label);
        output.plain(" (");
        output.push(SearchTextRole::Coordinate, &search.total.to_string());
        output.plain(" total).");
        return output.finish();
    }

    let mut previous_outline = None;
    let mut index = 0;
    while index < search.matches.len() {
        let found = &search.matches[index];
        if previous_outline != Some(&found.outline) {
            if index > 0 {
                output.line();
                output.line();
            }
            output.push(SearchTextRole::Document, &label);
            output.plain("  ");
            render_outline_trail(&mut output, &found.outline);
        }
        let end = context_group_end(&search.matches, index);
        let group = &search.matches[index..end];
        output.line();
        output.plain("  ");
        output.push(
            SearchTextRole::Coordinate,
            &text_group_coordinates(group, search.query.scope),
        );
        if let Some(summary) = truncated_occurrence_summary(group) {
            output.plain("  [");
            output.push(SearchTextRole::Muted, &summary);
            output.plain("]");
        }
        if found.context.is_empty() {
            output.plain("  ");
            let line = found
                .occurrences
                .first()
                .map_or(0, |occurrence| occurrence.markdown.start_line);
            let ranges = occurrence_line_ranges(found, line);
            let (visible, highlights) = render_search_line(&found.preview, &ranges);
            output.matching_line(&visible, highlights);
        } else {
            for (line_number, (text, matched, source_ranges)) in merged_context(group) {
                output.line();
                output.plain("    ");
                output.push(
                    if matched {
                        SearchTextRole::Match
                    } else {
                        SearchTextRole::Muted
                    },
                    if matched { ">" } else { " " },
                );
                output.plain(" ");
                output.push(SearchTextRole::Coordinate, &line_number.to_string());
                output.plain(" ");
                let (visible, highlights) = render_search_line(text, &source_ranges);
                if matched {
                    output.matching_line(&visible, highlights);
                } else {
                    output.plain(&visible);
                }
            }
        }
        previous_outline = Some(&found.outline);
        index = end;
    }
    if let Some(next_offset) = search.next_offset {
        output.line();
        output.line();
        output.push(SearchTextRole::Coordinate, &search.total.to_string());
        output.plain(" total matching lines; continue with ");
        output.push(SearchTextRole::Heading, "--offset");
        output.plain(" ");
        output.push(SearchTextRole::Coordinate, &next_offset.to_string());
        output.plain(".");
    }
    output.finish()
}

struct SearchTextRenderer<F> {
    rendered: String,
    decorate: F,
}

impl<F> SearchTextRenderer<F>
where
    F: FnMut(SearchTextRole, &str) -> String,
{
    fn new(decorate: F) -> Self {
        Self {
            rendered: String::new(),
            decorate,
        }
    }

    fn plain(&mut self, value: &str) {
        self.push(SearchTextRole::Plain, value);
    }

    fn push(&mut self, role: SearchTextRole, value: &str) {
        self.rendered.push_str(&(self.decorate)(role, value));
    }

    fn line(&mut self) {
        self.rendered.push('\n');
    }

    fn matching_line(&mut self, line: &str, matched: impl IntoIterator<Item = Range<usize>>) {
        let mut ranges = matched
            .into_iter()
            .filter(|range| {
                range.start < range.end
                    && range.end <= line.len()
                    && line.is_char_boundary(range.start)
                    && line.is_char_boundary(range.end)
            })
            .map(|range| (range.start, range.end))
            .collect::<Vec<_>>();
        if ranges.is_empty() {
            self.plain(line);
            return;
        }
        ranges.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            if let Some((_, previous_end)) = merged.last_mut().filter(|(_, end)| start <= *end) {
                *previous_end = (*previous_end).max(end);
            } else {
                merged.push((start, end));
            }
        }
        let mut position = 0;
        for (start, end) in merged {
            self.plain(&line[position..start]);
            self.push(SearchTextRole::Match, &line[start..end]);
            position = end;
        }
        self.plain(&line[position..]);
    }

    fn finish(self) -> String {
        self.rendered.trim_end().to_owned()
    }
}

fn render_outline_trail<F>(output: &mut SearchTextRenderer<F>, trail: &OutlineTrail)
where
    F: FnMut(SearchTextRole, &str) -> String,
{
    output.push(SearchTextRole::Muted, "Outline ");
    output.push(SearchTextRole::Path, trail.path());
    output.push(SearchTextRole::Muted, ": ");
    for (index, ancestor) in trail.ancestors.iter().enumerate() {
        if index > 0 {
            output.push(SearchTextRole::Muted, " > ");
        }
        output.push(SearchTextRole::Heading, &ancestor.title);
    }
    if !trail.ancestors.is_empty() {
        output.push(SearchTextRole::Muted, " > ");
    }
    output.push(search_node_role(&trail.node), trail.title());
}

const fn search_node_role(node: &OutlineNodeReference) -> SearchTextRole {
    match node {
        OutlineNodeReference::DocumentEntry { entry_kind, .. } => {
            SearchTextRole::Definition(*entry_kind)
        }
        OutlineNodeReference::Tldr { .. }
        | OutlineNodeReference::DocumentRoot { .. }
        | OutlineNodeReference::DocumentSection { .. } => SearchTextRole::Heading,
    }
}

fn document_label(search: &QuerySearch) -> String {
    search
        .meta
        .as_ref()
        .and_then(|meta| meta.manual_section.as_deref())
        .map_or_else(
            || search.label.clone(),
            |section| format!("{}({section})", search.label),
        )
}

#[cfg(test)]
mod tests {
    use mant_protocol::{
        MarkdownSchema, OutlineNodeReference, OutlineReference, OutlineTrail, QuerySearch,
        SearchCase, SearchContextLine, SearchHit, SearchLineRange, SearchMarkdownRange,
        SearchOccurrence, SearchQuery, SearchRender, SearchRenderFormat, SearchRenderScope,
        SearchSchema, SearchScope, SearchSyntax,
    };
    use pulldown_cmark::{Event, Parser};

    use super::{
        SearchTextRenderer, SearchTextRole, render_search_line_text, render_search_markdown,
        render_search_text, render_search_text_with,
    };

    fn result() -> QuerySearch {
        QuerySearch {
            schema: SearchSchema::V0Dot11,
            label: "tar".to_owned(),
            source: None,
            meta: Some(mant_ir::DocumentMeta {
                manual_section: Some("1".to_owned()),
                ..mant_ir::DocumentMeta::default()
            }),
            query: SearchQuery {
                pattern: "--acls".to_owned(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Insensitive,
                scope: SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 100,
                offset: 0,
            },
            render: SearchRender {
                schema: MarkdownSchema::V1,
                format: SearchRenderFormat::Markdown,
                scope: SearchRenderScope::Full,
                line_base: 1,
                column_base: 1,
                line_count: 900,
            },
            total: 1,
            returned: 1,
            offset: 0,
            truncated: false,
            next_offset: None,
            matches: vec![SearchHit {
                ordinal: 1,
                outline: OutlineTrail {
                    ancestors: vec![OutlineReference {
                        path: "5.3".to_owned().into(),
                        id: "archive-options".to_owned().into(),
                        title: "Archive options".to_owned(),
                    }],
                    node: OutlineNodeReference::DocumentEntry {
                        path: "5.3/e17".to_owned().into(),
                        id: "acls-option".to_owned().into(),
                        title: "--acls".to_owned(),
                        entry_kind: mant_ir::EntryKind::Parameter {
                            parameter_kind: mant_ir::ParameterKind::Option,
                        },
                        case: mant_ir::NameCase::Sensitive,
                        names: vec!["--acls".to_owned()],
                    },
                },
                occurrences: vec![SearchOccurrence {
                    matched_text: "--acls".to_owned(),
                    markdown: SearchMarkdownRange {
                        start_byte: 10,
                        end_byte: 16,
                        start_line: 824,
                        start_column: 3,
                        end_line: 824,
                        end_column: 9,
                    },
                    line_ranges: vec![SearchLineRange {
                        line: 824,
                        start_byte: 3,
                        end_byte: 9,
                    }],
                }],
                occurrence_count: 1,
                occurrences_truncated: false,
                node_source: None,
                preview: "- `--acls`".to_owned(),
                context: Vec::new(),
            }],
        }
    }

    #[test]
    fn search_reports_are_human_readable_but_keep_machine_node_paths() {
        let result = result();
        assert!(
            render_search_text(&result)
                .contains("tar(1)  Outline 5.3/e17: Archive options > --acls\n  824:1  --acls")
        );
        assert!(render_search_text(&result).contains("  --acls"));
        assert!(!render_search_text(&result).contains("`--acls`"));
        let markdown = render_search_markdown(&result);
        assert!(markdown.contains("# Search results for `--acls` in tar(1)"));
        assert!(markdown.contains("- Outline: `5.3/e17`"));
        assert!(markdown.contains("- Trail: `Archive options` → `--acls`"));
    }

    #[test]
    fn search_markdown_uses_canonical_commonmark_escaping() {
        let mut result = result();
        result.query.pattern = "`ticked start".to_owned();
        result.label = "a|b~c^d:e".to_owned();
        result.matches[0].outline.node = OutlineNodeReference::DocumentSection {
            path: "1".to_owned().into(),
            id: "ticked".to_owned().into(),
            title: "`ticked start".to_owned(),
        };

        let markdown = render_search_markdown(&result);
        assert!(markdown.contains("`` `ticked start ``"), "{markdown}");
        assert!(markdown.contains("a\\|b\\~c\\^d\\:e"), "{markdown}");
        assert_eq!(
            Parser::new(&markdown)
                .filter_map(|event| match event {
                    Event::Code(value) => Some(value.into_string()),
                    _ => None,
                })
                .filter(|value| value == "`ticked start")
                .count(),
            3
        );
    }

    #[test]
    fn text_search_coordinates_follow_the_presented_scope() {
        let mut visible = result();
        visible.matches[0].occurrences[0].markdown.start_column = 35;
        assert!(render_search_text(&visible).contains("  824:1  --acls"));

        visible.query.scope = SearchScope::Markdown;
        assert!(render_search_text(&visible).contains("  824:35  --acls"));
    }

    #[test]
    fn search_text_lines_hide_markdown_presentation_syntax() {
        assert_eq!(
            render_search_line_text("- **Use** [`mant`](https://example.test) with `--color`."),
            "Use mant with --color."
        );
    }

    #[test]
    fn semantic_search_text_marks_only_the_visible_match() {
        let rendered = render_search_text_with(&result(), |role, value| {
            if role == SearchTextRole::Match {
                format!("<match>{value}</match>")
            } else {
                value.to_owned()
            }
        });

        assert!(rendered.contains("  <match>--acls</match>"));
        assert!(!rendered.contains("<match>  --acls</match>"));
        assert!(!rendered.contains('`'));
    }

    #[test]
    fn semantic_search_text_does_not_rehighlight_nonmatching_substrings() {
        let mut result = result();
        result.query.pattern = "foo".to_owned();
        result.matches[0].preview = "foobar foo".to_owned();
        result.matches[0].occurrences[0].matched_text = "foo".to_owned();
        result.matches[0].occurrences[0].markdown.start_line = 824;
        result.matches[0].occurrences[0].markdown.end_line = 824;
        result.matches[0].occurrences[0].line_ranges = vec![SearchLineRange {
            line: 824,
            start_byte: 7,
            end_byte: 10,
        }];

        let rendered = render_search_text_with(&result, |role, value| {
            if role == SearchTextRole::Match {
                format!("<match>{value}</match>")
            } else {
                value.to_owned()
            }
        });

        assert!(rendered.contains("foobar <match>foo</match>"));
        assert!(!rendered.contains("<match>foo</match>bar"));
    }

    #[test]
    fn matching_lines_ignore_ranges_inside_utf8_characters() {
        let mut renderer = SearchTextRenderer::new(|_, value| value.to_owned());

        renderer.matching_line("é", std::iter::once(1..2));

        assert_eq!(renderer.finish(), "é");
    }

    #[test]
    fn search_text_groups_adjacent_matches_by_exact_outline_node() {
        let mut result = result();
        result.matches[0].preview = "- `--acls` and `--acls`".to_owned();
        let mut same_line = result.matches[0].occurrences[0].clone();
        same_line.markdown.start_column = 14;
        same_line.markdown.end_column = 20;
        same_line.line_ranges[0].start_byte = 16;
        same_line.line_ranges[0].end_byte = 22;
        result.matches[0].occurrences.push(same_line);
        let mut second = result.matches[0].clone();
        second.ordinal = 2;
        second.occurrences.truncate(1);
        second.occurrences[0].markdown.start_line = 825;
        second.occurrences[0].markdown.end_line = 825;
        second.occurrences[0].markdown.start_column = 7;
        second.occurrences[0].markdown.end_column = 13;
        second.occurrences[0].line_ranges[0].line = 825;
        second.occurrences[0].line_ranges[0].start_byte = 3;
        second.occurrences[0].line_ranges[0].end_byte = 9;
        second.preview = "- `--acls`".to_owned();

        let mut third = second.clone();
        third.ordinal = 3;
        third.occurrences[0].markdown.start_line = 900;
        third.occurrences[0].markdown.end_line = 900;
        third.outline.node = OutlineNodeReference::DocumentSection {
            path: "6".to_owned().into(),
            id: "examples".to_owned().into(),
            title: "Examples".to_owned(),
        };
        third.outline.ancestors.clear();

        result.total = 3;
        result.returned = 3;
        result.matches.extend([second, third]);
        let rendered = render_search_text(&result);

        assert_eq!(rendered.matches("Outline 5.3/e17").count(), 1);
        assert!(rendered.contains("  824:1,12  --acls and --acls\n  825:1  --acls"));
        assert_eq!(rendered.matches("Outline 6: Examples").count(), 1);
        assert!(rendered.contains("\n\ntar(1)  Outline 6: Examples\n  900:7  --acls"));
    }

    #[test]
    fn search_text_merges_overlapping_context_windows() {
        let mut result = result();
        result.matches[0].context = vec![
            context(823, "before", false),
            context(824, "first --acls", true),
            context(825, "between", false),
        ];
        result.matches[0].occurrences[0].line_ranges[0].start_byte = 6;
        result.matches[0].occurrences[0].line_ranges[0].end_byte = 12;
        let mut second = result.matches[0].clone();
        second.ordinal = 2;
        second.occurrences[0].markdown.start_line = 826;
        second.occurrences[0].markdown.end_line = 826;
        second.occurrences[0].markdown.start_column = 8;
        second.occurrences[0].line_ranges[0].line = 826;
        second.occurrences[0].line_ranges[0].start_byte = 7;
        second.occurrences[0].line_ranges[0].end_byte = 13;
        second.context = vec![
            context(825, "between", false),
            context(826, "second --acls", true),
            context(827, "after", false),
        ];
        result.total = 2;
        result.returned = 2;
        result.matches.push(second);

        let rendered = render_search_text(&result);

        assert!(rendered.contains("  824:7; 826:8"));
        assert_eq!(rendered.matches(" 825 between").count(), 1);
        assert_eq!(rendered.matches(" 824 first --acls").count(), 1);
        assert_eq!(rendered.matches(" 826 second --acls").count(), 1);
    }

    fn context(line: u32, text: &str, matched: bool) -> SearchContextLine {
        SearchContextLine {
            line,
            text: text.to_owned(),
            matched,
        }
    }
}
