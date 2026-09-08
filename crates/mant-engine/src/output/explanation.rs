//! One DTO-only report traversal for direct CLI, scope, Markdown and MCP.
mod metadata;
mod report;
pub(super) mod spans;
use mant_protocol::{QueryExplanation, ScopeExplanation, TextPresentation, TextRole};
use report::Report;

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
            decorate: &decorate,
        },
    )
}
/// Render the same evidence page as escaped, quoted `CommonMark`.
#[must_use]
pub fn render_explanation_markdown(result: &QueryExplanation) -> String {
    render(
        result,
        &Report {
            markdown: true,
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
            decorate: &decorate,
        },
    )
}
/// `CommonMark` presentation of the same global class-first evidence page.
#[must_use]
pub fn render_scope_explanation_markdown(result: &ScopeExplanation) -> String {
    render_scope(
        result,
        &Report {
            markdown: true,
            decorate: &|_, text| text.to_owned(),
        },
    )
}

fn render(result: &QueryExplanation, report: &Report<'_>) -> String {
    let mut output = report.meta(
        TextRole::Document,
        &mant_protocol::render_explanation_status(result),
    );
    report.counts(&mut output, &result.counts);
    let address = result
        .address
        .as_ref()
        .map(mant_ir::DocumentAddress::catalog_path);
    report.records(
        &mut output,
        result.evidence.iter().map(|e| (e, address.as_deref())),
    );
    output
}

fn render_scope(result: &ScopeExplanation, report: &Report<'_>) -> String {
    let mut output = report.meta(TextRole::Document, &format!(
        "Explanation {:?}: {}; owners={}, returned={}, offset={}; nextOffset={:?}; truncated: candidates={}, relations={}, content={}",
        mant_protocol::sanitize_terminal_text(&result.query.entry), metadata::outcome_name(result.outcome), result.total, result.returned, result.query.options.offset, result.next_offset, result.truncation.candidates, result.truncation.relations, result.truncation.content));
    report.counts(&mut output, &result.counts);
    let addresses = result
        .documents
        .iter()
        .map(|d| d.address.catalog_path())
        .collect::<Vec<_>>();
    for (doc, address) in result.documents.iter().zip(&addresses) {
        output.push('\n');
        output.push_str(&report.meta(
            TextRole::Metadata,
            &format!(
                "Source {}: {}; owners={}, returned={}; semanticsComplete={}",
                mant_protocol::sanitize_terminal_text(address),
                metadata::outcome_name(doc.outcome),
                doc.total,
                doc.returned,
                doc.semantics_complete
            ),
        ));
    }
    report.records(
        &mut output,
        result.evidence.iter().map(|e| {
            (
                &e.evidence,
                addresses.get(e.document_index).map(String::as_str),
            )
        }),
    );
    output
}
