//! Source-neutral mathematical structure, checked borrowed views and readable text.
//!
//! Native execution is resolved before publication. Mathematical encoders consume
//! this tree directly; the shared readable projection is not `MathML` or LaTeX.
mod readable;
#[cfg(test)]
mod tests;
mod validation;
mod views;
mod wire;

pub use validation::{EquationValidationError, EquationValidationLimits};
pub use views::{
    EquationDecorationKind, EquationDecorationRef, EquationMatrixRef, EquationOperationRef,
};

/// Source-neutral role of one mathematical expression.
#[derive(
    serde::Deserialize, serde::Serialize, schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq,
)]
#[serde(rename_all = "kebab-case")]
pub enum EquationKind {
    /// Number, variable, operator, function or literal mathematical atom.
    Text,
    /// Readable content whose internal mathematical structure is unavailable.
    Opaque,
    /// Mathematical operation such as a fraction or script.
    Operation,
    /// Ordered expressions without an additional operation boundary.
    Sequence,
    /// Expressions sharing an explicit operation boundary, not visible fences.
    Group,
    /// Vertical pile of expression rows.
    Pile,
    /// Matrix whose children retain column and row structure.
    Matrix,
}

/// Proven mathematical role of a text atom, independently from its font.
#[derive(
    serde::Deserialize,
    serde::Serialize,
    schemars::JsonSchema,
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
)]
#[serde(rename_all = "kebab-case")]
pub enum EquationAtomRole {
    /// No reliable role classification is available.
    #[default]
    Unknown,
    /// Variable or other mathematical identifier.
    Identifier,
    /// Numeric literal.
    Number,
    /// Mathematical operator or separator.
    Operator,
    /// Recognized named mathematical function, not inferred from roman font.
    Function,
    /// One literal mathematical atom, not automatically ordinary prose.
    Literal,
}

impl EquationAtomRole {
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde predicate takes a reference"
    )]
    fn is_unknown(&self) -> bool {
        *self == Self::Unknown
    }
}

/// Operation boundary retained by an opaque mathematical payload.
#[derive(
    serde::Deserialize,
    serde::Serialize,
    schemars::JsonSchema,
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
)]
#[serde(rename_all = "kebab-case")]
pub enum EquationOperandScope {
    /// No additional grouping is required by the payload itself.
    #[default]
    Atomic,
    /// The payload represents a complete compound operand.
    Compound,
}

impl EquationOperandScope {
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde predicate takes a reference"
    )]
    fn is_atomic(&self) -> bool {
        *self == Self::Atomic
    }
}

/// Resolved mathematical font presentation; independent from atom roles.
#[derive(
    serde::Deserialize, serde::Serialize, schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq,
)]
#[serde(rename_all = "kebab-case")]
pub enum EquationFont {
    /// Inherited/default font.
    None,
    /// Roman font.
    Roman,
    /// Bold font.
    Bold,
    /// Fat font, retained separately even when a target maps it to bold.
    Fat,
    /// Italic font.
    Italic,
}

/// Mathematical operator relating explicitly ordered operands.
#[derive(
    serde::Deserialize, serde::Serialize, schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq,
)]
#[serde(rename_all = "kebab-case")]
pub enum EquationOperator {
    /// No mathematical operation.
    None,
    /// Base and superscript.
    Superscript,
    /// Base, subscript and superscript, in that order.
    SubscriptSuperscript,
    /// Base and subscript.
    Subscript,
    /// Base and upper limit, not an ordinary superscript.
    UpperLimit,
    /// Base and lower limit, not an ordinary subscript.
    LowerLimit,
    /// Base, lower limit and upper limit, in that order.
    LowerUpperLimits,
    /// Numerator and denominator.
    Fraction,
    /// One radicand.
    SquareRoot,
}

/// Resolved mathematical structure independent from any source parser.
///
/// `children` is authoritative content. Borrowed views expose operand roles
/// without copying or flattening group, font, fence or matrix boundaries.
#[derive(serde::Serialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EquationExpression {
    /// Mathematical role of this node.
    pub kind: EquationKind,
    /// Resolved font presentation.
    pub font: EquationFont,
    /// Mathematical operation; only Operation nodes may select one.
    pub operator: EquationOperator,
    /// Proven atom role. Only Text nodes may carry a non-unknown role.
    #[serde(default, skip_serializing_if = "EquationAtomRole::is_unknown")]
    pub atom_role: EquationAtomRole,
    /// Scope of an opaque payload; structured expressions determine their own scope.
    #[serde(default, skip_serializing_if = "EquationOperandScope::is_atomic")]
    pub operand_scope: EquationOperandScope,
    /// Exact atom or opaque readable content, including an explicitly empty atom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Independent visible opening fence; empty and absent remain distinct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<String>,
    /// Independent visible closing fence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<String>,
    /// Exact decoration above the complete expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<String>,
    /// Exact decoration below the complete expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<String>,
    /// Ordered operands; matrices retain columns and their original rows.
    pub children: Vec<Self>,
}
