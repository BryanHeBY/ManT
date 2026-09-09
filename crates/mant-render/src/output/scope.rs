//! Pure reports over an already resolved scope; no loading or query execution.
use super::{
    SearchTextRole, render_scope_explanation_markdown, render_scope_explanation_text_with,
    render_search_markdown, render_search_text_with,
};
use crate::{TextPresentation, TextRole, sanitize_terminal_text};
use mant_ir::DocumentMeta;
use mant_protocol::{
    QuerySearch, ScopeQueryResponse, ScopeQueryResult, ScopedSearchDocument, SearchQuery,
    SearchSchema,
};
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
            for (index, found) in search.documents.iter().enumerate() {
                if index > 0 {
                    output.push_str("\n\n");
                }
                output.push_str(&decorate(
                    ScopeTextRole::Document,
                    &sanitize_terminal_text(&found.address.catalog_path()),
                ));
                output.push('\n');
                let rendered = render_search_text_with(
                    &scoped_search_projection(found, &search.query),
                    |role, text| {
                        decorate(ScopeTextRole::Search(role), &sanitize_terminal_text(text))
                    },
                );
                output.push_str(rendered.trim());
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
            for (index, found) in search.documents.iter().enumerate() {
                if index > 0 {
                    output.push_str("\n\n");
                }
                output.push_str("## ");
                output.push_str(&identity(&found.address.catalog_path()));
                output.push('\n');
                let mut local = scoped_search_projection(found, &search.query);
                local.label = identity(&local.label);
                if let Some(section) = local
                    .meta
                    .as_mut()
                    .and_then(|meta| meta.manual_section.as_mut())
                {
                    *section = identity(section);
                }
                output.push_str(render_search_markdown(&local).trim());
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

// Each group is already globally paginated. Local reports must not invent
// independent continuation hints or replace the retained global hit ordinals.
fn scoped_search_projection(found: &ScopedSearchDocument, query: &SearchQuery) -> QuerySearch {
    let (label, meta) = match &found.address {
        mant_protocol::DocumentAddress::Manual {
            name,
            manual_section,
        } => (
            name.clone(),
            Some(DocumentMeta {
                manual_section: Some(manual_section.clone()),
                ..DocumentMeta::default()
            }),
        ),
        mant_protocol::DocumentAddress::Markdown { path, .. } => (path.clone(), None),
    };
    let returned = u32::try_from(found.matches.len()).unwrap_or(u32::MAX);
    QuerySearch {
        schema: SearchSchema::V0Dot11,
        label,
        source: None,
        meta,
        query: query.clone(),
        render: found.render.clone(),
        total: returned,
        returned,
        offset: 0,
        truncated: false,
        next_offset: None,
        matches: found.matches.clone(),
    }
}

#[cfg(test)]
mod tests;
