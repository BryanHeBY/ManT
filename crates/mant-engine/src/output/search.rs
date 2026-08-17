//! Presents structure-aware search results for terminals and language models.

use mant_ir::DefinitionRole;
use std::{collections::BTreeMap, ops::Range};

use mant_protocol::{OutlineNodeReference, OutlineTrail, QuerySearch, SearchMatch};
use pulldown_cmark::{Event, Parser};

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
    Definition(DefinitionRole),
    /// Text that matched the search query.
    Match,
    /// Secondary guides and context markers.
    Muted,
}

#[cfg(test)]
fn render_search_line_text(markdown: &str) -> String {
    render_search_line(markdown, &[]).0
}

fn render_search_line(markdown: &str, highlights: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    let mut rendered = String::with_capacity(markdown.len());
    let mut rendered_highlights = Vec::new();
    for (event, source) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Text(value)
            | Event::Code(value)
            | Event::InlineMath(value)
            | Event::DisplayMath(value) => append_aligned_text(
                markdown,
                &value,
                source,
                highlights,
                &mut rendered,
                &mut rendered_highlights,
            ),
            Event::SoftBreak | Event::HardBreak => {
                let start = rendered.len();
                rendered.push(' ');
                if highlights
                    .iter()
                    .any(|range| ranges_overlap(range, &source))
                {
                    rendered_highlights.push(start..rendered.len());
                }
            }
            Event::TaskListMarker(checked) => {
                rendered.push_str(if checked { "[x] " } else { "[ ] " });
            }
            Event::Rule => rendered.push_str("---"),
            Event::Start(_)
            | Event::End(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_) => {}
        }
    }
    let visible_end = rendered.trim_end().len();
    rendered.truncate(visible_end);
    rendered_highlights.retain(|range| range.start < visible_end);
    for range in &mut rendered_highlights {
        range.end = range.end.min(visible_end);
    }
    (rendered, rendered_highlights)
}

