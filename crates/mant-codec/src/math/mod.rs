//! Bounded, memory-only encoding of resolved mathematical expressions.
//!
//! `MathML` output is a complete `MathML` Core fragment. LaTeX output is a bare
//! mathematical body, without dollar delimiters, using a documented fixed
//! mathematical/amsmath command subset. It is Unicode-aware, not a promise
//! that arbitrary text compiles in an unconfigured traditional TeX engine.
//! Source text never supplies commands, tags, attribute names or CSS.
//!
//! These encoders report target-format losses separately from producer
//! diagnostics. Resource and invalid-expression errors are always fatal;
//! best effort never returns truncated markup or starts an unbounded fallback.
//!
//! Mathematical fonts apply to text atoms and inherit through expression groups.
//! Fences, accents and rule geometry use the target engine's mathematical glyphs;
//! they do not inherit a text atom's font. No pixel-identical glyph metrics are
//! promised. An explicitly requested atom font that the restricted LaTeX profile
//! cannot reproduce is nevertheless a reported loss, not a silent approximation.

mod latex;
mod mathml;
mod symbols;

use std::{collections::HashSet, error::Error, fmt};

use mant_ir::{EquationExpression, EquationFont};

/// Maximum expression nodes visited by one encoding operation.
pub const MAX_MATH_NODES: usize = 16 * 1024;
/// Maximum expression depth accepted by one encoding operation; the root is one.
pub const MAX_MATH_DEPTH: usize = 64;
/// Maximum combined UTF-8 bytes in expression text, fences and decorations.
pub const MAX_MATH_INPUT_BYTES: usize = 1024 * 1024;
/// Maximum rectangular matrix slots expanded by one encoding operation.
pub const MAX_MATH_MATRIX_SLOTS: usize = 16 * 1024;
/// Maximum encoded UTF-8 bytes, including generated markup.
pub const MAX_MATH_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

/// A supported mathematical output representation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MathFormat {
    /// A standalone `MathML` Core `math` element.
    #[default]
    Mathml,
    /// A bare, safely encoded mathematical/amsmath LaTeX body.
    Latex,
}

/// Handling of facts that the selected mathematical format cannot express.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MathPolicy {
    /// Reject the first reported representational loss.
    #[default]
    Strict,
    /// Retain safe readable content and report representational losses.
    BestEffort,
}

/// Presentation controls for one formula, never stored in the source IR.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MathOptions {
    /// Target mathematical representation.
    pub format: MathFormat,
    /// Whether the formula is presented as a display rather than inline math.
    /// LaTeX remains delimiter-free; its enclosing consumer owns delimiters.
    pub display: bool,
    /// Whether representational losses are accepted and reported.
    pub policy: MathPolicy,
}

/// A mathematical representation and its target-local loss report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MathArtifact {
    /// Complete markup/body; resource errors never return partial output.
    pub text: String,
    /// Losses incurred by this encoding, not source document diagnostics.
    pub losses: Vec<MathLoss>,
}

/// A specific mathematical fact that could not be faithfully represented.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MathLossKind {
    /// Only a readable payload, rather than the original structure, is known.
    OpaqueStructure,
    /// The mathematical role of a text atom is unspecified.
    UnknownAtomRole,
    /// A mathematical atom cannot retain its declared role in the target subset.
    UnsupportedAtom,
    /// A glyph is outside the selected LaTeX symbol subset.
    UnsupportedSymbol,
    /// A control character cannot be represented as mathematical content.
    UnsupportedCharacter,
    /// A custom decoration has no supported LaTeX accent command.
    UnsupportedDecoration,
    /// A fence has no supported LaTeX delimiter spelling.
    UnsupportedFence,
    /// Matrix operands do not have a checked column/row structure.
    UnsupportedMatrix,
    /// Missing matrix cells require representational padding.
    IrregularMatrix,
    /// Literal whitespace cannot retain its exact mathematical appearance.
    LiteralWhitespace,
    /// The restricted LaTeX profile cannot express a requested font reliably.
    UnsupportedFont,
}

/// One loss at an expression-local child path; an empty path is the root.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MathLoss {
    /// Child indices relative to the expression passed to the encoder.
    pub path: Vec<usize>,
    /// The unsupported mathematical fact.
    pub kind: MathLossKind,
}

/// Work limit that prevented a complete, safe mathematical encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MathLimit {
    /// Too many expression nodes.
    Nodes,
    /// Expression nesting is too deep.
    Depth,
    /// Source text/decorations exceed the input allowance.
    InputBytes,
    /// Rectangular matrix expansion exceeds the operation allowance.
    MatrixSlots,
    /// Generated markup exceeds the output allowance.
    OutputBytes,
}

/// A formula that cannot be encoded under the selected contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MathError {
    /// A node violates the source-neutral expression contract.
    InvalidExpression {
        /// Child path to the invalid expression.
        path: Vec<usize>,
        /// Structural validation explanation.
        message: String,
    },
    /// No complete output can be returned within a fixed resource limit.
    LimitExceeded {
        /// The exhausted allowance.
        limit: MathLimit,
    },
    /// Strict encoding rejected a representational loss.
    Loss(MathLoss),
}

impl fmt::Display for MathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidExpression { path, message } => {
                write!(
                    formatter,
                    "invalid mathematical expression at {path:?}: {message}"
                )
            }
            Self::LimitExceeded { limit } => {
                write!(formatter, "mathematical encoding exceeded {limit:?} limit")
            }
            Self::Loss(loss) => write!(
                formatter,
                "mathematical encoding cannot preserve {:?} at {:?}",
                loss.kind, loss.path
            ),
        }
    }
}

impl Error for MathError {}

