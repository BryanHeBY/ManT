//! Source-neutral findings and their explicit coverage effects.
use super::{SourceKey, SourceSpan};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::num::NonZeroU32;

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
    /// Known source identity without a provable authored position. Mutually
    /// exclusive with `source`; never a substitute for coverage scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_key: Option<SourceKey>,
    /// Source-neutral semantic coverage scope; an authored location, if known,
    /// remains in `source` independently of this scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_scope: Option<CoverageScope>,
}

/// Best-known scope of one semantic coverage finding.
///
/// Non-source keys are nonzero here; their closure against a Fixed document's
/// mark tables is checked when those tables are introduced. Source keys are
/// closed against `Document::sources` at the current wire boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CoverageScope {
    /// The whole document; no local scope is known.
    Document,
    /// A document-local section mark.
    Section {
        /// Nonzero document-local section key.
        key: NonZeroU32,
    },
    /// A document-local owner mark.
    Owner {
        /// Nonzero document-local owner key.
        key: NonZeroU32,
    },
    /// A document-local region mark.
    Region {
        /// Nonzero document-local region key.
        key: NonZeroU32,
    },
    /// A source-table member, without invented line or column coordinates.
    Source {
        /// Source-table key, validated against the document's source table.
        key: SourceKey,
    },
}

impl<'de> Deserialize<'de> for CoverageScope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "kebab-case")]
        enum Wire {
            Document(Empty),
            Section(NonzeroKey),
            Owner(NonzeroKey),
            Region(NonzeroKey),
            Source(SourceKeyWire),
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Empty {}

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct NonzeroKey {
            key: NonZeroU32,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct SourceKeyWire {
            key: SourceKey,
        }

        Ok(match Wire::deserialize(deserializer)? {
            Wire::Document(Empty {}) => Self::Document,
            Wire::Section(NonzeroKey { key }) => Self::Section { key },
            Wire::Owner(NonzeroKey { key }) => Self::Owner { key },
            Wire::Region(NonzeroKey { key }) => Self::Region { key },
            Wire::Source(SourceKeyWire { key }) => Self::Source { key },
        })
    }
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
