//! Presents structure-aware search results for terminals and language models.

use mant_ir::DefinitionRole;
use mant_protocol::{QuerySearch, SearchNode};
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

fn render_search_line_text(markdown: &str) -> String {
    let mut rendered = String::with_capacity(markdown.len());
    for event in Parser::new(markdown) {
        match event {
            Event::Text(value)
            | Event::Code(value)
            | Event::InlineMath(value)
            | Event::DisplayMath(value) => rendered.push_str(&value),
            Event::SoftBreak | Event::HardBreak => rendered.push(' '),
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
    rendered.trim_end().to_owned()
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
        output.plain("No matches returned at offset ");
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

    for (index, found) in search.matches.iter().enumerate() {
        if index > 0 {
            output.line();
            output.line();
        }
        output.push(SearchTextRole::Document, &label);
        output.plain(":");
        output.push(
            SearchTextRole::Coordinate,
            &format!(
                "{}:{}",
                found.markdown.start_line, found.markdown.start_column
            ),
        );
        output.push(SearchTextRole::Muted, " [");
        output.push(SearchTextRole::Path, found.node.path());
        output.push(SearchTextRole::Muted, "] ");
        output.push(search_node_role(&found.node), found.node.title());
        output.line();
        if found.context.is_empty() {
            output.plain("  ");
            let visible = render_search_line_text(&found.preview);
            output.matching_line(&visible, &found.matched_text);
        } else {
            for (line_index, line) in found.context.iter().enumerate() {
                if line_index > 0 {
                    output.line();
                }
                output.plain("  ");
                output.push(
                    if line.matched {
                        SearchTextRole::Match
                    } else {
                        SearchTextRole::Muted
                    },
                    if line.matched { ">" } else { " " },
                );
                output.plain(" ");
                output.push(SearchTextRole::Coordinate, &line.line.to_string());
                output.plain(" ");
                let visible = render_search_line_text(&line.text);
                if line.matched {
                    output.matching_line(&visible, &found.matched_text);
                } else {
                    output.plain(&visible);
                }
            }
        }
    }
    if let Some(next_offset) = search.next_offset {
        output.line();
        output.line();
        output.push(SearchTextRole::Coordinate, &search.total.to_string());
        output.plain(" total matches; continue with ");
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

    fn matching_line(&mut self, line: &str, matched: &str) {
        if matched.is_empty() {
            self.plain(line);
            return;
        }

        let mut start = 0;
        let mut found = false;
        for (offset, _) in line.match_indices(matched) {
            found = true;
            self.plain(&line[start..offset]);
            let end = offset + matched.len();
            self.push(SearchTextRole::Match, &line[offset..end]);
            start = end;
        }
        if found {
            self.plain(&line[start..]);
        } else {
            // Markdown-scope matches can target presentation syntax that is
            // absent from the visible projection. Do not imply a false range.
            self.plain(line);
        }
    }

    fn finish(self) -> String {
        self.rendered.trim_end().to_owned()
    }
}

const fn search_node_role(node: &SearchNode) -> SearchTextRole {
    match node {
        SearchNode::DocumentEntry { role, .. } => SearchTextRole::Definition(*role),
        SearchNode::Tldr { .. }
        | SearchNode::DocumentRoot { .. }
        | SearchNode::DocumentSection { .. } => SearchTextRole::Heading,
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
        MarkdownSchema, QuerySearch, SearchCase, SearchMarkdownRange, SearchMatch, SearchNode,
        SearchQuery, SearchRender, SearchRenderFormat, SearchRenderScope, SearchSchema,
        SearchScope, SearchSyntax,
    };

    use super::{
        SearchTextRole, render_search_line_text, render_search_markdown, render_search_text,
        render_search_text_with,
    };

    fn result() -> QuerySearch {
        QuerySearch {
            schema: SearchSchema::V7,
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
                node: SearchNode::DocumentEntry {
                    path: "5.3/e17".to_owned().into(),
                    id: "acls-option".to_owned().into(),
                    title: "--acls".to_owned(),
                    role: mant_ir::DefinitionRole::Option,
                    case: mant_ir::DefinitionCase::Sensitive,
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
        assert!(render_search_text(&result).contains("tar(1):824:3 [5.3/e17] --acls"));
        assert!(render_search_text(&result).contains("  --acls"));
        assert!(!render_search_text(&result).contains("`--acls`"));
        let markdown = render_search_markdown(&result);
        assert!(markdown.contains("# Search results for `--acls` in tar(1)"));
        assert!(markdown.contains("- Node: `5.3/e17`"));
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
}
