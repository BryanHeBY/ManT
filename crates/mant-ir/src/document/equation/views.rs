//! Borrowed mathematical relationships, never a second content tree.
use super::{EquationExpression, EquationKind, EquationOperator};

/// Checked operands of one mathematical operation, preserving original wrappers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquationOperationRef<'a> {
    /// Numerator above denominator.
    Fraction {
        /// Numerator.
        numerator: &'a EquationExpression,
        /// Denominator.
        denominator: &'a EquationExpression,
    },
    /// Square root of a radicand.
    SquareRoot {
        /// Complete radicand.
        radicand: &'a EquationExpression,
    },
    /// Base with an ordinary superscript.
    Superscript {
        /// Base.
        base: &'a EquationExpression,
        /// Superscript.
        upper: &'a EquationExpression,
    },
    /// Base with an ordinary subscript.
    Subscript {
        /// Base.
        base: &'a EquationExpression,
        /// Subscript.
        lower: &'a EquationExpression,
    },
    /// Base with aligned ordinary subscript and superscript.
    SubscriptSuperscript {
        /// Base.
        base: &'a EquationExpression,
        /// Subscript.
        lower: &'a EquationExpression,
        /// Superscript.
        upper: &'a EquationExpression,
    },
    /// Upper limit rather than an ordinary superscript.
    UpperLimit {
        /// Base.
        base: &'a EquationExpression,
        /// Upper limit.
        upper: &'a EquationExpression,
    },
    /// Lower limit rather than an ordinary subscript.
    LowerLimit {
        /// Base.
        base: &'a EquationExpression,
        /// Lower limit.
        lower: &'a EquationExpression,
    },
    /// Lower and upper limits around one base.
    LowerUpperLimits {
        /// Base.
        base: &'a EquationExpression,
        /// Lower limit.
        lower: &'a EquationExpression,
        /// Upper limit.
        upper: &'a EquationExpression,
    },
}

/// Recognized interpretation of an already resolved decoration symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquationDecorationKind {
    /// One dot.
    Dot,
    /// Two dots.
    DoubleDot,
    /// Circumflex or hat.
    Hat,
    /// Tilde.
    Tilde,
    /// Line above the expression.
    Overbar,
    /// Line below the expression.
    Underline,
    /// Right-pointing vector arrow.
    Vector,
    /// Bidirectional vector arrow.
    BidirectionalVector,
    /// Other exact symbol; no semantic reconstruction is implied.
    Custom,
}

/// Interpretation and authoritative spelling of one decoration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EquationDecorationRef<'a> {
    /// Recognized mathematical presentation.
    pub kind: EquationDecorationKind,
    /// Exact original resolved symbol, never rewritten by this view.
    pub symbol: &'a str,
}

/// Standard matrix axes borrowing all original column and row owners.
///
/// Each column is a Pile whose children are Sequence or Group rows. Their
/// original fonts, fences and decorations remain on those borrowed nodes.
/// Column lengths may differ; a consumer must not use only the first height.
/// The columns group and columns themselves have no fences or decorations;
/// those nontransparent axes remain ordinary operands instead of this view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EquationMatrixRef<'a> {
    /// Original group owning the columns and any group-local presentation.
    pub columns_group: &'a EquationExpression,
    /// Original columns, in authored column order.
    pub columns: &'a [EquationExpression],
}

impl EquationExpression {
    /// Borrow named operands only when this node is a locally valid operation.
    #[must_use]
    pub fn operation_view(&self) -> Option<EquationOperationRef<'_>> {
        if self.kind != EquationKind::Operation || self.validate_shallow().is_err() {
            return None;
        }
        let children = self.children.as_slice();
        Some(match (self.operator, children) {
            (EquationOperator::Fraction, [numerator, denominator]) => {
                EquationOperationRef::Fraction {
                    numerator,
                    denominator,
                }
            }
            (EquationOperator::SquareRoot, [radicand]) => {
                EquationOperationRef::SquareRoot { radicand }
            }
            (EquationOperator::Superscript, [base, upper]) => {
                EquationOperationRef::Superscript { base, upper }
            }
            (EquationOperator::Subscript, [base, lower]) => {
                EquationOperationRef::Subscript { base, lower }
            }
            (EquationOperator::SubscriptSuperscript, [base, lower, upper]) => {
                EquationOperationRef::SubscriptSuperscript { base, lower, upper }
            }
            (EquationOperator::UpperLimit, [base, upper]) => {
                EquationOperationRef::UpperLimit { base, upper }
            }
            (EquationOperator::LowerLimit, [base, lower]) => {
                EquationOperationRef::LowerLimit { base, lower }
            }
            (EquationOperator::LowerUpperLimits, [base, lower, upper]) => {
                EquationOperationRef::LowerUpperLimits { base, lower, upper }
            }
            _ => return None,
        })
    }

    /// Borrow the exact upper decoration with an optional recognized meaning.
    #[must_use]
    pub fn top_decoration(&self) -> Option<EquationDecorationRef<'_>> {
        self.top.as_deref().map(|symbol| EquationDecorationRef {
            kind: match symbol {
                "˙" | "\u{0307}" => EquationDecorationKind::Dot,
                "¨" | "\u{0308}" => EquationDecorationKind::DoubleDot,
                "^" | "ˆ" | "\u{0302}" => EquationDecorationKind::Hat,
                "~" | "˜" | "\u{0303}" => EquationDecorationKind::Tilde,
                "‾" | "\u{0305}" => EquationDecorationKind::Overbar,
                "→" => EquationDecorationKind::Vector,
                "↔" => EquationDecorationKind::BidirectionalVector,
                _ => EquationDecorationKind::Custom,
            },
            symbol,
        })
    }

    /// Borrow the exact lower decoration without interpreting source escapes.
    #[must_use]
    pub fn bottom_decoration(&self) -> Option<EquationDecorationRef<'_>> {
        self.bottom.as_deref().map(|symbol| EquationDecorationRef {
            kind: if matches!(symbol, "_" | "\u{0332}") {
                EquationDecorationKind::Underline
            } else {
                EquationDecorationKind::Custom
            },
            symbol,
        })
    }

    /// Borrow the standard column/row shape, without expanding a rectangular grid.
    ///
    /// Alternative recovered matrix operands remain available in `children`.
    /// This view scans stored column and row headers only, not their expressions.
    #[must_use]
    pub fn matrix_view(&self) -> Option<EquationMatrixRef<'_>> {
        if self.kind != EquationKind::Matrix || self.validate_shallow().is_err() {
            return None;
        }
        let [group] = self.children.as_slice() else {
            return None;
        };
        if group.kind != EquationKind::Group
            || group.validate_shallow().is_err()
            || group.has_matrix_axis_surface()
        {
            return None;
        }
        let mut headers = 2usize;
        let max_headers = super::EquationValidationLimits::default().max_nodes;
        for column in &group.children {
            headers = headers.checked_add(1)?;
            if headers > max_headers
                || column.kind != EquationKind::Pile
                || column.validate_shallow().is_err()
                || column.has_matrix_axis_surface()
            {
                return None;
            }
            for row in &column.children {
                headers = headers.checked_add(1)?;
                if headers > max_headers
                    || !matches!(row.kind, EquationKind::Sequence | EquationKind::Group)
                    || row.validate_shallow().is_err()
                {
                    return None;
                }
            }
        }
        Some(EquationMatrixRef {
            columns_group: group,
            columns: &group.children,
        })
    }

    fn has_matrix_axis_surface(&self) -> bool {
        self.left.is_some() || self.right.is_some() || self.top.is_some() || self.bottom.is_some()
    }
}
