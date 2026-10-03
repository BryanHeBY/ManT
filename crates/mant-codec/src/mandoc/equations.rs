//! Transfer the parsed native equation structure without replaying source.
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
        text: box_node.normalized_text().map(super::visible_text),
        left: box_node.left.as_deref().map(super::visible_text),
        right: box_node.right.as_deref().map(super::visible_text),
        top: box_node.top.as_deref().map(super::visible_text),
        bottom: box_node.bottom.as_deref().map(super::visible_text),
        children: box_node.children.iter().map(expression_from_box).collect(),
    }
}
