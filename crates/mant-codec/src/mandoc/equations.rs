//! Bounded native equation normalization and source delimiter state.
use libmandoc_rs::{EquationBox, EquationFont, EquationKind, EquationPosition};
use mant_ir::{
    EquationExpression, EquationFont as IrFont, EquationKind as IrKind,
    EquationPosition as IrPosition,
};

/// Transfer the bounded owned eqn tree into source-neutral IR. Fields remain
/// independently addressable here; the complete document position determines
/// the wire-safe budget after structural lowering.
pub(super) fn expression_from_ast(box_node: &EquationBox) -> EquationExpression {
    expression_from_box(box_node)
}

fn expression_from_box(box_node: &EquationBox) -> EquationExpression {
    EquationExpression {
        kind: match box_node.kind {
            EquationKind::Text => IrKind::Text,
            EquationKind::Subexpression => IrKind::Subexpression,
            EquationKind::List => IrKind::List,
            EquationKind::Pile => IrKind::Pile,
            EquationKind::Matrix => IrKind::Matrix,
        },
        font: match box_node.font {
            EquationFont::None => IrFont::None,
            EquationFont::Roman => IrFont::Roman,
            EquationFont::Bold => IrFont::Bold,
            EquationFont::Fat => IrFont::Fat,
            EquationFont::Italic => IrFont::Italic,
        },
        position: match box_node.position {
            EquationPosition::None => IrPosition::None,
            EquationPosition::Superscript => IrPosition::Superscript,
            EquationPosition::SubscriptSuperscript => IrPosition::SubscriptSuperscript,
            EquationPosition::Subscript => IrPosition::Subscript,
            EquationPosition::To => IrPosition::To,
            EquationPosition::From => IrPosition::From,
            EquationPosition::FromTo => IrPosition::FromTo,
            EquationPosition::Over => IrPosition::Over,
            EquationPosition::Sqrt => IrPosition::Sqrt,
        },
        size: (box_node.size != i32::MIN).then_some(box_node.size),
        // CVS eqn.c::eqn_box_new initializes expectargs to UINT_MAX for a
        // list with no fixed grammar maximum. Do not leak that sentinel into
        // source-neutral IR or the unpublished JSON contract.
        expected_args: (box_node.expected_args != u32::MAX as usize)
            .then_some(box_node.expected_args),
        actual_args: box_node.actual_args,
        summarized_operand_group: false,
        // CVS eqn.c::eqn_next substitutes aliases before eqn_parse's font
        // splitting. Only a complete unquoted token is eligible for the GNU
        // enhancement; source-neutral IR projects stored text verbatim.
        text: box_node.text.as_deref().map(|text| {
            if box_node.gnu_ldots {
                "...".to_owned()
            } else {
                super::visible_text(text)
            }
        }),
        left: box_node.left.as_deref().map(super::visible_text),
        right: box_node.right.as_deref().map(super::visible_text),
        top: box_node.top.as_deref().map(super::visible_text),
        bottom: box_node.bottom.as_deref().map(super::visible_text),
        children: box_node.children.iter().map(expression_from_box).collect(),
    }
}

use super::{
    LoweringContext, MAX_INLINE_EQUATION_NORMALIZATIONS, Node, Parser, Path, visible_text,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct EquationDelimiterChange {
    line: u32,
    delimiters: Option<(char, char)>,
}

#[derive(Clone, Copy, Debug)]
enum EquationDelimiterDirective {
    Enable(char, char),
    Disable,
}

impl EquationDelimiterDirective {
    const fn delimiters(self) -> Option<(char, char)> {
        match self {
            Self::Enable(opening, closing) => Some((opening, closing)),
            Self::Disable => None,
        }
    }
}

fn first_equation(node: &Node) -> Option<String> {
    node.equation
        .as_ref()
        .map(libmandoc_rs::EquationBox::readable_text)
        .or_else(|| node.children.iter().find_map(first_equation))
}

/// Track active inline eqn delimiters at each source line.
///
/// eqn configures delimiters inside an `.EQ`/`.EN` block; they take effect on
/// following prose and tbl cells. Keeping the change points makes lookup
/// logarithm-free and deterministic without replaying the whole source for
/// every table cell.
pub(super) fn equation_delimiter_changes(source: &str) -> Vec<EquationDelimiterChange> {
    let mut changes = Vec::new();
    let mut in_equation = false;
    let mut pending = None;
    for (index, source_line) in source.lines().enumerate() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let trimmed = source_line.trim();
        if trimmed.starts_with(".\\\"") || trimmed.starts_with("'\\\"") {
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix(".EQ")
            .or_else(|| trimmed.strip_prefix("'EQ"))
            .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        {
            in_equation = true;
            pending = parse_equation_delimiters(rest.trim()).or(pending);
            continue;
        }
        if in_equation {
            if trimmed == ".EN" || trimmed == "'EN" {
                if let Some(delimiters) = pending.take() {
                    changes.push(EquationDelimiterChange {
                        line: line.saturating_add(1),
                        delimiters: delimiters.delimiters(),
                    });
                }
                in_equation = false;
            } else if let Some(delimiters) = parse_equation_delimiters(trimmed) {
                pending = Some(delimiters);
            }
        }
    }
    changes
}

fn parse_equation_delimiters(value: &str) -> Option<EquationDelimiterDirective> {
    let value = value.strip_prefix("delim")?.trim_start();
    if value == "off" {
        return Some(EquationDelimiterDirective::Disable);
    }
    let mut delimiters = value.chars();
    let opening = delimiters.next()?;
    let closing = delimiters.next()?;
    Some(EquationDelimiterDirective::Enable(opening, closing))
}

impl LoweringContext<'_> {
    pub(super) fn equation_delimiters_at(&self, line: u32) -> Option<(char, char)> {
        self.equation_delimiters
            .iter()
            .rev()
            .find(|change| change.line <= line)
            .and_then(|change| change.delimiters)
    }
    /// Normalize an eqn fragment through the same pinned parser used for
    /// display equations. tbl retains delimiter-wrapped cell text as an
    /// opaque string, so reparsing only that bounded fragment is the sole way
    /// to avoid a second, incomplete eqn grammar in the lowering layer.
    pub(super) fn normalize_equation(&self, source: &str, line: u32) -> String {
        {
            let normalized = self.normalized_equations.borrow();
            if let Some(value) = normalized.get(source) {
                return value.clone();
            }
            if normalized.len() >= MAX_INLINE_EQUATION_NORMALIZATIONS {
                drop(normalized);
                self.warn_inline_equation_budget(line);
                return visible_text(source);
            }
        }
        // Every attempted local parse, including a failed one, consumes the
        // same per-document byte/work allowance. The synthetic input contains
        // only this fragment, so parser work is bounded by charged bytes.
        if !self
            .equation_normalization_budget
            .borrow_mut()
            .charge(source.len())
        {
            self.warn_inline_equation_budget(line);
            return visible_text(source);
        }
        let synthetic = format!(".TH MANT-EQN 7\n.EQ\n{source}\n.EN\n");
        let normalized = Parser::default()
            .parse_bytes(Path::new("mant-inline-eqn.7"), synthetic.as_bytes())
            .ok()
            .and_then(|report| {
                first_equation(&report.document.root).map(|text| visible_text(&text))
            })
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| visible_text(source));
        self.normalized_equations
            .borrow_mut()
            .insert(source.to_owned(), normalized.clone());
        normalized
    }
}
