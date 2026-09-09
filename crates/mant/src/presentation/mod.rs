//! Deterministic rendering of already materialized query views.
mod content;
mod scope;
mod terminal;
#[cfg(test)]
mod tests;

use crate::{arguments::QueryFormat, error::Failure};
use mant_engine::QueryViewResult;
pub(super) use scope::render_scope_query_result;
use serde::Serialize;
pub(super) use terminal::render_catalog_output;
use terminal::{
    render_full_query, render_terminal_excerpt, render_terminal_explanation,
    render_terminal_outline, render_terminal_search, terminal_excerpt, terminal_outline,
    terminal_search,
};

/// Physical destination characteristics that may affect terminal safety only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OutputTarget {
    Stream,
    Terminal,
}

/// Complete rendering policy after command-line defaults have been resolved.
#[derive(Debug, Clone, Copy)]
pub(super) struct RenderOptions {
    pub(super) format: QueryFormat,
    pub(super) pretty: bool,
    pub(super) preserve_anchors: bool,
    pub(super) color: bool,
    pub(super) target: OutputTarget,
}

impl RenderOptions {
    const fn terminal(self) -> bool {
        matches!(self.target, OutputTarget::Terminal)
    }
}

pub(super) fn render_query_result(
    result: &QueryViewResult,
    options: RenderOptions,
) -> Result<String, Failure> {
    let RenderOptions {
        format,
        pretty,
        color,
        ..
    } = options;
    let output_terminal = options.terminal();
    match result {
        QueryViewResult::Full(query) => render_full_query(query, options),
        QueryViewResult::Outline(outline) => match format {
            QueryFormat::Markdown if output_terminal => Ok(mant_engine::render_outline_markdown(
                &terminal_outline(outline),
            )),
            QueryFormat::Markdown => Ok(mant_engine::render_outline_markdown(outline)),
            QueryFormat::Text => Ok(render_terminal_outline(outline, color)),
            QueryFormat::Man => Err(Failure::usage(
                "--format man applies only to full documents",
            )),
            QueryFormat::Json => {
                mant_engine::render_outline_json(outline, pretty).map_err(Failure::operational)
            }
        },
        QueryViewResult::Excerpt(excerpt) => render_excerpt(excerpt, options),
        QueryViewResult::Explanation(explanation) => match format {
            QueryFormat::Json => render_json(explanation, pretty),
            QueryFormat::Text => Ok(render_terminal_explanation(explanation, color)),
            QueryFormat::Markdown => Ok(mant_engine::render_explanation_markdown(explanation)),
            QueryFormat::Man => Err(Failure::usage(
                "--format man applies only to full documents",
            )),
        },
        QueryViewResult::Search(search) => match format {
            QueryFormat::Markdown if output_terminal => Ok(mant_engine::render_search_markdown(
                &terminal_search(search),
            )),
            QueryFormat::Markdown => Ok(mant_engine::render_search_markdown(search)),
            QueryFormat::Text => Ok(render_terminal_search(search, color)),
            QueryFormat::Man => Err(Failure::usage(
                "--format man applies only to full documents",
            )),
            QueryFormat::Json => {
                mant_engine::render_search_json(search, pretty).map_err(Failure::operational)
            }
        },
    }
}

fn render_excerpt(
    excerpt: &mant_protocol::QueryExcerpt,
    options: RenderOptions,
) -> Result<String, Failure> {
    let RenderOptions {
        format,
        pretty,
        preserve_anchors,
        color,
        ..
    } = options;
    let output_terminal = options.terminal();
    match format {
        QueryFormat::Markdown => {
            let terminal_copy = output_terminal.then(|| terminal_excerpt(excerpt));
            Ok(mant_engine::render_excerpt_markdown_with_options(
                terminal_copy.as_ref().unwrap_or(excerpt),
                mant_engine::MarkdownFragmentOptions { preserve_anchors },
            ))
        }
        QueryFormat::Text => Ok(render_terminal_excerpt(excerpt, color)),
        QueryFormat::Man => Err(Failure::usage(
            "--format man applies only to full documents",
        )),
        QueryFormat::Json => {
            mant_engine::render_excerpt_json(excerpt, pretty).map_err(Failure::operational)
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
