//! Direct `MathML` Core projection; no source macros or legacy mfenced markup.

use mant_ir::{
    EquationAtomRole, EquationDecorationKind, EquationDecorationRef, EquationExpression,
    EquationFont, EquationKind, EquationOperator,
};

use super::{
    Encoder, MathError, MathLossKind, effective_font, irregular_matrix, matrix_loss,
    safe_character, unsupported_whitespace,
};

pub(super) fn encode(
    encoder: &mut Encoder,
    expression: &EquationExpression,
) -> Result<(), MathError> {
    encoder.push(if encoder.options.display {
        "<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"block\">"
    } else {
        "<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"inline\">"
    })?;
    expression_xml(encoder, expression, EquationFont::None)?;
    encoder.push("</math>")
}

fn expression_xml(
    encoder: &mut Encoder,
    expression: &EquationExpression,
    inherited: EquationFont,
) -> Result<(), MathError> {
    let font = effective_font(expression, inherited);
    if let Some(loss) = matrix_loss(expression) {
        encoder.loss(loss)?;
        return token(encoder, "mtext", &expression.readable_text(), font, false);
    }
    let decoration = match (&expression.bottom, &expression.top) {
        (Some(_), Some(_)) => Some((
            "<munderover accentunder=\"true\" accent=\"true\">",
            "</munderover>",
        )),
        (Some(_), None) => Some(("<munder accentunder=\"true\">", "</munder>")),
        (None, Some(_)) => Some(("<mover accent=\"true\">", "</mover>")),
        (None, None) => None,
    };
    if let Some((opening, _)) = decoration {
        encoder.push(opening)?;
    }
    let fenced = expression.left.is_some() || expression.right.is_some();
    if fenced {
        encoder.push("<mrow>")?;
        if let Some(left) = &expression.left {
            fence(encoder, left)?;
        }
    }
    core(encoder, expression, font)?;
    if fenced {
        if let Some(right) = &expression.right {
            fence(encoder, right)?;
        }
        encoder.push("</mrow>")?;
    }
    if let Some(bottom) = expression.bottom_decoration() {
        accent(encoder, bottom)?;
    }
    if let Some(top) = expression.top_decoration() {
        accent(encoder, top)?;
    }
    if let Some((_, closing)) = decoration {
        encoder.push(closing)?;
    }
    Ok(())
}

fn core(
    encoder: &mut Encoder,
    expression: &EquationExpression,
    font: EquationFont,
) -> Result<(), MathError> {
    match expression.kind {
        EquationKind::Text | EquationKind::Opaque => atom(encoder, expression, font),
        EquationKind::Operation => {
            let (opening, closing) = match expression.operator {
                EquationOperator::Fraction => ("<mfrac>", "</mfrac>"),
                EquationOperator::SquareRoot => ("<msqrt>", "</msqrt>"),
                EquationOperator::Superscript => ("<msup>", "</msup>"),
                EquationOperator::Subscript => ("<msub>", "</msub>"),
                EquationOperator::SubscriptSuperscript => ("<msubsup>", "</msubsup>"),
                EquationOperator::UpperLimit => ("<mover accent=\"false\">", "</mover>"),
                EquationOperator::LowerLimit => ("<munder accentunder=\"false\">", "</munder>"),
                EquationOperator::LowerUpperLimits => (
                    "<munderover accentunder=\"false\" accent=\"false\">",
                    "</munderover>",
                ),
                EquationOperator::None => ("<mrow>", "</mrow>"),
            };
            encoder.push(opening)?;
            children(encoder, expression, font)?;
            encoder.push(closing)
        }
        EquationKind::Sequence | EquationKind::Group => {
            encoder.push("<mrow>")?;
            children(encoder, expression, font)?;
            encoder.push("</mrow>")
        }
        EquationKind::Pile => {
            encoder.push("<mtable>")?;
            for (index, child) in expression.children.iter().enumerate() {
                encoder.push("<mtr><mtd>")?;
                encoder.child(index, |encoder| expression_xml(encoder, child, font))?;
                encoder.push("</mtd></mtr>")?;
            }
            encoder.push("</mtable>")
        }
        EquationKind::Matrix => matrix(encoder, expression, font),
    }
}

fn children(
    encoder: &mut Encoder,
    expression: &EquationExpression,
    font: EquationFont,
) -> Result<(), MathError> {
    for (index, child) in expression.children.iter().enumerate() {
        encoder.child(index, |encoder| expression_xml(encoder, child, font))?;
    }
    Ok(())
}

