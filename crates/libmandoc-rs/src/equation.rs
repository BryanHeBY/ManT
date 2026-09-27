//! Owned eqn(7) syntax and one bounded, deterministic readable projection.
//!
//! The node vocabulary follows pinned CVS `eqn.h`; the textual projection is
//! `ManT`'s reading contract, not a copy of a terminal or `MathML` formatter.

/// Structural role of one native equation box.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EquationKind {
    /// Number, variable, operator, or other text atom.
    Text,
    /// Positioned subexpression such as a fraction or script.
    Subexpression,
    /// Group or ordered expression list.
    List,
    /// Vertical pile of expression rows.
    Pile,
    /// Matrix whose native children are columns.
    Matrix,
}

/// Native font selection for an equation box.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

/// Operator relating children of a native subexpression.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EquationBox {
    /// Box role.
    pub kind: EquationKind,
    /// Native font selection.
    pub font: EquationFont,
    /// Positional operator.
    pub position: EquationPosition,
    /// Native font size; `i32::MIN` denotes the parser default.
    pub size: i32,
    /// Grammar expected argument count; can differ from actual children.
    pub expected_args: usize,
    /// Parser recorded argument count.
    pub actual_args: usize,
    /// Atom spelling, when present.
    pub text: Option<String>,
    /// Opening and closing fences.
    pub left: Option<String>,
    /// Closing fence.
    pub right: Option<String>,
    /// Decoration above the expression.
    pub top: Option<String>,
    /// Decoration below the expression.
    pub bottom: Option<String>,
    /// Ordered children; matrix children retain native column order.
    pub children: Vec<Self>,
}

impl EquationBox {
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
                append_grouped(base, output);
            }
            let operator = match self.position {
                EquationPosition::Over => " / ",
                EquationPosition::Superscript | EquationPosition::To => " ^ ",
                _ => " _ ",
            };
            if let Some(argument) = children.next() {
                output.push_str(operator);
                append_grouped(argument, output);
            }
            if matches!(
                self.position,
                EquationPosition::FromTo | EquationPosition::SubscriptSuperscript
            ) && let Some(upper) = children.next()
            {
                output.push_str(" ^ ");
                append_grouped(upper, output);
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
        // CVS eqn_html.c::eqn_box enters its row/column branch only for a
        // non-singleton List operand. Other operands remain ordinary boxes.
        // Check every level before interpreting it as columns and rows;
        // malformed or alternative shapes still own visible operands.
        let columns = self.children.first().filter(|list| {
            self.children.len() == 1
                && list.kind == EquationKind::List
                && list.expected_args != 1
                && list.children.iter().all(|column| {
                    column.kind == EquationKind::Pile
                        && column
                            .children
                            .iter()
                            .all(|row| row.kind == EquationKind::List)
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
        // Unlike the pinned HTML renderer, retain rows of later columns when
        // the first column is shorter.
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

fn append_grouped(box_node: &EquationBox, output: &mut String) {
    let needs_group = box_node.needs_operand_group();
    if needs_group {
        output.push('(');
    }
    box_node.append_readable(output);
    if needs_group {
        output.push(')');
    }
}

impl EquationBox {
    /// Whether this box needs parentheses when used as an infix or script
    /// operand in the readable projection. A transparent native List inherits
    /// the requirement from its single child.
    #[must_use]
    pub fn needs_operand_group(&self) -> bool {
        if self.left.is_some() || self.right.is_some() {
            return false;
        }
        if self.kind == EquationKind::Matrix || self.position == EquationPosition::Sqrt {
            return false;
        }
        if self.kind == EquationKind::Subexpression && self.position != EquationPosition::None {
            return true;
        }
        if self.children.len() > 1 || self.text.is_some() && !self.children.is_empty() {
            return true;
        }
        self.children
            .first()
            .is_some_and(EquationBox::needs_operand_group)
    }
}
