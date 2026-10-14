//! Source-neutral equation structure and a shared readable projection.
//!
//! The projection is `ManT`'s reading contract, not a terminal or `MathML` formatter.

/// Source-neutral role of one mathematical expression.
#[derive(
    serde::Deserialize, serde::Serialize, schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq,
)]
#[serde(rename_all = "kebab-case")]
pub enum EquationKind {
    /// Number, variable, operator, or other text atom.
    Text,
    /// Mathematical operation such as a fraction or script.
    Operation,
    /// Ordered expressions without an additional operation boundary.
    Sequence,
    /// Expressions sharing an explicit operation boundary.
    Group,
    /// Vertical pile of expression rows.
    Pile,
    /// Matrix whose children retain column and row structure.
    Matrix,
}

/// Operation boundary of an opaque equation payload.
///
/// A readable text atom can stand for a complete compound expression even
/// when its internal structure is unavailable. This describes its mathematical
/// scope, independently of the producer or why that structure is unavailable.
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
    /// No additional grouping is required by the opaque payload itself.
    #[default]
    Atomic,
    /// The payload is a complete compound operand rather than one atom.
    Compound,
}

impl EquationOperandScope {
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde skip_serializing_if requires a reference-taking predicate"
    )]
    fn is_atomic(&self) -> bool {
        *self == Self::Atomic
    }
}

/// Resolved mathematical font presentation.
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
    /// Fat font.
    Fat,
    /// Italic font.
    Italic,
}

/// Mathematical operator relating the operands of a subexpression.
#[derive(
    serde::Deserialize, serde::Serialize, schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq,
)]
#[serde(rename_all = "kebab-case")]
pub enum EquationOperator {
    /// No mathematical operation.
    None,
    /// Superscript.
    Superscript,
    /// Subscript and superscript.
    SubscriptSuperscript,
    /// Subscript.
    Subscript,
    /// Upper limit.
    UpperLimit,
    /// Lower limit.
    LowerLimit,
    /// Lower and upper limits.
    LowerUpperLimits,
    /// Fraction.
    Fraction,
    /// Square root.
    SquareRoot,
}

/// Resolved mathematical structure independent from a source parser.
#[derive(
    serde::Deserialize, serde::Serialize, schemars::JsonSchema, Clone, Debug, Eq, PartialEq,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EquationExpression {
    /// Mathematical role.
    pub kind: EquationKind,
    /// Resolved font presentation.
    pub font: EquationFont,
    /// Resolved mathematical operator.
    pub operator: EquationOperator,
    /// Resolved font size, or inherited/default presentation when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i32>,
    /// Mathematical scope retained by an opaque text payload.
    #[serde(default, skip_serializing_if = "EquationOperandScope::is_atomic")]
    pub operand_scope: EquationOperandScope,
    /// Atom spelling, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Opening and closing fences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<String>,
    /// Closing fence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<String>,
    /// Decoration above the expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<String>,
    /// Decoration below the expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<String>,
    /// Ordered operands; matrix children retain column order.
    pub children: Vec<Self>,
}

#[derive(Clone, Copy)]
enum ProjectionContext {
    Root,
    Sequence { has_sibling: bool },
    Operand,
    PileRow,
}

struct ProjectionFences<'a> {
    left: Option<&'a str>,
    right: Option<&'a str>,
}

impl ProjectionFences<'_> {
    fn append_open(&self, output: &mut String) {
        if let Some(left) = self.left {
            output.push_str(left);
        }
    }

    fn append_close(&self, output: &mut String) {
        if let Some(right) = self.right {
            output.push_str(right);
        }
    }
}

fn opens_scope(fence: &str) -> bool {
    matches!(fence, "(" | "[" | "{" | "<" | "|" | "‖" | "⌈" | "⌊")
}

fn closes_scope(fence: &str) -> bool {
    matches!(fence, ")" | "]" | "}" | ">" | "|" | "‖" | "⌉" | "⌋")
}

impl EquationExpression {
    /// Derive readable text from the owned structure. This is the sole source
    /// of equation text used by downstream compatibility consumers.
    #[must_use]
    pub fn readable_text(&self) -> String {
        let mut output = String::new();
        self.append_readable(&mut output);
        output
    }

    fn append_readable(&self, output: &mut String) {
        self.append_in_context(output, ProjectionContext::Root);
    }

