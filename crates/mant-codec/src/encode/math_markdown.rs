//! Explicit mathematical Markdown with operation-local conversion accounting.
mod context;
mod preflight;
#[cfg(test)]
mod tests;

use super::{MarkdownArtifact, MarkdownOptions};
use crate::ResolvedContent;
use crate::math::{MathError, MathLossKind, MathPolicy};
pub(super) use context::{MathContext, RenderOptions};
use std::{error::Error, fmt};

/// Maximum independently owned formulas in one mathematical Markdown operation.
pub const MAX_MARKDOWN_MATH_FORMULAS: usize = 4096;
/// Maximum cumulative expression validation visits or source-context traversal work.
pub const MAX_MARKDOWN_MATH_NODES: usize = 65_536;
/// Maximum cumulative stored mathematical UTF-8 bytes charged by one operation.
pub const MAX_MARKDOWN_MATH_INPUT_BYTES: usize = 4 * 1024 * 1024;
/// Maximum cached LaTeX bodies and mathematical framing.
/// Checked export also reserves eight bytes per readable input byte and 32
/// framing bytes. Default best effort emits readable payloads independently.
pub const MAX_MARKDOWN_MATH_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
/// Maximum retained issue details; the summary still reports omitted details.
pub const MAX_MARKDOWN_MATH_ISSUES: usize = 256;

/// Mathematical content that required an explicit Markdown approximation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkdownMathIssueKind {
    /// A loss reported by the restricted mathematical encoder.
    Encoding(MathLossKind),
    /// A legacy equation has no authoritative expression to encode.
    MissingStructure,
    /// A literal code surface cannot contain active mathematical markup.
    LiteralContext,
    /// A portable reading-table surface cannot contain active mathematical markup.
    TableContext,
    /// The final phrasing could not preserve the mathematical delimiters.
    DelimiterContext,
    /// The expression or its readable cache violates the mathematical contract.
    InvalidEquation,
    /// A per-expression encoding allowance requires readable fallback.
    EncodingLimit(crate::math::MathLimit),
    /// The operation's mathematical allowance requires readable fallback.
    BudgetLimit(MarkdownMathLimit),
}

/// One issue at an artifact-local formula and expression-local child path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownMathIssue {
    /// Zero-based formula number assigned on its first actual rendering visit.
    pub formula: usize,
    /// Child indices inside that formula; context-level findings use an empty path.
    pub path: Vec<usize>,
    /// The accepted approximation.
    pub kind: MarkdownMathIssueKind,
}

/// Tracked formula counts and bounded conversion-detail accounting.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MarkdownMathSummary {
    /// Further formula owners use readable output after the mathematical budget.
    /// Counts describe tracked owners when this flag is true.
    pub budget_exhausted: bool,
    /// Distinct original formula owners visited by the renderer.
    pub formulas: usize,
    /// Formula owners emitted as mathematical Markdown.
    pub encoded_formulas: usize,
    /// Formula owners retained in readable, non-mathematical contexts.
    pub fallback_formulas: usize,
    /// Number of issues in the final accepted rendering branches.
    pub issues: usize,
    /// Issue details not retained under the operation's detail limit.
    pub details_omitted: usize,
}

/// Resource exhausted by a complete mathematical Markdown operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkdownMathLimit {
    /// Too many independently owned formulas.
    Formulas,
    /// Cumulative expression nodes or context-walk work exceed the allowance.
    Nodes,
    /// Mathematical or context nesting exceeds the encoding profile's depth.
    Depth,
    /// Cumulative mathematical input text exceeds the allowance.
    InputBytes,
    /// Cached mathematical output or reserved readable framing exceeds the allowance.
    OutputBytes,
}

