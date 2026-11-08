//! Transfer the parsed native equation structure without replaying source.
use libmandoc_rs::{
    EquationAtomRole as NativeAtomRole, EquationBox, EquationFont, EquationKind, EquationPosition,
    Node,
};
use mant_ir::{
    Diagnostic, DiagnosticImpact, DiagnosticLevel, EquationAtomRole, EquationExpression,
    EquationFont as IrFont, EquationKind as IrKind, EquationOperandScope,
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
    let text = box_node.normalized_text().map(super::visible_text);
    let mut expression = EquationExpression {
        kind: match box_node.kind {
            // CVS eqn_box_new leaves the root with the default EQN_TEXT type.
            // A text-free content container is a sequence, not an atom.
            EquationKind::Text if box_node.text.is_none() => IrKind::Sequence,
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
        atom_role: text.as_deref().map_or(EquationAtomRole::Unknown, |text| {
            atom_role(box_node.atom_role, text)
        }),
        operand_scope: EquationOperandScope::Atomic,
        // CVS eqn.c::eqn_next substitutes aliases before eqn_parse's font
        // splitting. Only a complete unquoted token is eligible for the GNU
        // enhancement; source-neutral IR projects stored text verbatim.
        text,
        left: box_node.left.as_deref().map(super::visible_text),
        right: box_node.right.as_deref().map(super::visible_text),
        top: box_node.top.as_deref().map(super::visible_text),
        bottom: box_node.bottom.as_deref().map(super::visible_text),
        children: box_node
            .children
            .iter()
            .map(|child| expression_from_box_in_row(child, box_node.kind == EquationKind::Pile))
            .collect(),
    };
    if expression.validate_shallow().is_err() {
        // CVS eqn.c permits incomplete operators such as `a over` and `sqrt`.
        // Retain the whole owned readable payload rather than dropping extra
        // operands or publishing a structurally invalid mathematical node.
        let scope = if expression.needs_operand_group() {
            EquationOperandScope::Compound
        } else {
            EquationOperandScope::Atomic
        };
        expression = EquationExpression {
            kind: IrKind::Opaque,
            font: IrFont::None,
            operator: IrOperator::None,
            atom_role: EquationAtomRole::Unknown,
            operand_scope: scope,
            text: Some(super::visible_text(&box_node.readable_text())),
            left: None,
            right: None,
            top: None,
            bottom: None,
            children: Vec::new(),
        };
    }
    expression
}

fn atom_role(native: NativeAtomRole, text: &str) -> EquationAtomRole {
    match native {
        NativeAtomRole::Function => return EquationAtomRole::Function,
        NativeAtomRole::Literal => return EquationAtomRole::Literal,
        NativeAtomRole::Unknown => {}
    }
    let mut characters = text.chars();
    let Some(first) = characters.next() else {
        return EquationAtomRole::Unknown;
    };
    if (first.is_alphabetic() || first == '_')
        && characters.all(|character| character.is_alphanumeric() || character == '_')
    {
        return EquationAtomRole::Identifier;
    }
    let mut decimal_points = 0;
    if text.bytes().any(|byte| byte.is_ascii_digit())
        && text.bytes().all(|byte| {
            if byte == b'.' {
                decimal_points += 1;
                decimal_points <= 1
            } else {
                byte.is_ascii_digit()
            }
        })
    {
        return EquationAtomRole::Number;
    }
    if text.chars().all(|character| {
        matches!(
            character,
            '+' | '-'
                | '−'
                | '='
                | '<'
                | '>'
                | '≤'
                | '≥'
                | '≠'
                | '≡'
                | '≈'
                | '±'
                | '∓'
                | '×'
                | '÷'
                | '*'
                | '/'
                | '·'
                | '⋅'
                | '∑'
                | '∏'
                | '∫'
                | '∩'
                | '∪'
                | '∂'
                | '∇'
                | '→'
                | '←'
                | '↔'
                | '≪'
                | '≫'
                | '!'
                | ','
                | ';'
                | ':'
                | '.'
                | '|'
                | '‖'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '^'
                | '~'
        )
    }) {
        EquationAtomRole::Operator
    } else {
        EquationAtomRole::Unknown
    }
}

/// Inspect each original formula once, independently of speculative lowering.
/// The native owned tree already bounds traversal; diagnostic details have a
/// separate page cap. The summary still marks semantic coverage incomplete.
pub(super) fn diagnostics(root: &Node) -> Vec<Diagnostic> {
    const MAX_DETAILS: usize = 256;
    let mut output = Vec::new();
    let mut summarized = false;
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        // Native man/mdoc renderers and the shared inline lowering both
        // suppress NODE_NOPRT subtrees; their unused formula evidence does
        // not make published document semantics incomplete.
        if node.flags.no_print || node.kind == libmandoc_rs::NodeKind::Comment {
            continue;
        }
        pending.extend(node.children.iter().rev());
        let Some(expression) = node.equation.as_ref() else {
            continue;
        };
        let mut boxes = vec![(expression, true)];
        let mut size = false;
        let mut malformed = false;
        while let Some((current, root)) = boxes.pop() {
            // eqn_box_new's outer root is calloc'ed (size=0), whereas child
            // sizes come from gsize or a size wrapper. Real zero selections
            // in those children remain unsupported evidence, not defaults.
            size |= current.size != i32::MIN && (!root || current.size != 0);
            malformed |= invalid_operation(current);
            boxes.extend(current.children.iter().map(|child| (child, false)));
        }
        for (present, code, message) in [
            (
                size,
                "manual.equation-size-unsupported",
                "native equation size evidence is not executed by the selected formatter; no resolved font size was invented",
            ),
            (
                malformed,
                "manual.equation-structure-opaque",
                "an incomplete native equation operation was retained as opaque readable content",
            ),
        ] {
            if !present {
                continue;
            }
            if output.len() >= MAX_DETAILS {
                summarized = true;
                continue;
            }
            output.push(Diagnostic {
                impact: DiagnosticImpact::SemanticCoverage,
                level: DiagnosticLevel::Unsupported,
                code: Some(code.to_owned()),
                message: message.to_owned(),
                source: super::source_span(node),
            });
        }
    }
    if summarized {
        output.push(Diagnostic {
            impact: DiagnosticImpact::SemanticCoverage,
            level: DiagnosticLevel::Unsupported,
            code: Some("manual.equation-diagnostics-summarized".to_owned()),
            message: "additional equation coverage findings exceeded the bounded detail allowance"
                .to_owned(),
            source: None,
        });
    }
    output
}

fn invalid_operation(expression: &EquationBox) -> bool {
    if expression.kind != EquationKind::Subexpression {
        return false;
    }
    let expected = match expression.position {
        EquationPosition::None => return true,
        EquationPosition::Sqrt => 1,
        EquationPosition::SubscriptSuperscript | EquationPosition::FromTo => 3,
        _ => 2,
    };
    expression.children.len() != expected
}