/// Encode resolved mathematical structure without replaying a source language.
///
/// Input is checked iteratively before any recursive rendering or readable
/// fallback. Limits also apply to manually built, not merely parsed, IR.
///
/// # Errors
///
/// Returns [`MathError`] for malformed/oversized IR or a loss rejected by strict
/// policy. Resource errors are fatal under both policies.
pub fn encode_equation(
    expression: &EquationExpression,
    options: MathOptions,
) -> Result<MathArtifact, MathError> {
    preflight(expression)?;
    let mut encoder = Encoder {
        options,
        output: String::new(),
        losses: Vec::new(),
        seen_losses: HashSet::new(),
        path: Vec::new(),
    };
    match options.format {
        MathFormat::Mathml => mathml::encode(&mut encoder, expression)?,
        MathFormat::Latex => latex::encode(&mut encoder, expression)?,
    }
    Ok(MathArtifact {
        text: encoder.output,
        losses: encoder.losses,
    })
}

fn preflight(root: &EquationExpression) -> Result<(), MathError> {
    let mut pending = vec![(root, 1usize, Vec::new())];
    let mut nodes = 0usize;
    let mut bytes = 0usize;
    let mut matrices = Vec::new();
    while let Some((expression, depth, path)) = pending.pop() {
        if depth > MAX_MATH_DEPTH {
            return Err(limit(MathLimit::Depth));
        }
        nodes += 1;
        if nodes > MAX_MATH_NODES
            || expression.children.len() > MAX_MATH_NODES - nodes - pending.len()
        {
            return Err(limit(MathLimit::Nodes));
        }
        expression
            .validate_shallow()
            .map_err(|error| MathError::InvalidExpression {
                path: path.clone(),
                message: error.to_string(),
            })?;
        for value in [
            &expression.text,
            &expression.left,
            &expression.right,
            &expression.top,
            &expression.bottom,
        ]
        .into_iter()
        .flatten()
        {
            bytes = bytes.saturating_add(value.len());
            if bytes > MAX_MATH_INPUT_BYTES {
                return Err(limit(MathLimit::InputBytes));
            }
        }
        if expression.kind == mant_ir::EquationKind::Matrix {
            matrices.push(expression);
        }
        for (index, child) in expression.children.iter().enumerate().rev() {
            let mut child_path = path.clone();
            child_path.push(index);
            pending.push((child, depth + 1, child_path));
        }
    }
    // A matrix view inspects row headers. Interpret it only after *all* input
    // nodes are known to fit, including hand-built, over-wide input graphs.
    let mut slots = 0usize;
    for expression in matrices {
        if let Some(matrix) = expression.matrix_view() {
            let rows = matrix
                .columns
                .iter()
                .map(|column| column.children.len())
                .max()
                .unwrap_or(0);
            slots = slots.saturating_add(rows.saturating_mul(matrix.columns.len()));
            if slots > MAX_MATH_MATRIX_SLOTS {
                return Err(limit(MathLimit::MatrixSlots));
            }
        }
    }
    Ok(())
}

fn limit(limit: MathLimit) -> MathError {
    MathError::LimitExceeded { limit }
}

struct Encoder {
    options: MathOptions,
    output: String,
    losses: Vec<MathLoss>,
    seen_losses: HashSet<MathLoss>,
    path: Vec<usize>,
}

impl Encoder {
    fn push(&mut self, value: &str) -> Result<(), MathError> {
        if value.len() > MAX_MATH_OUTPUT_BYTES - self.output.len() {
            return Err(limit(MathLimit::OutputBytes));
        }
        self.output.push_str(value);
        Ok(())
    }

    fn loss(&mut self, kind: MathLossKind) -> Result<(), MathError> {
        // A run of unsupported glyphs usually repeats one loss at one atom.
        // Avoid allocating its child path once per input character.
        if self
            .losses
            .last()
            .is_some_and(|last| last.kind == kind && last.path == self.path)
        {
            return Ok(());
        }
        let loss = MathLoss {
            path: self.path.clone(),
            kind,
        };
        if self.options.policy == MathPolicy::Strict {
            return Err(MathError::Loss(loss));
        }
        if self.seen_losses.insert(loss.clone()) {
            self.losses.push(loss);
        }
        Ok(())
    }

    fn child(
        &mut self,
        index: usize,
        action: impl FnOnce(&mut Self) -> Result<(), MathError>,
    ) -> Result<(), MathError> {
        self.path.push(index);
        let result = action(self);
        self.path.pop();
        result
    }
}

fn effective_font(expression: &EquationExpression, inherited: EquationFont) -> EquationFont {
    if expression.font == EquationFont::None {
        inherited
    } else {
        expression.font
    }
}

fn irregular_matrix(columns: &[EquationExpression]) -> bool {
    columns.first().is_some_and(|first| {
        columns
            .iter()
            .any(|column| column.children.len() != first.children.len())
    })
}

fn unsupported_whitespace(text: &str) -> bool {
    text.starts_with(char::is_whitespace)
        || text.ends_with(char::is_whitespace)
        || text
            .chars()
            .any(|character| character != ' ' && character.is_whitespace())
        || text.contains("  ")
}

fn safe_character(character: char) -> bool {
    (!character.is_control() || matches!(character, '\n' | '\r' | '\t'))
        && !matches!(character, '\u{fffe}' | '\u{ffff}')
}

fn matrix_loss(expression: &EquationExpression) -> Option<MathLossKind> {
    if expression.kind != mant_ir::EquationKind::Matrix {
        return None;
    }
    expression
        .matrix_view()
        .is_none()
        .then_some(MathLossKind::UnsupportedMatrix)
}

#[cfg(test)]
mod tests;
