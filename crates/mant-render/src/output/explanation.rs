//! One DTO-only report traversal for direct CLI, scope, Markdown and MCP.
mod definition;
mod metadata;
mod report;
use super::styles as spans;
use crate::presentation::{TextPresentation, TextRole};
use mant_protocol::{QueryExplanation, ScopeExplanation};
use report::Report;
use std::collections::HashSet;

/// Render classified evidence without conflating mentions with definitions.
#[must_use]
pub fn render_explanation_text(result: &QueryExplanation) -> String {
    render_explanation_text_with(result, |_, text| text.to_owned())
}
/// Render source-bound report spans; decoration must preserve visible text and
/// boundary whitespace. No collector or original document is consulted.
#[must_use]
pub fn render_explanation_text_with(
    result: &QueryExplanation,
    decorate: impl Fn(TextPresentation, &str) -> String,
) -> String {
    render(
        result,
        &Report {
            markdown: false,
            options: mant_codec::encode::MarkdownFragmentOptions::default(),
            decorate: &decorate,
        },
    )
}
/// Render the same evidence page as escaped, quoted `CommonMark`.
#[must_use]
pub fn render_explanation_markdown(result: &QueryExplanation) -> String {
    render_explanation_markdown_with_options(
        result,
        mant_codec::encode::MarkdownFragmentOptions::default(),
    )
}

/// Render explanation evidence using the document export's formula policy.
#[must_use]
pub fn render_explanation_markdown_with_options(
    result: &QueryExplanation,
    options: mant_codec::encode::MarkdownFragmentOptions,
) -> String {
    render(
        result,
        &Report {
            markdown: true,
            options,
            decorate: &|_, text| text.to_owned(),
        },
    )
}
/// Render the global class-first page without regrouping by document.
#[must_use]
pub fn render_scope_explanation_text(result: &ScopeExplanation) -> String {
    render_scope_explanation_text_with(result, |_, text| text.to_owned())
}
/// Decorate the same source-qualified scope report as plain text.
#[must_use]
pub fn render_scope_explanation_text_with(
    result: &ScopeExplanation,
    decorate: impl Fn(TextPresentation, &str) -> String,
) -> String {
    render_scope(
        result,
        &Report {
            markdown: false,
            options: mant_codec::encode::MarkdownFragmentOptions::default(),
            decorate: &decorate,
        },
        None,
    )
}
/// `CommonMark` presentation of the same global class-first evidence page.
#[must_use]
pub fn render_scope_explanation_markdown(result: &ScopeExplanation) -> String {
    render_scope_explanation_markdown_with_options(
        result,
        mant_codec::encode::MarkdownFragmentOptions::default(),
    )
}

/// Render cross-document explanation evidence with explicit formula options.
#[must_use]
pub fn render_scope_explanation_markdown_with_options(
    result: &ScopeExplanation,
    options: mant_codec::encode::MarkdownFragmentOptions,
) -> String {
    let report = Report {
        markdown: true,
        options,
        decorate: &|_, text| text.to_owned(),
    };
    if options.preserve_anchors {
        mant_codec::encode::render_scoped_report_page(|anchors| {
            render_scope(result, &report, anchors)
        })
    } else {
        render_scope(result, &report, None)
    }
}

fn render(result: &QueryExplanation, report: &Report<'_>) -> String {
    let mut output = report.meta(
        TextRole::Document,
        &crate::presentation::render_explanation_status(result),
    );
    report.counts(&mut output, &result.counts);
    let address = result
        .address
        .as_ref()
        .map(mant_ir::DocumentAddress::catalog_path);
    let source = report.source(address.as_deref(), &result.supports, &result.diagnostics);
    report.records(&mut output, result.evidence.iter().map(|e| (e, source)));
    output
}

fn render_scope(
    result: &ScopeExplanation,
    report: &Report<'_>,
    anchors: Option<&HashSet<String>>,
) -> String {
    let mut output = report.meta(TextRole::Document, &format!(
        "Explanation {:?}: {}; owners={}, returned={}, offset={}; nextOffset={:?}; truncated: candidates={}, relations={}, content={}",
        crate::presentation::sanitize_terminal_text(&result.query.entry), metadata::outcome_name(result.outcome), result.total, result.returned, result.query.options.offset, result.next_offset, result.truncation.candidates, result.truncation.relations, result.truncation.content));
    report.counts(&mut output, &result.counts);
    let addresses = result
        .documents
        .iter()
        .map(|d| d.address.catalog_path())
        .collect::<Vec<_>>();
    let mut sources = Vec::with_capacity(result.documents.len());
    let scoped_anchors = report.markdown && report.options.preserve_anchors;
    for (index, (doc, address)) in result.documents.iter().zip(&addresses).enumerate() {
        output.push('\n');
        output.push_str(&report.meta(
            TextRole::Metadata,
            &format!(
                "Source {}: {}; owners={}, returned={}; semanticsComplete={}",
                crate::presentation::sanitize_terminal_text(address),
                metadata::outcome_name(doc.outcome),
                doc.total,
                doc.returned,
                doc.semantics_complete
            ),
        ));
        let mut source = report.source(Some(address), &doc.supports, &doc.diagnostics);
        if scoped_anchors {
            source = source.in_scope(index, anchors);
        }
        sources.push(source);
    }
    let unknown = Report {
        options: mant_codec::encode::MarkdownFragmentOptions {
            preserve_anchors: false,
            ..report.options
        },
        ..*report
    };
    let unknown_source = unknown.source(None, &[], &[]);
    report.records(
        &mut output,
        result.evidence.iter().map(|e| {
            (
                &e.evidence,
                sources
                    .get(e.document_index)
                    .copied()
                    .unwrap_or(unknown_source),
            )
        }),
    );
    output
}
