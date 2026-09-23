//! CLI format dispatch and ANSI decoration over pure scope reports.
use super::{RenderOptions, render_json};
use crate::{arguments::QueryFormat, error::Failure};
use mant_protocol::ScopeQueryResponse;
use mant_render::{ScopeTextRole, sanitize_terminal_text};

pub(crate) fn render_scope_query_result(
    response: &ScopeQueryResponse,
    options: RenderOptions,
) -> Result<String, Failure> {
    match options.format {
        QueryFormat::Json => render_json(response, options.pretty),
        QueryFormat::Man => Err(Failure::usage(
            "--format man applies only to one full native manual",
        )),
        QueryFormat::Markdown => Ok(if options.terminal() {
            mant_render::render_scope_query_markdown_with(response, |text| {
                sanitize_terminal_text(text).into_owned()
            })
        } else {
            mant_render::render_scope_query_markdown(response)
        }),
        QueryFormat::Text => Ok(mant_render::render_scope_query_text_with(
            response,
            |role, text| match role {
                ScopeTextRole::Evidence(style) => {
                    super::content::decorate(style, text, options.color)
                }
                ScopeTextRole::Search(role) => {
                    super::terminal::decorate_search(role, text, options.color)
                }
                ScopeTextRole::Document if options.color => {
                    let style =
                        super::terminal::terminal_style(super::terminal::TerminalRole::Document);
                    format!("{style}{text}{style:#}")
                }
                ScopeTextRole::Document => text.to_owned(),
            },
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_protocol::{
        DocumentScope, DocumentTraversal, QuerySearch, ResolvedDocumentScope, ScopeQueryResult,
        ScopeQuerySchema, ScopeSearch, ScopeSearchSchema, ScopedSearchCoverage,
        ScopedSearchDocument,
    };

    #[test]
    #[allow(clippy::too_many_lines)] // One byte-for-byte fixture covers all CLI presentation modes.
    fn cli_scope_adapter_preserves_ansi_and_terminal_markdown_identity() {
        let local: QuerySearch = serde_json::from_value(serde_json::json!({
            "schema": "mant.search/v0.12",
            "label": " odd\u{1b} ",
            "sourceContext": {
                "sources": [{
                    "key": 1,
                    "identity": {"kind": "anonymous", "name": "scope-fixture"},
                    "format": "markdown",
                    "decodedByteLength": 0,
                    "coordinates": {"kind": "decoded-utf8-bytes"}
                }],
                "rootSource": 1
            },
            "query": {"pattern": "needle"},
            "render": {
                "schema": "mant.markdown/v1", "format": "markdown", "scope": "full",
                "lineBase": 1, "columnBase": 1, "lineCount": 1
            },
            "total": 0, "returned": 0, "offset": 0, "truncated": false,
            "semanticsComplete": true, "coverageDetailsOmitted": 0,
            "diagnostics": [], "matches": []
        }))
        .unwrap();
        let address = mant_ir::DocumentAddress::Markdown {
            path: local.label.clone(),
            origin: mant_ir::MarkdownOrigin::Documents,
        };
        let response = ScopeQueryResponse {
            schema: ScopeQuerySchema::V0Dot12,
            scope: ResolvedDocumentScope {
                query: DocumentScope {
                    documents: vec![],
                    traversal: DocumentTraversal::default(),
                },
                documents: vec![],
                edges: vec![],
                frontier: vec![],
                unresolved: vec![],
                reference_limits: vec![],
            },
            result: ScopeQueryResult::Search {
                search: ScopeSearch {
                    schema: ScopeSearchSchema::V0Dot12,
                    query: local.query.clone(),
                    total: 0,
                    returned: 0,
                    offset: 0,
                    truncated: false,
                    next_offset: None,
                    semantics_complete: true,
                    coverage_by_document: vec![ScopedSearchCoverage {
                        address: address.clone(),
                        depth: 0,
                        source_context: local.source_context.clone(),
                        semantics_complete: true,
                        coverage_details_omitted: 0,
                        diagnostics: vec![],
                    }],
                    documents: vec![ScopedSearchDocument {
                        address: address.clone(),
                        depth: 0,
                        source_context: local.source_context.clone(),
                        content_projection: None,
                        render: local.render.clone(),
                        matches: vec![],
                    }],
                },
            },
        };
        let options = RenderOptions {
            format: QueryFormat::Text,
            pretty: false,
            preserve_anchors: false,
            color: true,
            target: super::super::OutputTarget::Terminal,
        };
        let style =
            super::super::terminal::terminal_style(super::super::terminal::TerminalRole::Document);
        let expected = format!(
            "{style}{}{style:#}\n{}",
            sanitize_terminal_text(&address.catalog_path()),
            super::super::terminal::render_terminal_search(&local, true)
                .trim()
                .replacen("No matches for", "No retained occurrences for", 1)
        );
        assert_eq!(
            render_scope_query_result(&response, options).unwrap(),
            expected
        );
        for terminal in [false, true] {
            let heading = if terminal {
                sanitize_terminal_text(&address.catalog_path()).into_owned()
            } else {
                address.catalog_path()
            };
            let expected = format!("## {heading}\n");
            let options = RenderOptions {
                format: QueryFormat::Markdown,
                target: if terminal {
                    super::super::OutputTarget::Terminal
                } else {
                    super::super::OutputTarget::Stream
                },
                ..options
            };
            assert_eq!(
                render_scope_query_result(&response, options).unwrap(),
                expected
            );
        }
    }
}