    fn append_in_context(&self, output: &mut String, context: ProjectionContext) {
        let fences = self.projection_fences();
        let incomplete_fence = self.has_fence() && !self.has_scope_fence_pair();
        let grouped = match context {
            ProjectionContext::Root => {
                incomplete_fence && fences.left.is_none() && fences.right.is_none()
            }
            ProjectionContext::Sequence { has_sibling } => {
                incomplete_fence && (has_sibling || fences.left.is_none() && fences.right.is_none())
                    || (self.is_explicit_group() || self.kind == EquationKind::Pile && has_sibling)
                        && !self.has_fence()
            }
            ProjectionContext::Operand => self.needs_operand_group(),
            ProjectionContext::PileRow => {
                incomplete_fence || self.is_explicit_group() && !self.has_fence()
            }
        };
        if grouped {
            output.push('(');
        }
        if self.operator == EquationOperator::SquareRoot {
            output.push_str("sqrt(");
        }
        fences.append_open(output);
        if let Some(text) = &self.text {
            output.push_str(text);
        }
        if self.kind == EquationKind::Matrix {
            self.append_matrix(output);
        } else if self.kind == EquationKind::Operation
            && self.operator != EquationOperator::None
            && self.operator != EquationOperator::SquareRoot
        {
            let mut children = self.children.iter();
            if let Some(base) = children.next() {
                base.append_in_context(output, ProjectionContext::Operand);
            }
            let operator = match self.operator {
                EquationOperator::Fraction => " / ",
                EquationOperator::Superscript | EquationOperator::UpperLimit => " ^ ",
                _ => " _ ",
            };
            if let Some(argument) = children.next() {
                output.push_str(operator);
                argument.append_in_context(output, ProjectionContext::Operand);
            }
            if matches!(
                self.operator,
                EquationOperator::LowerUpperLimits | EquationOperator::SubscriptSuperscript
            ) && let Some(upper) = children.next()
            {
                output.push_str(" ^ ");
                upper.append_in_context(output, ProjectionContext::Operand);
            }
            for extra in children {
                output.push(' ');
                extra.append_readable(output);
            }
        } else {
            for (index, child) in self.children.iter().enumerate() {
                if index > 0 && !joins_explicit_group(&self.children[index - 1], child) {
                    output.push(' ');
                }
                let child_context = if index == 0 && self.operator == EquationOperator::SquareRoot {
                    // The readable square-root notation provides its own
                    // outer operation boundary.
                    ProjectionContext::Root
                } else if index == 0 && (self.top.is_some() || self.bottom.is_some()) {
                    ProjectionContext::Operand
                } else if self.kind == EquationKind::Pile {
                    // The producer resolves row scope: a transparent singleton
                    // row is a Sequence; a bounded multi-operand row is a Group.
                    ProjectionContext::PileRow
                } else {
                    ProjectionContext::Sequence {
                        has_sibling: self.children.len() > 1,
                    }
                };
                child.append_in_context(output, child_context);
            }
        }
        if let Some(top) = &self.top {
            output.push_str(top);
        }
        if self.bottom.is_some() {
            output.push('_');
        }
        fences.append_close(output);
        if self.operator == EquationOperator::SquareRoot {
            output.push(')');
        }
        if grouped {
            output.push(')');
        }
    }

