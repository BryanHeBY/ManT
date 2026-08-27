use std::fmt;

use crate::{Diagnostic, Document, LimitViolation};

/// Immutable parser result plus non-fatal findings and work counters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseReport {
    /// Bounded immutable syntax document.
    pub document: Document,
    /// Recoverable diagnostics in source order.
    pub diagnostics: Vec<Diagnostic>,
    /// Observable work counters for debugging and benchmark evidence.
    pub statistics: ParseStatistics,
}

/// Counters recorded without exposing mutable parser internals.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParseStatistics {
    /// Total uncompressed bytes accepted across sources.
    pub source_bytes: usize,
    /// Number of top-level and resolved source files.
    pub source_files: usize,
    /// Roff expansion and reparse steps.
    pub expansion_steps: usize,
    /// Nodes in the final immutable AST.
    pub emitted_nodes: usize,
    /// Maximum structural or equation nesting depth observed.
    pub maximum_depth: usize,
    /// Whether a coherent prefix was truncated by a deterministic limit.
    pub truncated: bool,
}

/// Fatal session failure that prevents a coherent bounded report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FatalError {
    /// Stable failure category.
    pub kind: FatalErrorKind,
    /// Human explanation not used as a programmatic discriminator.
    pub message: Box<str>,
}

impl FatalError {
    pub(super) fn invalid_configuration(error: LimitViolation) -> Self {
        Self {
            kind: FatalErrorKind::InvalidConfiguration,
            message: error.to_string().into(),
        }
    }

    pub(super) fn source_limit(name: &str, actual: usize, maximum: usize) -> Self {
        Self {
            kind: FatalErrorKind::SourceLimit,
            message: format!("{name}: source has {actual} bytes; configured limit is {maximum}")
                .into(),
        }
    }

    pub(super) fn source_line_limit(name: &str, actual: usize, maximum: usize) -> Self {
        Self {
            kind: FatalErrorKind::SourceLineLimit,
            message: format!(
                "{name}: source has {actual} physical lines; configured limit is {maximum}"
            )
            .into(),
        }
    }
}

/// Stable categories for a fatal parser boundary failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FatalErrorKind {
    /// `Limits` contains an impossible or zero budget.
    InvalidConfiguration,
    /// Top-level source bytes exceeded the configured input budget.
    SourceLimit,
    /// Top-level source lines exceeded the bounded source-map budget.
    SourceLineLimit,
    /// A source cannot fit the public byte-offset representation.
    SourceTooLargeForSpans,
    /// Future I/O adapters could not read a caller-requested source.
    Read,
    /// Future transport adapter could not decode a requested source.
    Decompression,
    /// A caller selected a feature-gated transport adapter that is unavailable.
    Unsupported,
    /// Internal invariant violation, reserved for bugs rather than source errors.
    Invariant,
}

impl fmt::Display for FatalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for FatalError {}
