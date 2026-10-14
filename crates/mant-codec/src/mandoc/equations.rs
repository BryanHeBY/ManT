//! Transfer the parsed native equation structure without replaying source.
use libmandoc_rs::{EquationBox, EquationFont, EquationKind, EquationPosition};
use mant_ir::{
    EquationExpression, EquationFont as IrFont, EquationKind as IrKind, EquationOperandScope,
    EquationOperator as IrOperator,
};

/// Transfer the bounded owned eqn tree into source-neutral IR. Fields remain
/// independently addressable here; the complete document position determines
/// the wire-safe budget after structural lowering.
pub(super) fn expression_from_ast(box_node: &EquationBox) -> EquationExpression {
    expression_from_box(box_node)
}

fn expression_from_box(box_node: &EquationBox) -> EquationExpression {
    expression_from_box_in_row(box_node, false)
}

fn expression_from_box_in_row(box_node: &EquationBox, pile_row: bool) -> EquationExpression {
    EquationExpression {
        kind: match box_node.kind {
            // CVS eqn_box_new leaves the root with the default EQN_TEXT type.
            // A text-free content container is a sequence, not an atom.
            EquationKind::Text if box_node.text.is_none() && !box_node.children.is_empty() => {
                IrKind::Sequence
            }
            EquationKind::Text => IrKind::Text,
            EquationKind::Subexpression => IrKind::Operation,
            // CVS eqn.c uses expectargs=1 for transparent font/size wrappers.
            // eqn_term.c also skips a Pile's singleton row wrapper. Resolve
            // both distinctions here; consumers must not interpret counters.
            EquationKind::List
                if box_node.expected_args == 1 || pile_row && box_node.actual_args <= 1 =>
            {
                IrKind::Sequence
            }
            EquationKind::List => IrKind::Group,
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
        operator: match box_node.position {
            EquationPosition::None => IrOperator::None,
            EquationPosition::Superscript => IrOperator::Superscript,
            EquationPosition::SubscriptSuperscript => IrOperator::SubscriptSuperscript,
            EquationPosition::Subscript => IrOperator::Subscript,
            EquationPosition::To => IrOperator::UpperLimit,
            EquationPosition::From => IrOperator::LowerLimit,
            EquationPosition::FromTo => IrOperator::LowerUpperLimits,
            EquationPosition::Over => IrOperator::Fraction,
            EquationPosition::Sqrt => IrOperator::SquareRoot,
        },
        size: (box_node.size != i32::MIN).then_some(box_node.size),
        operand_scope: EquationOperandScope::Atomic,
        // CVS eqn.c::eqn_next substitutes aliases before eqn_parse's font
        // splitting. Only a complete unquoted token is eligible for the GNU
        // enhancement; source-neutral IR projects stored text verbatim.
        text: box_node.normalized_text().map(super::visible_text),
        left: box_node.left.as_deref().map(super::visible_text),
        right: box_node.right.as_deref().map(super::visible_text),
        top: box_node.top.as_deref().map(super::visible_text),
        bottom: box_node.bottom.as_deref().map(super::visible_text),
        children: box_node
            .children
            .iter()
            .map(|child| expression_from_box_in_row(child, box_node.kind == EquationKind::Pile))
            .collect(),
    }
}
