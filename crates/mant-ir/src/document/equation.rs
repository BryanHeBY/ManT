//! Source-neutral equation structure and a shared readable projection.
//!
//! The projection is `ManT`'s reading contract, not a terminal or `MathML` formatter.

/// Structural role of one parsed equation box.
#[derive(
    serde::Deserialize, serde::Serialize, schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq,
)]
pub enum EquationKind {
    /// Number, variable, operator, or other text atom.
    Text,
    /// Positioned subexpression such as a fraction or script.
    Subexpression,
    /// Group or ordered expression list.
    List,
    /// Vertical pile of expression rows.
    Pile,
    /// Matrix whose parsed children are columns.
    Matrix,
}

/// Parsed font selection for an equation box.
#[derive(
    serde::Deserialize, serde::Serialize, schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq,
)]
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

/// Operator relating children of a parsed subexpression.
#[derive(
    serde::Deserialize, serde::Serialize, schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq,
)]
pub enum EquationPosition {
    /// No positional operator.
    None,
    /// Superscript.
    Superscript,
    /// Subscript and superscript.
    SubscriptSuperscript,
    /// Subscript.
    Subscript,
    /// Upper limit.
    To,
    /// Lower limit.
    From,
    /// Lower and upper limits.
    FromTo,
    /// Fraction.
    Over,
    /// Square root.
    Sqrt,
}

/// Self-contained equation box copied from the parser before it is freed.
#[derive(
    serde::Deserialize, serde::Serialize, schemars::JsonSchema, Clone, Debug, Eq, PartialEq,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EquationExpression {
    /// Box role.
    pub kind: EquationKind,
    /// Parsed font selection.
    pub font: EquationFont,
    /// Positional operator.
    pub position: EquationPosition,
    /// Parsed font size, or the parser's default when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i32>,
    /// Grammar maximum argument count, or no fixed maximum when absent.
    /// This can differ from the actual child count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_args: Option<usize>,
    /// Parser recorded argument count.
    pub actual_args: usize,
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
    /// Ordered children; matrix children retain parsed column order.
    pub children: Vec<Self>,
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
        if self.position == EquationPosition::Sqrt {
            output.push_str("sqrt(");
        }
        if let Some(left) = &self.left {
            output.push_str(left);
        }
        if let Some(text) = &self.text {
            output.push_str(if text == "ldots" { "..." } else { text });
        }
        if self.kind == EquationKind::Matrix {
            self.append_matrix(output);
        } else if self.kind == EquationKind::Subexpression
            && self.position != EquationPosition::None
            && self.position != EquationPosition::Sqrt
        {
            let mut children = self.children.iter();
            if let Some(base) = children.next() {
                if self.position == EquationPosition::Over {
                    append_grouped(base, output);
                } else {
                    base.append_readable(output);
                }
            }
            let operator = match self.position {
                EquationPosition::Over => " / ",
                EquationPosition::Superscript | EquationPosition::To => " ^ ",
                _ => " _ ",
            };
            if let Some(argument) = children.next() {
                output.push_str(operator);
                if self.position == EquationPosition::Over {
                    append_grouped(argument, output);
                } else {
                    argument.append_readable(output);
                }
            }
            if matches!(
                self.position,
                EquationPosition::FromTo | EquationPosition::SubscriptSuperscript
            ) && let Some(upper) = children.next()
            {
                output.push_str(" ^ ");
                upper.append_readable(output);
            }
            for extra in children {
                output.push(' ');
                extra.append_readable(output);
            }
        } else {
            for (index, child) in self.children.iter().enumerate() {
                if index > 0 {
                    output.push(' ');
                }
                child.append_readable(output);
            }
        }
        if let Some(top) = &self.top {
            output.push_str(top);
        }
        if self.bottom.is_some() {
            output.push('_');
        }
        if let Some(right) = &self.right {
            output.push_str(right);
        }
        if self.position == EquationPosition::Sqrt {
            output.push(')');
        }
    }

    fn append_matrix(&self, output: &mut String) {
        // eqn_html.c::eqn_box traverses matrix columns, then pile rows.
        // Use the longest column so a short first column cannot drop later
        // cells, a documented edge case in the pinned HTML renderer.
        let columns = self.children.first().map_or(&[][..], |list| &list.children);
        let rows = columns
            .iter()
            .map(|column| column.children.len())
            .max()
            .unwrap_or(0);
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

fn append_grouped(box_node: &EquationExpression, output: &mut String) {
    let needs_group = box_node.children.len() > 1 && box_node.left.is_none();
    if needs_group {
        output.push('(');
    }
    box_node.append_readable(output);
    if needs_group {
        output.push(')');
    }
}
