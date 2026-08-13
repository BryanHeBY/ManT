//! Deterministic rendering of already materialized query views.

use mant_core::{QueryViewResult, ResolvedQuery};
use mant_ir::SourceFormat;
use serde::Serialize;

use crate::{arguments::QueryFormat, error::Failure};

pub(super) fn render_query_result(
    result: &QueryViewResult,
    format: QueryFormat,
    pretty: bool,
    preserve_anchors: bool,
) -> Result<String, Failure> {
    match result {
        QueryViewResult::Full(query) => render_full_query(query, format, pretty, preserve_anchors),
        QueryViewResult::Outline(outline) => match format {
            QueryFormat::Markdown => Ok(mant_core::render_outline_markdown(outline)),
            QueryFormat::Text | QueryFormat::Man => Ok(mant_core::render_outline_text(outline)),
            QueryFormat::Json => {
                mant_core::render_outline_json(outline, pretty).map_err(Failure::operational)
            }
        },
        QueryViewResult::Excerpt(excerpt) => {
            render_excerpt(excerpt, format, pretty, preserve_anchors)
        }
        QueryViewResult::Search(search) => match format {
            QueryFormat::Markdown => Ok(mant_core::render_search_markdown(search)),
            QueryFormat::Text | QueryFormat::Man => Ok(mant_core::render_search_text(search)),
            QueryFormat::Json => {
                mant_core::render_search_json(search, pretty).map_err(Failure::operational)
            }
        },
    }
}

fn render_excerpt(
    excerpt: &mant_protocol::QueryExcerpt,
    format: QueryFormat,
    pretty: bool,
    preserve_anchors: bool,
) -> Result<String, Failure> {
    match format {
        QueryFormat::Markdown => Ok(mant_core::render_excerpt_markdown_with_options(
            excerpt,
            mant_core::MarkdownOptions { preserve_anchors },
        )),
        QueryFormat::Text | QueryFormat::Man => Ok(mant_core::render_excerpt_text(excerpt)),
        QueryFormat::Json => {
            mant_core::render_excerpt_json(excerpt, pretty).map_err(Failure::operational)
        }
    }
}

fn render_full_query(
    query: &ResolvedQuery,
    format: QueryFormat,
    pretty: bool,
    preserve_anchors: bool,
) -> Result<String, Failure> {
    match format {
        QueryFormat::Markdown => Ok(mant_core::render_markdown_with_options(
            query,
            mant_core::MarkdownOptions { preserve_anchors },
        )),
        QueryFormat::Text => Ok(mant_core::render_query_text(query)),
        QueryFormat::Man => {
            let Some(document) = query.document.as_ref() else {
                return Err(Failure::operational(
                    "manual page is unavailable; --format man cannot render tldr-only content",
                ));
            };
            if document.source.format == SourceFormat::Markdown {
                return Err(Failure::usage(
                    "--format man applies only to roff manual pages",
                ));
            }
            Ok(mant_core::render_query_man(query))
        }
        QueryFormat::Json => {
            mant_core::render_query_json(query, pretty).map_err(Failure::operational)
        }
    }
}

pub(super) fn render_json(value: &impl Serialize, pretty: bool) -> Result<String, Failure> {
    if pretty {
        serde_json::to_string_pretty(value).map_err(Failure::operational)
    } else {
        serde_json::to_string(value).map_err(Failure::operational)
    }
}
