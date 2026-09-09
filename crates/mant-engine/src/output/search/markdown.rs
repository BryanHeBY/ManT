//! Markdown search reports consume only the supplied result DTO.
use super::{context::group_coordinates, document_label};
use mant_codec::encode::{commonmark_code_span as code_span, escape_commonmark as escape_text};
use mant_protocol::QuerySearch;

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
        if let Some(source) = found.node_source {
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