/// Failure that prevents publishing complete mathematical Markdown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MarkdownMathError {
    /// Mathematical Markdown cannot claim the portable semantic reimport contract.
    PreserveSemantics,
    /// An original expression or its readable cache violates the IR contract.
    InvalidEquation {
        /// Artifact-local formula number.
        formula: usize,
        /// Bounded structural validation explanation, not source commands.
        message: String,
    },
    /// Mathematical encoding failed; partial output is never returned.
    Encoding {
        /// Artifact-local formula number.
        formula: usize,
        /// Mathematical encoder failure.
        error: MathError,
    },
    /// Strict mathematical Markdown cannot represent this source context.
    UnsupportedContext {
        /// Artifact-local formula number.
        formula: usize,
        /// The context requiring readable fallback.
        kind: MarkdownMathIssueKind,
    },
    /// The complete operation exceeded a resource bound, under either policy.
    LimitExceeded {
        /// Exhausted operation allowance.
        limit: MarkdownMathLimit,
    },
}

impl fmt::Display for MarkdownMathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PreserveSemantics => formatter.write_str(
                "mathematical Markdown cannot preserve portable semantic reimport declarations",
            ),
            Self::InvalidEquation { formula, message } => {
                write!(formatter, "invalid formula {formula}: {message}")
            }
            Self::Encoding { formula, error } => write!(formatter, "formula {formula}: {error}"),
            Self::UnsupportedContext { formula, kind } => write!(
                formatter,
                "formula {formula} requires readable fallback in {kind:?}"
            ),
            Self::LimitExceeded { limit } => {
                write!(formatter, "mathematical Markdown exceeded {limit:?} limit")
            }
        }
    }
}

impl Error for MarkdownMathError {}

/// Final Markdown bytes, matching source-coordinate maps and conversion issues.
pub struct MathMarkdownArtifact<'src> {
    markdown: MarkdownArtifact<'src>,
}

impl<'src> MathMarkdownArtifact<'src> {
    /// Borrow the exact final bytes and source-bound node ranges.
    #[must_use]
    pub fn markdown(&self) -> &MarkdownArtifact<'src> {
        &self.markdown
    }

    /// Mathematical issues retained under the bounded detail allowance.
    #[must_use]
    pub fn issues(&self) -> &[MarkdownMathIssue] {
        self.markdown.math_issues()
    }

    /// Complete issue and formula counts for the final accepted branches.
    #[must_use]
    pub fn summary(&self) -> MarkdownMathSummary {
        self.markdown.math_summary()
    }

    /// Consume the artifact when no borrowed source-coordinate map is needed.
    #[must_use]
    pub fn into_text(self) -> String {
        self.markdown.into_text()
    }
}

/// Encode checked mathematical Markdown without reinterpreting readable caches.
///
/// This API's formula identities borrow original
/// immutable IR: the document-artifact route has no presentation projection,
/// and definition/list root assembly only borrows or moves original roots.
/// Code and portable table surfaces retain readable formulas with context issues.
/// Selecting `MarkdownMath::Readable` delegates to ordinary readable export.
/// Search coordinates use `MarkdownOptions::READABLE_ADDRESSABLE` separately.
///
/// # Errors
/// Rejects semantic reimport options, malformed expressions, any resource limit,
/// or a mathematical/context loss rejected by `policy`. No partial artifact is
/// returned, and target-format issues never mutate document diagnostics.
pub fn render_math_markdown(
    query: &ResolvedContent,
    options: MarkdownOptions,
    policy: MathPolicy,
) -> Result<MathMarkdownArtifact<'_>, MarkdownMathError> {
    if options.math == super::MarkdownMath::Readable {
        return Ok(MathMarkdownArtifact {
            markdown: super::artifact::render_markdown_artifact(query, options, true),
        });
    }
    if options.preserve_semantics {
        return Err(MarkdownMathError::PreserveSemantics);
    }
    preflight::check(query)?;
    let context = MathContext::new(policy);
    let mut markdown = super::artifact::render_markdown_with_context(
        query,
        RenderOptions {
            options,
            math: Some(&context),
        },
        true,
    );
    let (issues, summary) = context.finish()?;
    markdown.math_issues = issues;
    markdown.math_summary = summary;
    Ok(MathMarkdownArtifact { markdown })
}