    fn append_matrix(&self, output: &mut String) {
        const MAX_MATRIX_RECTANGULAR_SLOTS: usize = 16 * 1024;
        const MAX_MATRIX_RECTANGULAR_SEPARATOR_BYTES: usize = 32 * 1024;

        // A matrix's column group contains vertical piles of expression rows.
        // Check every level before projecting that shape; alternative
        // operands remain visible rather than being discarded as empty cells.
        let columns = self.children.first().filter(|list| {
            self.children.len() == 1
                && list.kind == EquationKind::Group
                && list.children.iter().all(|column| {
                    column.kind == EquationKind::Pile
                        && column.children.iter().all(|row| {
                            matches!(row.kind, EquationKind::Sequence | EquationKind::Group)
                        })
                })
        });
        let Some(columns) = columns else {
            output.push_str("matrix(");
            for (index, child) in self.children.iter().enumerate() {
                if index > 0 {
                    output.push(' ');
                }
                child.append_readable(output);
            }
            output.push(')');
            return;
        };
        let columns = &columns.children;
        // Every retained row is part of the matrix, even when its first column
        // is shorter than another column.
        let rows = columns
            .iter()
            .map(|column| column.children.len())
            .max()
            .unwrap_or(0);
        // Printing every row/column slot, including empty cells, can be
        // quadratic in the expression nodes
        // for unequal columns. Bound both visits and the bytes introduced by
        // its ", "/"; " separators; actual cell text is visited only once.
        let actual_cells = columns.iter().try_fold(0usize, |count, column| {
            count.checked_add(column.children.len())
        });
        let rectangular = rows
            .checked_mul(columns.len())
            .zip(actual_cells)
            .is_some_and(|(slots, actual_cells)| {
                slots <= MAX_MATRIX_RECTANGULAR_SLOTS
                    && actual_cells
                        .checked_mul(8)
                        .is_some_and(|relative_limit| slots <= relative_limit)
                    && slots
                        .saturating_sub(1)
                        .checked_mul(2)
                        .is_some_and(|bytes| bytes <= MAX_MATRIX_RECTANGULAR_SEPARATOR_BYTES)
            });
        if !rectangular {
            // Preserve every parsed column and row without synthesizing
            // missing cells. Explicit col(...) wrappers retain the matrix
            // axes when row-major expansion would exceed the budget.
            output.push_str("matrix(");
            for (column_index, column) in columns.iter().enumerate() {
                if column_index > 0 {
                    output.push_str(", ");
                }
                output.push_str("col(");
                for (row_index, row) in column.children.iter().enumerate() {
                    if row_index > 0 {
                        output.push_str("; ");
                    }
                    row.append_readable(output);
                }
                output.push(')');
            }
            output.push(')');
            return;
        }
        output.push_str("matrix(");
        for row in 0..rows {
            if row > 0 {
                output.push_str("; ");
            }
            for (column_index, column) in columns.iter().enumerate() {
                if column_index > 0 {
                    output.push_str(", ");
                }
                if let Some(cell) = column.children.get(row) {
                    cell.append_readable(output);
                }
            }
        }
        output.push(')');
    }
}

fn joins_explicit_group(previous: &EquationExpression, current: &EquationExpression) -> bool {
    // A bounded argument group follows a name or another expression group
    // without a separator. Vertical or matrix arguments remain distinct.
    current.is_explicit_group()
        && current.children.first().is_some_and(|child| {
            child.kind != EquationKind::Pile && child.kind != EquationKind::Matrix
        })
        && (matches!(previous.kind, EquationKind::Sequence | EquationKind::Group)
            || previous.text.as_deref().is_some_and(|text| {
                text.chars()
                    .next()
                    .is_some_and(|character| character == '\\' || character.is_alphabetic())
            }))
}

