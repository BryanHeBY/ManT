//! Safe Markdown reports over already-retained exact occurrences.

use mant_codec::encode::{commonmark_code_span as code_span, escape_commonmark as escape_text};
use mant_protocol::{QuerySearch, ScopedSearchDocument, SearchMatch};

use super::{coordinate, document_label};

/// Render one document's occurrence page as readable, escaped Markdown.
#[must_use]
pub fn render_search_markdown(search: &QuerySearch) -> String {
    let label = document_label(search);
    let mut blocks = vec![format!(
        "# Search results for {} in {}",
        code_span(&search.query.pattern),
        escape_text(&label)
    )];
    blocks.push(format!(
        "{} {} in the document.",
        search.total,
        if search.total == 1 {
            "occurrence"
        } else {
            "occurrences"
        }
    ));
    if search.returned < search.total {
        if search.returned == 0 {
            blocks.push(format!(
                "No occurrences were returned at offset {}.",
                search.offset
            ));
        } else {
            let start = search.offset.saturating_add(1);
            let end = search.offset.saturating_add(search.returned);
            let continuation = search
                .next_offset
                .map_or(String::new(), |offset| format!(" Next offset: `{offset}`."));
            blocks.push(format!("Showing occurrences {start}–{end}.{continuation}"));
        }
    }
    blocks.extend(search.matches.iter().map(|found| match_block(found, "##")));
    if !search.semantics_complete {
        blocks.push(coverage_block(search.coverage_details_omitted));
    }
    blocks.join("\n\n").trim_end().to_owned()
}

/// Render one scope group without claiming a local total or page offset.
pub(in crate::output) fn render_scoped_search_markdown(found: &ScopedSearchDocument) -> String {
    let mut blocks = Vec::new();
    blocks.extend(
        found
            .matches
            .iter()
            .map(|matched| match_block(matched, "###")),
    );
    blocks.join("\n\n").trim_end().to_owned()
}

fn match_block(found: &SearchMatch, heading: &str) -> String {
    let mut lines = vec![
        format!(
            "{heading} {}. {}",
            found.ordinal,
            code_span(found.outline.title())
        ),
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
        format!("- Coordinate: {}", code_span(&coordinate(found))),
        format!("- Match: {}", code_span(&found.matched_text)),
    ];
    if let Some(source) = found.node_source {
        lines.push(format!(
            "- Source: line {}, column {}",
            source.line, source.column
        ));
    }
    if !found.preview.is_empty() && !found.preview.contains(&found.matched_text) {
        lines.push(format!(
            "> {}",
            found
                .preview
                .split('\n')
                .map(escape_text)
                .collect::<Vec<_>>()
                .join("\n> ")
        ));
    }
    for context in &found.context {
        if context.matched && context.text.contains(&found.matched_text) {
            continue;
        }
        lines.push(format!(
            "> {} {}: {}",
            if context.matched { ">" } else { " " },
            context.line,
            escape_text(&context.text)
        ));
    }
    lines.join("\n")
}

fn coverage_block(omitted: u32) -> String {
    if omitted == 0 {
        "Semantic coverage incomplete.".to_owned()
    } else {
        format!("Semantic coverage incomplete; {omitted} details omitted.")
    }
}
