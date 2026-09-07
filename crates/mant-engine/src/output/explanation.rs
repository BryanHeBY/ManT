//! Shared classification-aware presentation; renderers never rematch or reorder.
use mant_protocol::{
    EvidenceClass, ExplanationContent, ExplanationEvidence, QueryExplanation, ScopeExplanation,
};
use std::fmt::Write;

/// Render classified evidence without conflating mentions with definitions.
#[must_use]
pub fn render_explanation_text(result: &QueryExplanation) -> String {
    render(result, false)
}
/// Render the same evidence page with escaped `CommonMark` metadata.
#[must_use]
pub fn render_explanation_markdown(result: &QueryExplanation) -> String {
    render(result, true)
}
/// Render the unique global scope page without regrouping it by document.
#[must_use]
pub fn render_scope_explanation_text(result: &ScopeExplanation) -> String {
    render_scope(result, false)
}
/// `CommonMark` presentation of the same global class-first evidence page.
#[must_use]
pub fn render_scope_explanation_markdown(result: &ScopeExplanation) -> String {
    render_scope(result, true)
}

fn render(result: &QueryExplanation, markdown: bool) -> String {
    let mut output = display(&mant_protocol::render_explanation_status(result), markdown);
    output.push_str(&display(
        &mant_protocol::render_evidence_counts(&result.counts),
        markdown,
    ));
    let address = result
        .address
        .as_ref()
        .map(mant_ir::DocumentAddress::catalog_path);
    records(
        &mut output,
        result.evidence.iter().map(|e| (e, address.as_deref())),
        markdown,
    );
    safe(&output)
}

fn render_scope(result: &ScopeExplanation, markdown: bool) -> String {
    let mut output = display(
        &format!(
            "Explanation {:?}: {}; owners={}, returned={}, offset={}; nextOffset={:?}; truncated: candidates={}, relations={}, content={}",
            result.query.entry,
            outcome_name(result.outcome),
            result.total,
            result.returned,
            result.query.options.offset,
            result.next_offset,
            result.truncation.candidates,
            result.truncation.relations,
            result.truncation.content
        ),
        markdown,
    );
    output.push_str(&display(
        &mant_protocol::render_evidence_counts(&result.counts),
        markdown,
    ));
    for doc in &result.documents {
        write!(
            output,
            "\n{}",
            display(
                &format!(
                    "Source {}: {}; owners={}, returned={}; semanticsComplete={}",
                    doc.address.catalog_path(),
                    outcome_name(doc.outcome),
                    doc.total,
                    doc.returned,
                    doc.semantics_complete
                ),
                markdown
            )
        )
        .expect("String writer");
    }
    let addresses = result
        .documents
        .iter()
        .map(|d| d.address.catalog_path())
        .collect::<Vec<_>>();
    records(
        &mut output,
        result.evidence.iter().map(|record| {
            (
                &record.evidence,
                addresses.get(record.document_index).map(String::as_str),
            )
        }),
        markdown,
    );
    safe(&output)
}

