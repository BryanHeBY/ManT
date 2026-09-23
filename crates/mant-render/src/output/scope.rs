//! Pure reports over an already resolved scope; no loading or query execution.
use super::search::{render_scoped_search_markdown, render_scoped_search_text_with};
use super::{
    SearchTextRole, render_scope_explanation_markdown, render_scope_explanation_text_with,
};
use crate::{TextPresentation, TextRole, sanitize_terminal_text};
use mant_protocol::{ScopeQueryResponse, ScopeQueryResult};
use std::fmt::Write as _;

/// A scope report span. Hosts may decorate it without changing report ordering.
#[derive(Debug, Clone, Copy)]
pub enum ScopeTextRole {
    /// A document-group heading, separate from evidence metadata.
    Document,
    /// A source-aware explanation span.
    Evidence(TextPresentation),
    /// A search result span.
    Search(SearchTextRole),
}

/// Render a scope report as terminal-safe plain text.
#[must_use]
pub fn render_scope_query_text(response: &ScopeQueryResponse) -> String {
    render_scope_query_text_with(response, |_, text| text.to_owned())
}

/// Decorate a plain scope report without requerying or regrouping its DTO.
/// Text is made safe before decoration; body newlines and tabs remain intact.
#[must_use]
pub fn render_scope_query_text_with(
    response: &ScopeQueryResponse,
    decorate: impl Fn(ScopeTextRole, &str) -> String,
) -> String {
    let mut output = match &response.result {
        ScopeQueryResult::Explain { explanation } => {
            let mut output = render_scope_explanation_text_with(explanation, |role, text| {
                let text = match role.role {
                    TextRole::Body | TextRole::DefinitionTerm => text
                        .chars()
                        .map(|c| {
                            if c.is_control() && !matches!(c, '\n' | '\t') {
                                '\u{fffd}'
                            } else {
                                c
                            }
                        })
                        .collect(),
                    _ => sanitize_terminal_text(text).into_owned(),
                };
                decorate(ScopeTextRole::Evidence(role), &text)
            });
            write_coverage(&mut output, response, explanation.documents.len());
            for failure in &explanation.failures {
                if !output.is_empty() {
                    output.push_str("\n\n");
                }
                output.push_str(&decorate(
                    ScopeTextRole::Document,
                    &sanitize_terminal_text(&failure.address.catalog_path()),
                ));
                output.push('\n');
                output.push_str(&sanitize_terminal_text(&failure.reason));
            }
            output
        }
        ScopeQueryResult::Search { search } => {
            let mut output = String::new();
            if search.documents.is_empty() {
                if search.total == 0 {
                    output.push_str("No matches for \"");
                    output.push_str(&sanitize_terminal_text(&search.query.pattern));
                    output.push_str("\" in scope.");
                } else {
                    let _ = write!(
                        output,
                        "No occurrences returned at global offset {} ({} total).",
                        search.offset, search.total
                    );
                }
            }
            for (index, found) in search.documents.iter().enumerate() {
                if index > 0 {
                    output.push_str("\n\n");
                }
                output.push_str(&decorate(
                    ScopeTextRole::Document,
                    &sanitize_terminal_text(&found.address.catalog_path()),
                ));
                output.push('\n');
                let rendered =
                    render_scoped_search_text_with(found, &search.query, |role, text| {
                        decorate(ScopeTextRole::Search(role), &sanitize_terminal_text(text))
                    });
                output.push_str(rendered.trim_end());
            }
            if let Some(next) = search.next_offset {
                let _ = write!(
                    output,
                    "\n\n{} total occurrences; next occurrence offset {next}.",
                    search.total
                );
            }
            if !search.semantics_complete {
                output.push_str("\n\nSemantic coverage incomplete.");
            }
            output
        }
    };
    write_reference_limits(&mut output, response);
    output
}

/// Render a scope report as Markdown without host-terminal transformations.
#[must_use]
pub fn render_scope_query_markdown(response: &ScopeQueryResponse) -> String {
    render_scope_query_markdown_with(response, str::to_owned)
}

/// Map external document identities and failure text before Markdown reporting.
/// Hosts writing to a terminal can sanitize these fields without changing parsed
/// evidence bodies, structural newlines, or the already-selected result page.
#[must_use]
pub fn render_scope_query_markdown_with(
    response: &ScopeQueryResponse,
    identity: impl Fn(&str) -> String,
) -> String {
    let mut output = match &response.result {
        ScopeQueryResult::Explain { explanation } => {
            let mut output = render_scope_explanation_markdown(explanation);
            write_coverage(&mut output, response, explanation.documents.len());
            for failure in &explanation.failures {
                if !output.is_empty() {
                    output.push_str("\n\n");
                }
                output.push_str("## ");
                output.push_str(&identity(&failure.address.catalog_path()));
                output.push('\n');
                output.push_str(&identity(&failure.reason));
            }
            output
        }
        ScopeQueryResult::Search { search } => {
            let mut output = String::new();
            if search.documents.is_empty() {
                if search.total == 0 {
                    output.push_str("No matches for ");
                    output.push_str(&mant_codec::encode::commonmark_code_span(
                        &search.query.pattern,
                    ));
                    output.push_str(" in scope.");
                } else {
                    let _ = write!(
                        output,
                        "No occurrences returned at global offset {} ({} total).",
                        search.offset, search.total
                    );
                }
            }
            for (index, found) in search.documents.iter().enumerate() {
                if index > 0 {
                    output.push_str("\n\n");
                }
                output.push_str("## ");
                output.push_str(&identity(&found.address.catalog_path()));
                output.push('\n');
                output.push_str(&render_scoped_search_markdown(found));
            }
            if let Some(next) = search.next_offset {
                let _ = write!(
                    output,
                    "\n\n{} total occurrences. Next offset: `{next}`.",
                    search.total
                );
            }
            if !search.semantics_complete {
                output.push_str("\n\nSemantic coverage incomplete.");
            }
            output
        }
    };
    write_reference_limits(&mut output, response);
    output
}

fn write_coverage(output: &mut String, response: &ScopeQueryResponse, loaded: usize) {
    let _ = write!(
        output,
        "\nCoverage: loaded={loaded}, unresolved={}, frontier={}",
        response.scope.unresolved.len(),
        response.scope.frontier.len()
    );
}

fn write_reference_limits(output: &mut String, response: &ScopeQueryResponse) {
    for limited in &response.scope.reference_limits {
        let _ = write!(
            output,
            "\nReference scan incomplete for {}: {:?}",
            sanitize_terminal_text(&limited.document.catalog_path()),
            limited.coverage.status
        );
        if let Some(limit) = limited.retention_limit {
            let _ = write!(output, " ({limit:?})");
        }
    }
}

#[cfg(test)]
mod tests;
