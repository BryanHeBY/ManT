//! Scope coverage and document grouping; entry bodies use shared renderers.
use super::{
    RenderOptions, render_json,
    terminal::{
        TerminalRole, render_terminal_explanation, render_terminal_search, terminal_search,
        terminal_style,
    },
};
use crate::{arguments::QueryFormat, error::Failure};
use mant_ir::DocumentMeta;
use mant_protocol::{
    QuerySearch, ScopeQueryResponse, ScopeQueryResult, ScopedQueryFailure, ScopedSearchDocument,
    SearchQuery, SearchSchema, sanitize_terminal_text,
};
use std::fmt::Write as _;
pub(crate) fn render_scope_query_result(
    response: &ScopeQueryResponse,
    options: RenderOptions,
) -> Result<String, Failure> {
    let RenderOptions {
        format,
        pretty,
        color,
        ..
    } = options;
    let output_terminal = options.terminal();
    if format == QueryFormat::Json {
        return render_json(response, pretty);
    }
    if format == QueryFormat::Man {
        return Err(Failure::usage(
            "--format man applies only to one full native manual",
        ));
    }
    let mut output = String::new();
    match &response.result {
        ScopeQueryResult::Explain { explanation } => {
            let outcome = match explanation.outcome {
                mant_protocol::ExplanationOutcome::Evidence => "evidence",
                mant_protocol::ExplanationOutcome::NoEvidence => "no-evidence",
            };
            let _ = write!(
                output,
                "Explanation {:?}: {outcome}; owners={}, returned={}, offset={}",
                sanitize_terminal_text(&explanation.query.entry),
                explanation.total,
                explanation.returned,
                explanation.query.options.offset
            );
            if let Some(next) = explanation.next_offset {
                let _ = write!(output, "; nextOffset={next}");
            }
            let _ = write!(
                output,
                "\nCoverage: loaded={}, unresolved={}, frontier={}; truncated: candidates={}, relations={}, content={}",
                explanation.documents.len(),
                response.scope.unresolved.len(),
                response.scope.frontier.len(),
                explanation.truncation.candidates,
                explanation.truncation.relations,
                explanation.truncation.content
            );
            for found in &explanation.documents {
                if found.explanation.evidence.is_empty() {
                    continue;
                }
                output.push_str("\n\n");
                write_scope_heading(
                    &mut output,
                    &found.address.catalog_path(),
                    format,
                    color,
                    output_terminal,
                );
                output.push('\n');
                let rendered = if format == QueryFormat::Markdown {
                    mant_engine::render_explanation_markdown(&found.explanation)
                } else {
                    render_terminal_explanation(&found.explanation, color)
                };
                output.push_str(&rendered);
            }
            write_scope_failures(
                &mut output,
                &explanation.failures,
                format,
                color,
                output_terminal,
            );
        }
        ScopeQueryResult::Search { search } => {
            for (index, found) in search.documents.iter().enumerate() {
                if index > 0 {
                    output.push_str("\n\n");
                }
                write_scope_heading(
                    &mut output,
                    &found.address.catalog_path(),
                    format,
                    color,
                    output_terminal,
                );
                output.push('\n');
                let local_search = scoped_search_projection(found, &search.query);
                let rendered = match format {
                    QueryFormat::Markdown => {
                        let search = output_terminal.then(|| terminal_search(&local_search));
                        mant_engine::render_search_markdown(
                            search.as_ref().unwrap_or(&local_search),
                        )
                    }
                    QueryFormat::Text => render_terminal_search(&local_search, color),
                    QueryFormat::Json | QueryFormat::Man => unreachable!(),
                };
                output.push_str(rendered.trim());
            }
        }
    }
    Ok(output)
}

fn write_scope_failures(
    output: &mut String,
    failures: &[ScopedQueryFailure],
    format: QueryFormat,
    color: bool,
    output_terminal: bool,
) {
    for failure in failures {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        write_scope_heading(
            output,
            &failure.address.catalog_path(),
            format,
            color,
            output_terminal,
        );
        output.push('\n');
        if format == QueryFormat::Text || output_terminal {
            output.push_str(&sanitize_terminal_text(&failure.reason));
        } else {
            output.push_str(&failure.reason);
        }
    }
}

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

fn write_scope_heading(
    output: &mut String,
    address: &str,
    format: QueryFormat,
    color: bool,
    output_terminal: bool,
) {
    let address = if format == QueryFormat::Text || output_terminal {
        sanitize_terminal_text(address)
    } else {
        std::borrow::Cow::Borrowed(address)
    };
    match format {
        QueryFormat::Markdown => {
            output.push_str("## ");
            output.push_str(&address);
        }
        QueryFormat::Text if color => {
            let style = terminal_style(TerminalRole::Document);
            write!(output, "{style}{address}{style:#}").expect("writing to String cannot fail");
        }
        QueryFormat::Text => output.push_str(&address),
        QueryFormat::Json | QueryFormat::Man => unreachable!(),
    }
}