fn atom(
    encoder: &mut Encoder,
    expression: &EquationExpression,
    font: EquationFont,
) -> Result<(), MathError> {
    let text = expression.text.as_deref().unwrap_or_default();
    let tag = if expression.kind == EquationKind::Opaque {
        encoder.loss(MathLossKind::OpaqueStructure)?;
        "mtext"
    } else {
        match expression.atom_role {
            EquationAtomRole::Number => "mn",
            EquationAtomRole::Operator => "mo",
            EquationAtomRole::Unknown => {
                encoder.loss(MathLossKind::UnknownAtomRole)?;
                "mi"
            }
            // A quoted literal is one mathematical atom, not a prose request.
            EquationAtomRole::Identifier
            | EquationAtomRole::Function
            | EquationAtomRole::Literal => "mi",
        }
    };
    let normal_identifier = tag == "mi"
        && (font != EquationFont::None || expression.atom_role == EquationAtomRole::Function);
    let font = if expression.atom_role == EquationAtomRole::Function && font == EquationFont::None {
        EquationFont::Roman
    } else {
        font
    };
    token(encoder, tag, text, font, normal_identifier)
}

fn token(
    encoder: &mut Encoder,
    tag: &str,
    text: &str,
    font: EquationFont,
    normal_identifier: bool,
) -> Result<(), MathError> {
    if text
        .chars()
        .any(|character| matches!(character, '\t' | '\n' | '\r'))
    {
        encoder.loss(MathLossKind::LiteralWhitespace)?;
    }
    encoder.push("<")?;
    encoder.push(tag)?;
    if normal_identifier {
        encoder.push(" mathvariant=\"normal\"")?;
    }
    match font {
        EquationFont::Italic => encoder.push(" style=\"font-style:italic;font-weight:normal\"")?,
        EquationFont::Bold | EquationFont::Fat => {
            encoder.push(" style=\"font-style:normal;font-weight:bold\"")?;
        }
        EquationFont::Roman => encoder.push(" style=\"font-style:normal;font-weight:normal\"")?,
        EquationFont::None => {}
    }
    encoder.push(">")?;
    text_xml(encoder, text)?;
    encoder.push("</")?;
    encoder.push(tag)?;
    encoder.push(">")
}

fn fence(encoder: &mut Encoder, value: &str) -> Result<(), MathError> {
    if value.is_empty() {
        return Ok(());
    }
    if unsupported_whitespace(value) {
        encoder.loss(MathLossKind::LiteralWhitespace)?;
    }
    encoder.push("<mo fence=\"true\" stretchy=\"true\">")?;
    text_xml(encoder, value)?;
    encoder.push("</mo>")
}

fn accent(encoder: &mut Encoder, decoration: EquationDecorationRef<'_>) -> Result<(), MathError> {
    if unsupported_whitespace(decoration.symbol) {
        encoder.loss(MathLossKind::LiteralWhitespace)?;
    }
    encoder.push(
        if matches!(
            decoration.kind,
            EquationDecorationKind::Dot
                | EquationDecorationKind::DoubleDot
                | EquationDecorationKind::Custom
        ) {
            "<mo stretchy=\"false\">"
        } else {
            "<mo stretchy=\"true\">"
        },
    )?;
    text_xml(encoder, decoration.symbol)?;
    encoder.push("</mo>")
}

fn matrix(
    encoder: &mut Encoder,
    expression: &EquationExpression,
    font: EquationFont,
) -> Result<(), MathError> {
    let matrix = expression
        .matrix_view()
        .expect("expression entry checked matrix axes");
    if irregular_matrix(matrix.columns) {
        encoder.loss(MathLossKind::IrregularMatrix)?;
    }
    let font = effective_font(matrix.columns_group, font);
    let rows = matrix
        .columns
        .iter()
        .map(|column| column.children.len())
        .max()
        .unwrap_or(0);
    encoder.push("<mtable>")?;
    for row in 0..rows {
        encoder.push("<mtr>")?;
        for (column_index, column) in matrix.columns.iter().enumerate() {
            encoder.push("<mtd>")?;
            if let Some(cell) = column.children.get(row) {
                encoder.child(0, |encoder| {
                    encoder.child(column_index, |encoder| {
                        encoder.child(row, |encoder| {
                            expression_xml(encoder, cell, effective_font(column, font))
                        })
                    })
                })?;
            }
            encoder.push("</mtd>")?;
        }
        encoder.push("</mtr>")?;
    }
    encoder.push("</mtable>")
}

fn text_xml(encoder: &mut Encoder, text: &str) -> Result<(), MathError> {
    for character in text.chars() {
        match character {
            // MathML token normalization trims and collapses ASCII spaces.
            // A nonbreaking space keeps each authored blank inside its atom.
            ' ' => encoder.push("&#xA0;")?,
            '\t' => encoder.push("&#x9;")?,
            '\n' => encoder.push("&#xA;")?,
            '\r' => encoder.push("&#xD;")?,
            '&' => encoder.push("&amp;")?,
            '<' => encoder.push("&lt;")?,
            '>' => encoder.push("&gt;")?,
            '"' => encoder.push("&quot;")?,
            '\'' => encoder.push("&apos;")?,
            character if !safe_character(character) => {
                encoder.loss(MathLossKind::UnsupportedCharacter)?;
                encoder.push(&format!("[U+{:04X}]", u32::from(character)))?;
            }
            character => encoder.push(character.encode_utf8(&mut [0; 4]))?,
        }
    }
    Ok(())
}