fn append_aligned_text(
    markdown: &str,
    value: &str,
    source: Range<usize>,
    highlights: &[Range<usize>],
    rendered: &mut String,
    rendered_highlights: &mut Vec<Range<usize>>,
) {
    let mut cursor = source.start.min(markdown.len());
    let source_end = source.end.min(markdown.len());
    for character in value.chars() {
        let found = markdown[cursor..source_end]
            .find(character)
            .map_or(cursor, |relative| cursor + relative);
        let character_source = found..found.saturating_add(character.len_utf8()).min(source_end);
        let visible_start = rendered.len();
        rendered.push(character);
        if highlights
            .iter()
            .any(|range| ranges_overlap(range, &character_source))
        {
            rendered_highlights.push(visible_start..rendered.len());
        }
        cursor = character_source.end;
    }
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
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
        output.push(SearchTextRole::Coordinate, &group_coordinates(group));
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
            .filter(|range| range.start < range.end && range.end <= line.len())
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

fn occurrence_line_ranges(found: &SearchMatch, line: u32) -> Vec<Range<usize>> {
    found
        .occurrences
        .iter()
        .flat_map(|occurrence| occurrence.line_ranges.iter())
        .filter(|range| range.line == line)
        .filter_map(|range| {
            Some(usize::try_from(range.start_byte).ok()?..usize::try_from(range.end_byte).ok()?)
        })
        .collect()
}

fn group_coordinates(matches: &[SearchMatch]) -> String {
    let mut lines: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for occurrence in matches.iter().flat_map(|found| found.occurrences.iter()) {
        lines
            .entry(occurrence.markdown.start_line)
            .or_default()
            .push(occurrence.markdown.start_column);
    }
    lines
        .into_iter()
        .map(|(line, mut columns)| {
            columns.sort_unstable();
            columns.dedup();
            format!(
                "{line}:{}",
                columns
                    .into_iter()
                    .map(|column| column.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn truncated_occurrence_summary(matches: &[SearchMatch]) -> Option<String> {
    matches
        .iter()
        .any(|found| found.occurrences_truncated)
        .then(|| {
            let total = matches
                .iter()
                .map(|found| u64::from(found.occurrence_count))
                .sum::<u64>();
            let shown = matches
                .iter()
                .map(|found| found.occurrences.len() as u64)
                .sum::<u64>();
            format!("{total} occurrences; {shown} exact coordinates shown")
        })
}

fn context_group_end(matches: &[SearchMatch], start: usize) -> usize {
    let Some((_, mut last_line)) = context_bounds(&matches[start]) else {
        return start + 1;
    };
    let outline = &matches[start].outline;
    let mut end = start + 1;
    while let Some(found) = matches.get(end) {
        let Some((first_line, found_last_line)) = context_bounds(found) else {
            break;
        };
        if &found.outline != outline || first_line > last_line.saturating_add(1) {
            break;
        }
        last_line = last_line.max(found_last_line);
        end += 1;
    }
    end
}

fn context_bounds(found: &SearchMatch) -> Option<(u32, u32)> {
    Some((found.context.first()?.line, found.context.last()?.line))
}

type MergedContext<'a> = BTreeMap<u32, (&'a str, bool, Vec<Range<usize>>)>;

fn merged_context(matches: &[SearchMatch]) -> MergedContext<'_> {
    let mut merged: MergedContext<'_> = BTreeMap::new();
    for found in matches {
        for line in &found.context {
            let entry = merged
                .entry(line.line)
                .or_insert_with(|| (line.text.as_str(), false, Vec::new()));
            entry.1 |= line.matched;
            if line.matched {
                entry.2.extend(occurrence_line_ranges(found, line.line));
            }
        }
    }
    merged
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
        OutlineNodeReference::DocumentEntry { role, .. } => SearchTextRole::Definition(*role),
        OutlineNodeReference::Tldr { .. }
        | OutlineNodeReference::DocumentRoot { .. }
        | OutlineNodeReference::DocumentSection { .. } => SearchTextRole::Heading,
    }
}

/// Render a readable Markdown report whose coordinates target the full page.
#[must_use]
pub fn render_search_markdown(search: &QuerySearch) -> String {
    let label = document_label(search);
    let mut blocks = vec![format!(
        "# Search results for {} in {}",
        code_span(&search.query.pattern),
        escape_text(&label)
    )];
    blocks.push(format!(
        "{} {} in the full Markdown document.",
        search.total,
        if search.total == 1 {
            "matching line"
        } else {
            "matching lines"
        }
    ));
    if search.returned < search.total {
        if search.returned == 0 {
            blocks.push(format!(
                "No matching lines were returned at offset {}.",
                search.offset
            ));
        } else {
            let range_start = search.offset.saturating_add(1);
            let range_end = search.offset.saturating_add(search.returned);
            let continuation = search
                .next_offset
                .map_or(String::new(), |offset| format!(" Next offset: `{offset}`."));
            blocks.push(format!(
                "Showing matching lines {range_start}–{range_end}.{continuation}"
            ));
        }
    }

    for found in &search.matches {
        blocks.push(format!(
            "## {}. {}",
            found.ordinal,
            code_span(found.outline.title())
        ));
        let mut details = vec![
            format!("- Outline: {}", code_span(found.outline.path())),
            format!(
                "- Trail: {}",
                found
                    .outline
                    .ancestors
                    .iter()
                    .map(|ancestor| code_span(&ancestor.title))
                    .chain(std::iter::once(code_span(found.outline.title())))
                    .collect::<Vec<_>>()
                    .join(" → ")
            ),
            format!(
                "- Markdown: {}",
                group_coordinates(std::slice::from_ref(found))
            ),
        ];
        if let Some(source) = found.source {
            details.push(format!(
                "- Source: line {}, column {}",
                source.line, source.column
            ));
        }
        if found.occurrences_truncated {
            details.push(format!(
                "- Occurrences: {} total; {} exact coordinates shown",
                found.occurrence_count,
                found.occurrences.len()
            ));
        }
        blocks.push(details.join("\n"));
        blocks.push(format!("> {}", found.preview.replace('\n', "\n> ")));
    }
    blocks.join("\n\n").trim_end().to_owned()
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

fn code_span(value: &str) -> String {
    let width = value
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(1);
    let delimiter = "`".repeat(width);
    format!("{delimiter}{value}{delimiter}")
}

fn escape_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

#[cfg(test)]
mod tests {
    use mant_protocol::{
        MarkdownSchema, OutlineNodeReference, OutlineReference, OutlineTrail, QuerySearch,
        SearchCase, SearchContextLine, SearchLineRange, SearchMarkdownRange, SearchMatch,
        SearchOccurrence, SearchQuery, SearchRender, SearchRenderFormat, SearchRenderScope,
        SearchSchema, SearchScope, SearchSyntax,
    };

    use super::{
        SearchTextRole, render_search_line_text, render_search_markdown, render_search_text,
        render_search_text_with,
    };

    fn result() -> QuerySearch {
        QuerySearch {
            schema: SearchSchema::V0Dot8,
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
            matches: vec![SearchMatch {
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
                        role: mant_ir::DefinitionRole::Option,
                        case: mant_ir::DefinitionCase::Sensitive,
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
                source: None,
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
                .contains("tar(1)  Outline 5.3/e17: Archive options > --acls\n  824:3  --acls")
        );
        assert!(render_search_text(&result).contains("  --acls"));
        assert!(!render_search_text(&result).contains("`--acls`"));
        let markdown = render_search_markdown(&result);
        assert!(markdown.contains("# Search results for `--acls` in tar(1)"));
        assert!(markdown.contains("- Outline: `5.3/e17`"));
        assert!(markdown.contains("- Trail: `Archive options` → `--acls`"));
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
    fn search_text_groups_adjacent_matches_by_exact_outline_node() {
        let mut result = result();
        let mut same_line = result.matches[0].occurrences[0].clone();
        same_line.markdown.start_column = 14;
        same_line.markdown.end_column = 20;
        result.matches[0].occurrences.push(same_line);
        let mut second = result.matches[0].clone();
        second.ordinal = 2;
        second.occurrences.truncate(1);
        second.occurrences[0].markdown.start_line = 825;
        second.occurrences[0].markdown.end_line = 825;
        second.occurrences[0].markdown.start_column = 7;
        second.occurrences[0].markdown.end_column = 13;

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
        assert!(rendered.contains("  824:3,14  --acls\n  825:7  --acls"));
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
        let mut second = result.matches[0].clone();
        second.ordinal = 2;
        second.occurrences[0].markdown.start_line = 826;
        second.occurrences[0].markdown.end_line = 826;
        second.occurrences[0].markdown.start_column = 8;
        second.context = vec![
            context(825, "between", false),
            context(826, "second --acls", true),
            context(827, "after", false),
        ];
        result.total = 2;
        result.returned = 2;
        result.matches.push(second);

        let rendered = render_search_text(&result);

        assert!(rendered.contains("  824:3; 826:8"));
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
