//! Local node contracts and bounded, non-recursive whole-expression validation.
use super::{
    EquationAtomRole, EquationExpression, EquationKind, EquationOperandScope, EquationOperator,
};
use std::fmt;

/// Work limits for validating an independently constructed expression.
///
/// Text bytes include atoms, opaque content, fences and decorations. Validation
/// visits every stored node once and uses stack space proportional to depth;
/// it never expands a matrix or generates the readable projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EquationValidationLimits {
    /// Maximum number of stored expression nodes, including the root.
    pub max_nodes: usize,
    /// Maximum node depth; the root is at depth one.
    pub max_depth: usize,
    /// Maximum total UTF-8 bytes in the expression's stored string fields.
    pub max_text_bytes: usize,
}

impl Default for EquationValidationLimits {
    fn default() -> Self {
        Self {
            max_nodes: 65_536,
            max_depth: 256,
            max_text_bytes: 4 * 1024 * 1024,
        }
    }
}

/// A local contract failure or a validation work limit, without source execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EquationValidationError {
    /// The node's kind, payload, role, scope or operands do not agree.
    InvalidNode(&'static str),
    /// The node count exceeds the caller's budget.
    NodeLimit,
    /// The expression is deeper than the caller's budget.
    DepthLimit,
    /// Stored content exceeds the caller's UTF-8 byte budget.
    TextByteLimit,
}

impl fmt::Display for EquationValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidNode(message) => message,
            Self::NodeLimit => "equation validation node limit exceeded",
            Self::DepthLimit => "equation validation depth limit exceeded",
            Self::TextByteLimit => "equation validation text byte limit exceeded",
        })
    }
}

impl std::error::Error for EquationValidationError {}

impl EquationExpression {
    /// Check this node's combinations and operand count without visiting children.
    ///
    /// Empty text and empty groups remain authored content. Matrix operands may
    /// retain recovered, non-column structures; `matrix_view` distinguishes the
    /// standard column/row shape without dropping those alternative operands.
    ///
    /// # Errors
    /// Returns `InvalidNode` when payload, atom role, scope, operator or operand
    /// count does not agree with the node kind. Children are not traversed.
    pub fn validate_shallow(&self) -> Result<(), EquationValidationError> {
        use EquationValidationError::InvalidNode;
        if self.kind != EquationKind::Text && self.atom_role != EquationAtomRole::Unknown {
            return Err(InvalidNode("only text equations may have an atom role"));
        }
        if self.kind != EquationKind::Opaque && self.operand_scope != EquationOperandScope::Atomic {
            return Err(InvalidNode(
                "only opaque equations may select an operand scope",
            ));
        }
        if matches!(self.kind, EquationKind::Text | EquationKind::Opaque) {
            if self.text.is_none()
                || !self.children.is_empty()
                || self.operator != EquationOperator::None
            {
                return Err(InvalidNode(
                    "text and opaque equations require text without children or an operator",
                ));
            }
            return Ok(());
        }
        if self.text.is_some() {
            return Err(InvalidNode("structured equations must not also carry text"));
        }
        if self.kind != EquationKind::Operation {
            return if self.operator == EquationOperator::None {
                Ok(())
            } else {
                Err(InvalidNode(
                    "only operation equations may select an operator",
                ))
            };
        }
        let operands = match self.operator {
            EquationOperator::None => {
                return Err(InvalidNode("operation equation requires an operator"));
            }
            EquationOperator::SquareRoot => 1,
            EquationOperator::SubscriptSuperscript | EquationOperator::LowerUpperLimits => 3,
            EquationOperator::Superscript
            | EquationOperator::Subscript
            | EquationOperator::UpperLimit
            | EquationOperator::LowerLimit
            | EquationOperator::Fraction => 2,
        };
        if self.children.len() != operands {
            return Err(InvalidNode(
                "equation operator has an invalid operand count",
            ));
        }
        Ok(())
    }

    /// Validate the complete stored tree under explicit work limits.
    ///
    /// Local deserialization checks are constant-time per node; this separate
    /// root operation supplies aggregate limits without revalidating each
    /// subtree quadratically during recursive deserialization.
    ///
    /// # Errors
    /// Returns the first local contract failure, or `NodeLimit`, `DepthLimit`
    /// or `TextByteLimit` before exceeding the corresponding caller budget.
    pub fn validate(
        &self,
        limits: EquationValidationLimits,
    ) -> Result<(), EquationValidationError> {
        let mut nodes = 0usize;
        let mut bytes = 0usize;
        let mut check = |node: &Self, depth: usize| {
            if nodes >= limits.max_nodes {
                return Err(EquationValidationError::NodeLimit);
            }
            nodes += 1;
            if depth > limits.max_depth {
                return Err(EquationValidationError::DepthLimit);
            }
            node.validate_shallow()?;
            for text in [&node.text, &node.left, &node.right, &node.top, &node.bottom]
                .into_iter()
                .flatten()
            {
                bytes = bytes
                    .checked_add(text.len())
                    .ok_or(EquationValidationError::TextByteLimit)?;
                if bytes > limits.max_text_bytes {
                    return Err(EquationValidationError::TextByteLimit);
                }
            }
            Ok(())
        };
        check(self, 1)?;
        let mut stack = vec![self.children.iter()];
        while let Some(children) = stack.last_mut() {
            if let Some(child) = children.next() {
                check(child, stack.len() + 1)?;
                stack.push(child.children.iter());
            } else {
                stack.pop();
            }
        }
        Ok(())
    }

    /// Verify the shared text cache only after bounded structural validation.
    ///
    /// # Errors
    /// Returns a default-budget validation error, or `InvalidNode` when `value`
    /// differs from the owned expression's readable projection.
    pub fn validate_projection(&self, value: &str) -> Result<(), EquationValidationError> {
        self.validate(EquationValidationLimits::default())?;
        if self.readable_text() != value {
            return Err(EquationValidationError::InvalidNode(
                "equation text does not match its owned expression",
            ));
        }
        Ok(())
    }
}