impl EquationExpression {
    fn projection_fences(&self) -> ProjectionFences<'_> {
        // Left and right fences are independent authored content.
        // Parentheses added by this projection are separate scope markers;
        // never infer or replace an author's visible fence from its mate.
        ProjectionFences {
            left: self.left.as_deref().filter(|fence| !fence.is_empty()),
            right: self.right.as_deref().filter(|fence| !fence.is_empty()),
        }
    }

    fn has_fence(&self) -> bool {
        self.left.is_some() || self.right.is_some()
    }

    fn has_scope_fence_pair(&self) -> bool {
        // Only test each side's ability to mark its own boundary; never
        // require its glyph to be the other side's conventional mate.
        self.left.as_deref().is_some_and(opens_scope)
            && self.right.as_deref().is_some_and(closes_scope)
    }

    fn is_explicit_group(&self) -> bool {
        self.kind == EquationKind::Group
    }

    /// Whether an operator or decorated parent needs to group this operand.
    /// Explicit Groups and decorated expressions retain their scope; a
    /// transparent Sequence inherits its child's scope.
    #[must_use]
    pub fn needs_operand_group(&self) -> bool {
        if self.operand_scope == EquationOperandScope::Compound {
            return true;
        }
        if self.has_fence() {
            return !self.has_scope_fence_pair();
        }
        if self.kind == EquationKind::Matrix || self.operator == EquationOperator::SquareRoot {
            return false;
        }
        if self.is_explicit_group() || self.top.is_some() || self.bottom.is_some() {
            return true;
        }
        if self.kind == EquationKind::Operation && self.operator != EquationOperator::None {
            return true;
        }
        if self.children.len() > 1 || self.text.is_some() && !self.children.is_empty() {
            return true;
        }
        self.children
            .first()
            .is_some_and(EquationExpression::needs_operand_group)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_neutral_text_atom_preserves_literal_ldots() {
        // CVS eqn.c::eqn_next bypasses macro/symbol lookup for quoted text.
        // A standalone IR atom has no parse provenance, so its bytes are
        // literal even when they match a GNU compatibility macro name.
        let expression = EquationExpression {
            kind: EquationKind::Text,
            font: EquationFont::Italic,
            operator: EquationOperator::None,
            size: None,
            operand_scope: EquationOperandScope::Atomic,
            text: Some("ldots".to_owned()),
            left: None,
            right: None,
            top: None,
            bottom: None,
            children: Vec::new(),
        };
        assert_eq!(expression.readable_text(), "ldots");
        let encoded = serde_json::to_string(&expression).expect("encode equation");
        let decoded: EquationExpression = serde_json::from_str(&encoded).expect("decode equation");
        assert_eq!(decoded.readable_text(), "ldots");
    }

    fn atom(value: &str) -> EquationExpression {
        EquationExpression {
            kind: EquationKind::Text,
            font: EquationFont::None,
            operator: EquationOperator::None,
            size: None,
            operand_scope: EquationOperandScope::Atomic,
            text: Some(value.into()),
            left: None,
            right: None,
            top: None,
            bottom: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn source_neutral_scope_distinguishes_groups_sequences_and_opaque_operands() {
        // These are independently authored IR expressions, not parsed roff.
        let mut grouped = atom("");
        grouped.kind = EquationKind::Group;
        grouped.text = None;
        grouped.children = vec![atom("b")];
        let mut sequence = grouped.clone();
        sequence.kind = EquationKind::Sequence;
        sequence.children = vec![atom("a"), grouped.clone()];
        assert_eq!(sequence.readable_text(), "a(b)");
        sequence.children[1].kind = EquationKind::Sequence;
        assert_eq!(sequence.readable_text(), "a b");

        let mut compound = atom("a + b");
        compound.operand_scope = EquationOperandScope::Compound;
        assert_eq!(compound.readable_text(), "a + b");
        let mut fraction = atom("");
        fraction.kind = EquationKind::Operation;
        fraction.operator = EquationOperator::Fraction;
        fraction.text = None;
        fraction.children = vec![compound, atom("c")];
        assert_eq!(fraction.readable_text(), "(a + b) / c");
    }

    #[test]
    fn opaque_scope_is_canonical_and_parser_execution_fields_are_rejected() {
        let atom = atom("x");
        let mut encoded = serde_json::to_value(&atom).unwrap();
        assert!(encoded.get("operandScope").is_none());
        assert_eq!(encoded["kind"], "text");
        assert_eq!(encoded["font"], "none");
        assert_eq!(encoded["operator"], "none");
        for retired in ["expectedArgs", "actualArgs", "summarizedOperandGroup"] {
            let mut hostile = encoded.clone();
            hostile[retired] = serde_json::json!(0);
            assert!(serde_json::from_value::<EquationExpression>(hostile).is_err());
        }
        let mut retired_kind = encoded.clone();
        retired_kind["kind"] = serde_json::json!("List");
        assert!(serde_json::from_value::<EquationExpression>(retired_kind).is_err());
        let mut retired_operator = encoded.clone();
        retired_operator["position"] = serde_json::json!("Over");
        assert!(serde_json::from_value::<EquationExpression>(retired_operator).is_err());
        encoded["operandScope"] = serde_json::json!("compound");
        let compound: EquationExpression = serde_json::from_value(encoded).unwrap();
        assert!(compound.needs_operand_group());
        assert_eq!(compound.readable_text(), "x");
        let encoded = serde_json::to_string(&compound).unwrap();
        let decoded: EquationExpression = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, compound);
    }

    #[test]
    fn independently_authored_fences_are_visible_literals_not_roff_escapes() {
        let mut group = atom("");
        group.kind = EquationKind::Group;
        group.text = None;
        group.children = vec![atom("x")];
        group.left = Some("⌈".into());
        group.right = Some("⌉".into());
        assert!(!group.needs_operand_group());
        assert_eq!(group.readable_text(), "⌈x⌉");

        // A Markdown or foreign producer may store these exact literal bytes.
        // The IR must not run a source decoder or treat them as Unicode fences.
        group.left = Some(r"\[lc]".into());
        group.right = Some(r"\[rc]".into());
        assert!(group.needs_operand_group());
        assert_eq!(group.readable_text(), r"\[lc]x\[rc]");
        let mut fraction = atom("");
        fraction.kind = EquationKind::Operation;
        fraction.operator = EquationOperator::Fraction;
        fraction.text = None;
        fraction.children = vec![group, atom("y")];
        assert_eq!(fraction.readable_text(), r"(\[lc]x\[rc]) / y");
    }
}
