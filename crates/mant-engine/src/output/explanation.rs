//! One evidence presentation used by CLI and compact agent transports.
use mant_protocol::{ExplanationContent, QueryExplanation, sanitize_terminal_text};
use std::fmt::Write;

/// Render evidence with original plain-text bodies and explicit match bases.
#[must_use]
pub fn render_explanation_text(result: &QueryExplanation) -> String {
    render(result, false)
}
/// Render evidence with original `CommonMark` bodies and explicit match bases.
#[must_use]
pub fn render_explanation_markdown(result: &QueryExplanation) -> String {
    render(result, true)
}

fn render(result: &QueryExplanation, markdown: bool) -> String {
    let mut output = mant_protocol::render_explanation_status(result);
    for evidence in &result.evidence {
        output.push_str("\n\n");
        let heading = mant_protocol::render_evidence_heading(evidence);
        if markdown {
            output.push_str("### ");
        }
        output.push_str(&heading);
        if let Some(content) = &evidence.content {
            let (ExplanationContent::Entry { block } | ExplanationContent::Block { block }) =
                content;
            let body = if markdown {
                super::markdown::blocks::render_blocks(
                    std::slice::from_ref(block),
                    super::MarkdownOptions::default(),
                )
                .join("\n\n")
            } else {
                super::text::render_blocks(std::slice::from_ref(block), 0)
            };
            output.push_str("\n\n");
            output.push_str(&body);
        }
        if evidence.content_omitted || evidence.details_omitted {
            write!(output, "\n\n[content budget: bodyOmitted={}, detailsOmitted={}; read node {} for original content]", evidence.content_omitted, evidence.details_omitted, sanitize_terminal_text(evidence.outline.path())).expect("String writer");
        }
    }
    // Public IR producers need not have passed a Markdown/roff safety masker.
    // Preserve LF/TAB layout, never terminal escapes or carriage-return actions.
    output
        .chars()
        .map(|c| {
            if c.is_control() && !matches!(c, '\n' | '\t') {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect()
}
