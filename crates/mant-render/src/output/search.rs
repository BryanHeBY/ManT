//! Presents structure-aware search results for terminals and language models.

use mant_ir::EntryKind;
use std::ops::Range;

use mant_protocol::{OutlineNodeReference, OutlineTrail, QuerySearch};

mod context;
mod legacy_context;
mod line;
mod markdown;
mod render_compat;
use context::{
    context_group_end, group_coordinates, merged_context, occurrence_line_ranges,
    occurrence_start_line, truncated_occurrence_summary,
};
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
///
/// # Panics
///
/// Panics only when a caller constructs an invalid in-memory response with
/// logical hits but without their required response-local content projection.
#[must_use]
pub fn render_search_text_with(
    search: &QuerySearch,
    decorate: impl FnMut(SearchTextRole, &str) -> String,
) -> String {
    if search.content_projection.is_none() && !search.matches.is_empty() {
        return render_compat::render_search_text_with(search, decorate);
    }
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
    let content = search
        .content_projection
        .as_ref()
        .expect("logical search hits require a content projection")
        .content();

    let mut previous_outline = None;
    let mut index = 0;
    while index < search.matches.len() {
        let found = &search.matches[index];
        if previous_outline != Some(&found.outline) {
            if index > 0 {
                output.paragraph_break();
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
            let occurrence = found
                .occurrences
                .first()
                .expect("validated search hit has one logical occurrence");
            let logical = content
                .root_logical_text(occurrence.root.expect("validated logical hit has a root"))
                .expect("validated logical search root must resolve");
            let line = occurrence_start_line(&logical, occurrence).unwrap_or(1);
            let ranges = occurrence_line_ranges(found, &logical, line);
            output.matching_line(&found.preview, ranges);
        } else {
            for (line_number, (text, matched, source_ranges)) in merged_context(content, group) {
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
                if matched {
                    output.matching_line(text, source_ranges);
                } else {
                    output.plain(text);
                }
            }
        }
        previous_outline = Some(&found.outline);
        index = end;
    }
    if let Some(next_offset) = search.next_offset {
        output.paragraph_break();
        output.push(SearchTextRole::Coordinate, &search.total.to_string());
        output.plain(" total logical matches; continue with ");
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

    fn paragraph_break(&mut self) {
        self.line();
        self.line();
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
    use std::ops::Range;

    use mant_ir::{
        ContentOwnerKind, ContentProjection, ContentRootKind, ContentStoreBuilder, ContentStyle,
        Provenance,
    };
    use mant_protocol::{
        MarkdownSchema, OutlineNodeReference, OutlineTrail, QuerySearch, SearchCase, SearchHit,
        SearchLineRange, SearchLogicalRange, SearchMarkdownRange, SearchOccurrence, SearchQuery,
        SearchRender, SearchRenderFormat, SearchRenderScope, SearchSchema, SearchScope,
        SearchSyntax,
    };

    use super::{SearchTextRole, render_search_markdown, render_search_text_with};

    fn result(
        logical: &str,
        found: Range<usize>,
        markdown_projections: Vec<SearchMarkdownRange>,
    ) -> QuerySearch {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Document, Provenance::Unknown);
        let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let _ = builder.push_text(
            root,
            logical.to_owned(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let matched_text = logical[found.clone()].to_owned();
        let start_scalar = logical[..found.start].chars().count() as u64;
        let end_scalar = logical[..found.end].chars().count() as u64;
        QuerySearch {
            schema: SearchSchema::V0Dot12,
            label: "tar".to_owned(),
            source_context: None,
            meta: Some(mant_ir::DocumentMeta {
                manual_section: Some("1".to_owned()),
                ..mant_ir::DocumentMeta::default()
            }),
            content_projection: Some(ContentProjection {
                content_store: builder.finish(),
            }),
            query: SearchQuery {
                pattern: matched_text.clone(),
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
                line_count: 1,
            },
            total: 1,
            returned: 1,
            offset: 0,
            truncated: false,
            next_offset: None,
            matches: vec![SearchHit {
                ordinal: 1,
                outline: OutlineTrail {
                    ancestors: Vec::new(),
                    node: OutlineNodeReference::DocumentRoot {
                        path: "root".into(),
                        id: mant_ir::DOCUMENT_ROOT_ID.into(),
                        title: "OVERVIEW".into(),
                    },
                },
                occurrences: vec![SearchOccurrence {
                    matched_text,
                    root: Some(root),
                    logical: Some(SearchLogicalRange {
                        start_byte: found.start as u64,
                        end_byte: found.end as u64,
                        start_scalar,
                        end_scalar,
                    }),
                    markdown_projections,
                    markdown: None,
                    line_ranges: Vec::new(),
                }],
                occurrence_count: 1,
                occurrences_truncated: false,
                node_source: None,
                preview: logical.to_owned(),
                context: Vec::new(),
            }],
        }
    }

    #[test]
    fn logical_preview_highlights_the_authoritative_range() {
        let search = result("before --acls after", 7..13, Vec::new());
        let rendered = render_search_text_with(&search, |role, value| {
            if role == SearchTextRole::Match {
                format!("<match>{value}</match>")
            } else {
                value.to_owned()
            }
        });
        assert!(rendered.contains("before <match>--acls</match> after"));
        assert!(rendered.contains("logical-root-1:8"));
    }

    #[test]
    fn logical_preview_is_not_reparsed_as_markdown() {
        let search = result("literal *asterisks*", 8..19, Vec::new());
        let rendered = render_search_text_with(&search, |_, value| value.to_owned());
        assert!(rendered.contains("literal *asterisks*"));
    }

    #[test]
    fn markdown_report_distinguishes_projection_from_logical_fallback() {
        let logical = result("alpha", 0..5, Vec::new());
        assert!(render_search_markdown(&logical).contains("- Logical: logical-root-1:1"));

        let projected = result(
            "alpha",
            0..5,
            vec![SearchMarkdownRange {
                start_byte: 10,
                end_byte: 15,
                start_line: 4,
                start_column: 3,
                end_line: 4,
                end_column: 8,
            }],
        );
        assert!(render_search_markdown(&projected).contains("- Markdown: 4:3"));
    }

    #[test]
    fn render_only_search_highlights_without_a_content_projection() {
        let mut search = result("prefix needle suffix", 7..13, Vec::new());
        search.content_projection = None;
        search.query.scope = SearchScope::Markdown;
        let occurrence = &mut search.matches[0].occurrences[0];
        occurrence.root = None;
        occurrence.logical = None;
        occurrence.markdown = Some(SearchMarkdownRange {
            start_byte: 7,
            end_byte: 13,
            start_line: 1,
            start_column: 8,
            end_line: 1,
            end_column: 14,
        });
        occurrence.line_ranges = vec![SearchLineRange {
            line: 1,
            start_byte: 7,
            end_byte: 13,
        }];
        let rendered = render_search_text_with(&search, |role, value| {
            if role == SearchTextRole::Match {
                format!("<match>{value}</match>")
            } else {
                value.to_owned()
            }
        });
        assert!(rendered.contains("prefix <match>needle</match> suffix"));
        assert!(rendered.contains("1:8"));
    }
}
