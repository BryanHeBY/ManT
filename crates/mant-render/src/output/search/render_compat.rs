//! Legacy rendered-coordinate presentation for Markdown and TLDR-inclusive search.

use super::legacy_context::{
    context_group_end, merged_context, occurrence_line_ranges, text_group_coordinates,
    truncated_occurrence_summary,
};
use super::line::render_search_line;
use super::{SearchTextRenderer, SearchTextRole, document_label, render_outline_trail};
use mant_protocol::QuerySearch;

pub(super) fn render_search_text_with(
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
            let line = found.occurrences.first().map_or(0, |occurrence| {
                occurrence
                    .markdown
                    .expect("render-only hit has a Markdown range")
                    .start_line
            });
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
