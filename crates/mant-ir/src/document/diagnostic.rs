//! Source-neutral findings and their explicit coverage effects.
use super::SourceSpan;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Recoverable parser or IR validation finding attached to the document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// Severity of the finding.
    pub level: DiagnosticLevel,
    /// Explicit effect on semantic extraction, independent of severity or code.
    /// Required on the wire: missing producer coverage must never mean complete.
    pub impact: DiagnosticImpact,
    /// Stable machine-readable code, when the producer defines one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Concise human-readable explanation.
    pub message: String,
    /// Original source location associated with the finding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
}

/// Producer-declared effect of a finding on semantic extraction completeness.
/// This is not a measure of rendering fidelity or proof of exhaustive recall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticImpact {
    /// This finding does not invalidate semantic extraction coverage.
    None,
    /// Semantic declarations, facts or coverage were rejected or incomplete.
    SemanticCoverage,
}

/// Whether all producers and shared validators permit a complete projection.
/// Consumers must not infer this effect from diagnostic severity, text or codes.
#[must_use]
pub fn semantics_complete(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .all(|diagnostic| diagnostic.impact != DiagnosticImpact::SemanticCoverage)
}

/// Severity reported by the parser without turning useful output into failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticLevel {
    /// Non-semantic source style issue.
    Style,
    /// Recoverable source defect or portability concern.
    Warning,
    /// Invalid source that left partial output available.
    Error,
    /// Valid construct that the active parser cannot represent fully.
    Unsupported,
}
