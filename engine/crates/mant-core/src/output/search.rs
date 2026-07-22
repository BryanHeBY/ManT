//! Presents structure-aware search results for terminals and language models.

use std::fmt::Write as _;

use mant_ast::QuerySearch;

/// Render grep-like results with stable Markdown coordinates and node paths.
#[must_use]
pub fn render_search_text(search: &QuerySearch) -> String {
    let label = document_label(search);
    if search.total == 0 {
        return format!("No matches for \"{}\" in {label}.", search.query.pattern);
    }
    if search.matches.is_empty() {
        return format!(
            "No matches returned at offset {} for \"{}\" in {label} ({} total).",
            search.offset, search.query.pattern, search.total
        );
    }

    let mut rendered = search
        .matches
        .iter()
        .map(|found| {
            let mut lines = vec![format!(
                "{label}:{}:{} [{}] {}",
                found.markdown.start_line,
                found.markdown.start_column,
                found.node.path(),
                found.node.title()
            )];
            if found.context.is_empty() {
                lines.push(format!("  {}", found.preview));
            } else {
                lines.extend(found.context.iter().map(|line| {
                    format!(
                        "  {} {} {}",
                        if line.matched { ">" } else { " " },
                        line.line,
                        line.text
                    )
                }));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if let Some(next_offset) = search.next_offset {
        let _ = write!(
            rendered,
            "\n\n{} total matches; continue with --offset {next_offset}.",
            search.total
        );
    }
    rendered
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
            "match"
        } else {
            "matches"
        }
    ));
    if search.returned < search.total {
        if search.returned == 0 {
            blocks.push(format!(
                "No matches were returned at offset {}.",
                search.offset
            ));
        } else {
            let range_start = search.offset.saturating_add(1);
            let range_end = search.offset.saturating_add(search.returned);
            let continuation = search
                .next_offset
                .map_or(String::new(), |offset| format!(" Next offset: `{offset}`."));
            blocks.push(format!(
                "Showing matches {range_start}–{range_end}.{continuation}"
            ));
        }
    }

    for found in &search.matches {
        blocks.push(format!(
            "## {}. {}",
            found.ordinal,
            code_span(found.node.title())
        ));
        let mut details = vec![
            format!("- Node: {}", code_span(found.node.path())),
            format!(
                "- Markdown: line {}, column {}",
                found.markdown.start_line, found.markdown.start_column
            ),
        ];
        if let Some(section) = &found.section {
            details.push(format!(
                "- Section: {} ({})",
                code_span(&section.title),
                code_span(&section.path)
            ));
        }
        if let Some(source) = found.source {
            details.push(format!(
                "- Source: line {}, column {}",
                source.line, source.column
            ));
        }
        blocks.push(details.join("\n"));
        blocks.push(format!("> {}", found.preview.replace('\n', "\n> ")));
    }
    blocks.join("\n\n").trim_end().to_owned()
}

fn document_label(search: &QuerySearch) -> String {
    search.manual_section.as_ref().map_or_else(
        || search.topic.clone(),
        |section| format!("{}({section})", search.topic),
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
    use mant_ast::{
        MarkdownSchema, QuerySearch, SearchCase, SearchMarkdownRange, SearchMatch, SearchNode,
        SearchQuery, SearchRender, SearchRenderFormat, SearchRenderScope, SearchSchema,
        SearchScope, SearchSyntax,
    };

    use super::{render_search_markdown, render_search_text};

    fn result() -> QuerySearch {
        QuerySearch {
            schema: SearchSchema::V1,
            topic: "tar".to_owned(),
            manual_section: Some("1".to_owned()),
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
                node: SearchNode::ManualEntry {
                    path: "5.3/o17".to_owned(),
                    id: "acls-option".to_owned(),
                    title: "--acls".to_owned(),
                    role: mant_ast::DefinitionRole::Option,
                    names: vec!["--acls".to_owned()],
                },
                section: None,
                matched_text: "--acls".to_owned(),
                markdown: SearchMarkdownRange {
                    start_byte: 10,
                    end_byte: 16,
                    start_line: 824,
                    start_column: 3,
                    end_line: 824,
                    end_column: 9,
                },
                source: None,
                preview: "- `--acls`".to_owned(),
                context: Vec::new(),
            }],
        }
    }

    #[test]
    fn search_reports_are_human_readable_but_keep_machine_node_paths() {
        let result = result();
        assert!(render_search_text(&result).contains("tar(1):824:3 [5.3/o17] --acls"));
        let markdown = render_search_markdown(&result);
        assert!(markdown.contains("# Search results for `--acls` in tar(1)"));
        assert!(markdown.contains("- Node: `5.3/o17`"));
    }
}