fn records<'a>(
    output: &mut String,
    records: impl Iterator<Item = (&'a ExplanationEvidence, Option<&'a str>)>,
    markdown: bool,
) {
    let mut previous = None;
    for (evidence, address) in records {
        if previous != Some(evidence.class) {
            write!(
                output,
                "\n\n{}{}",
                if markdown { "## " } else { "" },
                evidence.class.title()
            )
            .expect("String writer");
            previous = Some(evidence.class);
        }
        let heading = mant_protocol::render_evidence_heading(evidence);
        write!(
            output,
            "\n\n{}{}",
            if markdown { "### " } else { "" },
            display(&heading, markdown)
        )
        .expect("String writer");
        entry_details(output, evidence, markdown);
        if matches!(
            evidence.class,
            EvidenceClass::DirectEntry | EvidenceClass::RelatedEntry
        ) {
            if let Some(ExplanationContent::Entry { block } | ExplanationContent::Block { block }) =
                &evidence.content
            {
                let body = if markdown {
                    super::markdown::blocks::render_blocks(
                        std::slice::from_ref(block),
                        super::MarkdownOptions::default(),
                    )
                    .join("\n\n")
                } else {
                    super::text::render_blocks(std::slice::from_ref(block), 0)
                };
                write!(output, "\n\n{body}").expect("String writer");
            }
        } else {
            for preview in &evidence.previews {
                write!(
                    output,
                    "\n\n{}",
                    display(
                        &format!(
                            "At {}; match chars={}..{}; clippedBefore={}, clippedAfter={}",
                            preview.block_path,
                            preview.match_start_char,
                            preview.match_end_char,
                            preview.clipped_before,
                            preview.clipped_after
                        ),
                        markdown
                    )
                )
                .expect("String writer");
                for line in preview.text.split('\n') {
                    write!(
                        output,
                        "\n{}{}",
                        if markdown { "> " } else { "  " },
                        display(line, markdown)
                    )
                    .expect("String writer");
                }
            }
        }
        write!(
            output,
            "\n\n{}",
            display(
                &format!(
                    "Read original: {}; node {}",
                    address.unwrap_or("current input"),
                    evidence.outline.path()
                ),
                markdown
            )
        )
        .expect("String writer");
        if evidence.has_omitted_content() {
            write!(
                output,
                "\n{}",
                display(
                    &format!(
                        "[content budget: bodyOmitted={}, detailsOmitted={}, previewsOmitted={}]",
                        evidence.content_omitted,
                        evidence.details_omitted,
                        evidence.previews_omitted
                    ),
                    markdown
                )
            )
            .expect("String writer");
        }
    }
}

fn outcome_name(value: mant_protocol::ExplanationOutcome) -> &'static str {
    match value {
        mant_protocol::ExplanationOutcome::Evidence => "evidence",
        mant_protocol::ExplanationOutcome::NoEvidence => "no-evidence",
    }
}

fn domain_label(domain: &mant_ir::ValueDomain) -> String {
    match domain {
        mant_ir::ValueDomain::Choices { exhaustive } => format!("choices; exhaustive={exhaustive}"),
        mant_ir::ValueDomain::EntrySet {
            reference,
            entry_kinds,
            ..
        } => {
            let target = match reference {
                mant_ir::SemanticDocumentReference::Document { name, fragment } => {
                    format!(
                        "{name}{}",
                        fragment
                            .as_ref()
                            .map(|f| format!("#{f}"))
                            .unwrap_or_default()
                    )
                }
                mant_ir::SemanticDocumentReference::Manual {
                    name,
                    manual_section,
                } => {
                    format!("manual/{}/{name}", manual_section.as_deref().unwrap_or("?"))
                }
            };
            format!("entries={target}; kinds={entry_kinds:?}")
        }
    }
}

fn safe(value: &str) -> String {
    value
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
fn display(value: &str, markdown: bool) -> String {
    let value = safe(value);
    if !markdown {
        return value;
    }
    let mut out = String::new();
    for c in value.chars() {
        if matches!(
            c,
            '\\' | '`'
                | '*'
                | '_'
                | '{'
                | '}'
                | '['
                | ']'
                | '('
                | ')'
                | '#'
                | '+'
                | '-'
                | '.'
                | '!'
                | '|'
                | '>'
                | '<'
                | '~'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn entry_details(output: &mut String, evidence: &ExplanationEvidence, markdown: bool) {
    if let Some(entry) = &evidence.entry {
        let forms = entry
            .forms
            .iter()
            .map(|form| crate::inline::plain_text(form))
            .collect::<Vec<_>>()
            .join(" | ");
        write!(
            output,
            "\n{}",
            display(&format!("Kind: {:?}; forms: {forms}", entry.kind), markdown)
        )
        .expect("String writer");
        if !entry.alias_groups.is_empty() {
            write!(
                output,
                "\n{}",
                display(
                    &format!("Declared alias groups: {:?}", entry.alias_groups),
                    markdown
                )
            )
            .expect("String writer");
        }
        if let Some(domain) = &entry.value_domain {
            write!(
                output,
                "\n{}",
                display(
                    &format!("Value domain (not followed): {}", domain_label(domain)),
                    markdown
                )
            )
            .expect("String writer");
        }
    }
}
