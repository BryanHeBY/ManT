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
    /// A depth-summarized leaf still needs grouping when used as an operand.
    /// Absent for ordinary parsed boxes.
    #[serde(default, skip_serializing_if = "is_false")]
    pub summarized_operand_group: bool,
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

#[derive(Clone, Copy)]
enum ProjectionContext {
    Root,
    Sequence { has_sibling: bool },
    Operand,
    PileRow,
}

struct ProjectionFences<'a> {
    before_left: Option<&'static str>,
    left: Option<&'a str>,
    before_right: Option<&'static str>,
    right: Option<&'a str>,
    after_right: Option<&'static str>,
}

impl ProjectionFences<'_> {
    fn append_open(&self, output: &mut String) {
        if let Some(before_left) = self.before_left {
            output.push_str(before_left);
        }
        if let Some(left) = self.left {
            output.push_str(left);
        }
    }

    fn append_close(&self, output: &mut String) {
        if let Some(before_right) = self.before_right {
            output.push_str(before_right);
        }
        if let Some(right) = self.right {
            output.push_str(right);
        }
        if let Some(after_right) = self.after_right {
            output.push_str(after_right);
        }
    }
}

fn matching_right(left: &str) -> Option<&'static str> {
    match left {
        "(" => Some(")"),
        "[" => Some("]"),
        "{" => Some("}"),
        "<" => Some(">"),
        "|" => Some("|"),
        "‖" => Some("‖"),
        "⌈" => Some("⌉"),
        "\\[lc]" => Some("\\[rc]"),
        "⌊" => Some("⌋"),
        "\\[lf]" => Some("\\[rf]"),
        _ => None,
    }
}

fn matching_left(right: &str) -> Option<&'static str> {
    match right {
        ")" => Some("("),
        "]" => Some("["),
        "}" => Some("{"),
        ">" => Some("<"),
        "|" => Some("|"),
        "‖" => Some("‖"),
        "⌉" => Some("⌈"),
        "\\[rc]" => Some("\\[lc]"),
        "⌋" => Some("⌊"),
        "\\[rf]" => Some("\\[lf]"),
        _ => None,
    }
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
        let grouped = match context {
            ProjectionContext::Root => false,
            ProjectionContext::Sequence { has_sibling } => {
                (self.is_explicit_group() || self.kind == EquationKind::Pile && has_sibling)
                    && self.left.is_none()
                    && self.right.is_none()
            }
            ProjectionContext::Operand => self.needs_operand_group(),
            ProjectionContext::PileRow => {
                self.is_explicit_group()
                    && self.actual_args > 1
                    && self.left.is_none()
                    && self.right.is_none()
            }
        };
        if grouped {
            output.push('(');
        }
        if self.position == EquationPosition::Sqrt {
            output.push_str("sqrt(");
        }
        fences.append_open(output);
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
                base.append_in_context(output, ProjectionContext::Operand);
            }
            let operator = match self.position {
                EquationPosition::Over => " / ",
                EquationPosition::Superscript | EquationPosition::To => " ^ ",
                _ => " _ ",
            };
            if let Some(argument) = children.next() {
                output.push_str(operator);
                argument.append_in_context(output, ProjectionContext::Operand);
            }
            if matches!(
                self.position,
                EquationPosition::FromTo | EquationPosition::SubscriptSuperscript
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
                let joins = self.kind != EquationKind::Pile || child.actual_args != 1;
                if index > 0 && !(joins && joins_explicit_group(&self.children[index - 1], child)) {
                    output.push(' ');
                }
                let child_context = if index == 0 && self.position == EquationPosition::Sqrt {
                    // CVS eqn_term.c::eqn_box supplies the sqrt operand's
                    // outer delimiters itself; the readable sqrt(...) does too.
                    ProjectionContext::Root
                } else if index == 0 && (self.top.is_some() || self.bottom.is_some()) {
                    ProjectionContext::Operand
                } else if self.kind == EquationKind::Pile {
                    // CVS eqn_term.c::eqn_box skips a singleton row List and
                    // groups only rows containing multiple expressions.
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
        if self.position == EquationPosition::Sqrt {
            output.push(')');
        }
        if grouped {
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
                && list.expected_args != Some(1)
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

fn joins_explicit_group(previous: &EquationExpression, current: &EquationExpression) -> bool {
    // CVS eqn_term.c::eqn_box suppresses space before a grouped EQN_LIST
    // following another list or an alphabetic/escape-led atom, except when
    // that list directly contains a pile or matrix.
    current.is_explicit_group()
        && current.children.first().is_some_and(|child| {
            child.kind != EquationKind::Pile && child.kind != EquationKind::Matrix
        })
        && (previous.kind == EquationKind::List
            || previous.text.as_deref().is_some_and(|text| {
                text.chars()
                    .next()
                    .is_some_and(|character| character == '\\' || character.is_alphabetic())
            }))
}

impl EquationExpression {
    fn projection_fences(&self) -> ProjectionFences<'_> {
        // CVS eqn_html.c::eqn_box keeps even invisible left/right as an
        // mfenced expression. Supply readable counterpart delimiters so an
        // empty or single-sided fence cannot erase that operand boundary.
        if self.left.is_none() && self.right.is_none() {
            return ProjectionFences {
                before_left: None,
                left: None,
                before_right: None,
                right: None,
                after_right: None,
            };
        }
        let left = self.left.as_deref().unwrap_or("");
        let right = self.right.as_deref().unwrap_or("");
        let (before_left, before_right, after_right) =
            if !left.is_empty() && matching_right(left) == Some(right) {
                (None, None, None)
            } else {
                match (matching_right(left), matching_left(right)) {
                    (Some(close), Some(open)) => (Some(open), Some(close), None),
                    (Some(close), None) => (None, Some(close), None),
                    (None, Some(open)) => (Some(open), None, None),
                    (None, None) => (Some("("), None, Some(")")),
                }
            };
        ProjectionFences {
            before_left,
            left: (!left.is_empty()).then_some(left),
            before_right,
            right: (!right.is_empty()).then_some(right),
            after_right,
        }
    }

    fn is_explicit_group(&self) -> bool {
        self.kind == EquationKind::List && self.expected_args != Some(1)
    }

    /// Whether a positional or decorated parent needs to group this operand.
    /// Explicit Lists and decorated boxes retain their scope; a transparent
    /// List inherits its child's scope.
    #[must_use]
    pub fn needs_operand_group(&self) -> bool {
        if self.summarized_operand_group {
            return true;
        }
        if self.left.is_some() || self.right.is_some() {
            return false;
        }
        if self.kind == EquationKind::Matrix || self.position == EquationPosition::Sqrt {
            return false;
        }
        if self.is_explicit_group() || self.top.is_some() || self.bottom.is_some() {
            return true;
        }
        if self.kind == EquationKind::Subexpression && self.position != EquationPosition::None {
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

// serde's skip_serializing_if callback takes a reference to the field.
#[expect(clippy::trivially_copy_pass_by_ref, reason = "serde requires &bool")]
fn is_false(value: &bool) -> bool {
    !*value
}
