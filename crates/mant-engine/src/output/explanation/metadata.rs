//! Stable human metadata, never reconstructed from rendered source text.
use mant_protocol::{EvidenceBasis, ExplanationEvidence, OutlineNodeReference};
pub(super) fn outcome_name(value: mant_protocol::ExplanationOutcome) -> &'static str {
    match value {
        mant_protocol::ExplanationOutcome::Evidence => "evidence",
        mant_protocol::ExplanationOutcome::NoEvidence => "no-evidence",
    }
}
pub(super) fn kind(e: &ExplanationEvidence) -> Option<mant_ir::EntryKind> {
    if let OutlineNodeReference::DocumentEntry { entry_kind, .. } = e.outline.node {
        Some(entry_kind)
    } else {
        None
    }
}
fn quoted(value: &str) -> String {
    format!("\"{}\"", mant_protocol::sanitize_terminal_text(value))
}
pub(super) fn bases(e: &ExplanationEvidence) -> String {
    e.bases
        .iter()
        .map(|b| match b {
            EvidenceBasis::Name { matches } => {
                if matches.is_empty() {
                    "documented name (details omitted)".into()
                } else {
                    format!(
                        "documented name {}",
                        matches
                            .iter()
                            .map(|m| quoted(&m.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            EvidenceBasis::Form { matches } => {
                if matches.is_empty() {
                    "complete form (details omitted)".into()
                } else {
                    format!(
                        "complete form {}",
                        matches
                            .iter()
                            .map(|m| quoted(&m.text))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            EvidenceBasis::Identity { fields } => format!(
                "exact {}",
                fields
                    .iter()
                    .map(|f| match f {
                        mant_protocol::ExplanationIdentityField::Id => "ID",
                        mant_protocol::ExplanationIdentityField::Path => "path",
                    })
                    .collect::<Vec<_>>()
                    .join(" and ")
            ),
            EvidenceBasis::Literal => "text mention".into(),
            EvidenceBasis::AliasGroup { members } => format!(
                "explicit alias group {}",
                members
                    .iter()
                    .map(|m| quoted(m))
                    .collect::<Vec<_>>()
                    .join(" = ")
            ),
            EvidenceBasis::Related { from, declarations } => format!(
                "related from {} via {}",
                quoted(from),
                declarations
                    .iter()
                    .map(|d| quoted(d))
                    .collect::<Vec<_>>()
                    .join(" → ")
            ),
        })
        .collect::<Vec<_>>()
        .join("; ")
}
pub(super) fn identity_match(
    e: &ExplanationEvidence,
    field: mant_protocol::ExplanationIdentityField,
) -> bool {
    e.bases
        .iter()
        .any(|b| matches!(b, EvidenceBasis::Identity { fields } if fields.contains(&field)))
}
pub(super) fn domain_label(domain: &mant_ir::ValueDomain) -> String {
    match domain {
        mant_ir::ValueDomain::Choices { exhaustive } => format!("choices; exhaustive={exhaustive}"),
        mant_ir::ValueDomain::EntrySet {
            reference,
            entry_kinds,
            ..
        } => {
            let target = match reference {
                mant_ir::SemanticDocumentReference::Document { name, fragment } => format!(
                    "{name}{}",
                    fragment
                        .as_ref()
                        .map(|f| format!("#{f}"))
                        .unwrap_or_default()
                ),
                mant_ir::SemanticDocumentReference::Manual {
                    name,
                    manual_section,
                } => format!("manual/{}/{name}", manual_section.as_deref().unwrap_or("?")),
            };
            format!(
                "entries={}; kinds={}",
                mant_protocol::sanitize_terminal_text(&target),
                entry_kinds
                    .iter()
                    .map(|k| mant_protocol::entry_kind_label(*k))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}
pub(super) fn safe(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control() && !matches!(c, '\n' | '\t') {
                '�'
            } else {
                c
            }
        })
        .collect()
}
pub(super) fn escape(value: &str) -> String {
    let mut output = String::new();
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
            output.push('\\');
        }
        output.push(c);
    }
    output
}

pub(super) fn marked(value: &str, matched: bool) -> String {
    use std::fmt::Write;
    if !matched {
        return escape(value);
    }
    let mut output = String::new();
    for line in value.split_inclusive('\n') {
        let core = line.trim();
        if core.is_empty() {
            output.push_str(line);
            continue;
        }
        let start = line.len() - line.trim_start().len();
        write!(
            output,
            "{}**{}**{}",
            &line[..start],
            escape(core),
            &line[start + core.len()..]
        )
        .expect("String writer");
    }
    output
}
