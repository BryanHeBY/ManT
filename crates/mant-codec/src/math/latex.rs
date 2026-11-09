//! Restricted mathematical/amsmath bodies assembled only from fixed commands.

use mant_ir::{
    EquationAtomRole, EquationExpression, EquationFont, EquationKind, EquationOperationRef,
};

use super::{
    Encoder, MathError, MathLossKind, effective_font, irregular_matrix, matrix_loss,
    safe_character,
    symbols::{latex_accent, latex_fence, latex_symbol},
    unsupported_whitespace,
};

pub(super) fn encode(
    encoder: &mut Encoder,
    expression: &EquationExpression,
) -> Result<(), MathError> {
    if encoder.options.display {
        encoder.push("\\displaystyle ")?;
    }
    expression_tex(encoder, expression, EquationFont::None)
}

fn expression_tex(
    encoder: &mut Encoder,
    expression: &EquationExpression,
    inherited: EquationFont,
) -> Result<(), MathError> {
    let font = effective_font(expression, inherited);
    if let Some(loss) = matrix_loss(expression) {
        encoder.loss(loss)?;
        return readable(encoder, expression, font);
    }
    let left = expression.left.as_deref().map(latex_fence);
    let right = expression.right.as_deref().map(latex_fence);
    let literal_fences = left == Some(None) || right == Some(None);
    if literal_fences {
        encoder.loss(MathLossKind::UnsupportedFence)?;
    }
    let top = expression
        .top_decoration()
        .map(|decoration| latex_accent(decoration.kind));
    let bottom = expression
        .bottom_decoration()
        .map(|decoration| latex_accent(decoration.kind));
    if top == Some(None) || bottom == Some(None) {
        encoder.loss(MathLossKind::UnsupportedDecoration)?;
        return readable(encoder, expression, font);
    }
    for opening in [bottom.flatten(), top.flatten()].into_iter().flatten() {
        encoder.push(opening)?;
    }
    let fenced = left.is_some() || right.is_some();
    if literal_fences {
        encoder.push("{")?;
        if let Some(left) = expression.left.as_deref() {
            literal_fence(encoder, left, true)?;
        }
    } else if fenced {
        encoder.push("\\left")?;
        encoder.push(left.flatten().unwrap_or("."))?;
        encoder.push(" ")?;
    }
    core(encoder, expression, font)?;
    if literal_fences {
        if let Some(right) = expression.right.as_deref() {
            literal_fence(encoder, right, false)?;
        }
        encoder.push("}")?;
    } else if fenced {
        encoder.push("\\right")?;
        encoder.push(right.flatten().unwrap_or("."))?;
        encoder.push(" ")?;
    }
    for _ in [top, bottom].into_iter().flatten() {
        encoder.push("}")?;
    }
    Ok(())
}

fn core(
    encoder: &mut Encoder,
    expression: &EquationExpression,
    font: EquationFont,
) -> Result<(), MathError> {
    match expression.kind {
        EquationKind::Text => atom(encoder, expression, font),
        EquationKind::Opaque => {
            encoder.loss(MathLossKind::OpaqueStructure)?;
            literal(
                encoder,
                expression.text.as_deref().unwrap_or_default(),
                font,
            )
        }
        EquationKind::Sequence | EquationKind::Group => {
            encoder.push("{")?;
            for (index, child) in expression.children.iter().enumerate() {
                if index > 0 {
                    encoder.push(" ")?;
                }
                child_tex(encoder, index, child, font)?;
            }
            encoder.push("}")
        }
        EquationKind::Operation => operation(encoder, expression, font),
        EquationKind::Pile => {
            encoder.push("\\begin{matrix}")?;
            for (index, child) in expression.children.iter().enumerate() {
                if index > 0 {
                    encoder.push(" \\\\ ")?;
                }
                child_tex(encoder, index, child, font)?;
            }
            encoder.push("\\end{matrix}")
        }
        EquationKind::Matrix => matrix(encoder, expression, font),
    }
}

fn child_tex(
    encoder: &mut Encoder,
    index: usize,
    child: &EquationExpression,
    font: EquationFont,
) -> Result<(), MathError> {
    encoder.child(index, |encoder| expression_tex(encoder, child, font))
}

fn argument(
    encoder: &mut Encoder,
    index: usize,
    child: &EquationExpression,
    font: EquationFont,
) -> Result<(), MathError> {
    encoder.push("{")?;
    child_tex(encoder, index, child, font)?;
    encoder.push("}")
}

fn operation(
    encoder: &mut Encoder,
    expression: &EquationExpression,
    font: EquationFont,
) -> Result<(), MathError> {
    match expression
        .operation_view()
        .expect("preflight checked operation")
    {
        EquationOperationRef::Fraction {
            numerator,
            denominator,
        } => {
            encoder.push("\\frac")?;
            argument(encoder, 0, numerator, font)?;
            argument(encoder, 1, denominator, font)
        }
        EquationOperationRef::SquareRoot { radicand } => {
            encoder.push("\\sqrt")?;
            argument(encoder, 0, radicand, font)
        }
        EquationOperationRef::Superscript { base, upper } => {
            argument(encoder, 0, base, font)?;
            encoder.push("^")?;
            argument(encoder, 1, upper, font)
        }
        EquationOperationRef::Subscript { base, lower } => {
            argument(encoder, 0, base, font)?;
            encoder.push("_")?;
            argument(encoder, 1, lower, font)
        }
        EquationOperationRef::SubscriptSuperscript { base, lower, upper } => {
            argument(encoder, 0, base, font)?;
            encoder.push("_")?;
            argument(encoder, 1, lower, font)?;
            encoder.push("^")?;
            argument(encoder, 2, upper, font)
        }
        EquationOperationRef::UpperLimit { base, upper } => {
            encoder.push("\\overset")?;
            argument(encoder, 1, upper, font)?;
            argument(encoder, 0, base, font)
        }
        EquationOperationRef::LowerLimit { base, lower } => {
            encoder.push("\\underset")?;
            argument(encoder, 1, lower, font)?;
            argument(encoder, 0, base, font)
        }
        EquationOperationRef::LowerUpperLimits { base, lower, upper } => {
            encoder.push("\\underset")?;
            argument(encoder, 1, lower, font)?;
            encoder.push("{\\overset")?;
            argument(encoder, 2, upper, font)?;
            argument(encoder, 0, base, font)?;
            encoder.push("}")
        }
    }
}

fn atom(
    encoder: &mut Encoder,
    expression: &EquationExpression,
    font: EquationFont,
) -> Result<(), MathError> {
    let text = expression.text.as_deref().unwrap_or_default();
    match expression.atom_role {
        EquationAtomRole::Unknown => {
            encoder.loss(MathLossKind::UnknownAtomRole)?;
            literal(encoder, text, font)
        }
        EquationAtomRole::Literal => literal(encoder, text, font),
        EquationAtomRole::Function => {
            encoder.push("\\mathop{")?;
            literal(
                encoder,
                text,
                if font == EquationFont::None {
                    EquationFont::Roman
                } else {
                    font
                },
            )?;
            encoder.push("}\\nolimits ")
        }
        EquationAtomRole::Identifier | EquationAtomRole::Number | EquationAtomRole::Operator => {
            symbolic_atom(encoder, text, expression.atom_role, font)
        }
    }
}

fn symbolic_atom(
    encoder: &mut Encoder,
    text: &str,
    role: EquationAtomRole,
    font: EquationFont,
) -> Result<(), MathError> {
    if !simple_spelling(text, role) {
        // A custom identifier such as a+b, a numeric spelling such as 1e-5,
        // and an operator label remain one atom, not new TeX operations.
        if role == EquationAtomRole::Operator {
            encoder.loss(MathLossKind::UnsupportedAtom)?;
        }
        return literal(encoder, text, font);
    }
    if matches!(font, EquationFont::Bold | EquationFont::Fat) && !text.is_ascii()
        || role == EquationAtomRole::Operator
            && matches!(
                font,
                EquationFont::Bold | EquationFont::Fat | EquationFont::Italic
            )
        || font == EquationFont::Roman && text.chars().any(greek_lowercase)
    {
        encoder.loss(MathLossKind::UnsupportedFont)?;
    }
    let unwrapped_operator = role == EquationAtomRole::Operator
        && matches!(font, EquationFont::None | EquationFont::Roman);
    if !unwrapped_operator {
        encoder.push(match font {
            EquationFont::None => "{",
            EquationFont::Roman => "\\mathrm{",
            EquationFont::Italic => "\\mathit{",
            EquationFont::Bold | EquationFont::Fat => "\\mathbf{",
        })?;
    }
    if role == EquationAtomRole::Operator && text == "..." {
        encoder.push("\\ldots{}")?;
    } else {
        for character in text.chars() {
            math_character(encoder, character)?;
        }
    }
    if unwrapped_operator {
        Ok(())
    } else {
        encoder.push("}")
    }
}

fn simple_spelling(text: &str, role: EquationAtomRole) -> bool {
    match role {
        EquationAtomRole::Identifier => text
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_'),
        EquationAtomRole::Number => text
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.'),
        EquationAtomRole::Operator => {
            if text == "..." {
                return true;
            }
            let mut characters = text.chars();
            characters.next().is_some_and(|character| {
                !character.is_alphanumeric()
                    && (latex_symbol(character).is_some() || ascii_operator(character))
            }) && characters.next().is_none()
        }
        _ => false,
    }
}

fn ascii_operator(character: char) -> bool {
    matches!(
        character,
        '+' | '-'
            | '='
            | '*'
            | '/'
            | '('
            | ')'
            | '['
            | ']'
            | '<'
            | '>'
            | '|'
            | ','
            | '.'
            | ':'
            | ';'
            | '!'
            | '?'
    )
}

fn math_character(encoder: &mut Encoder, character: char) -> Result<(), MathError> {
    if let Some(symbol) = latex_symbol(character) {
        encoder.push(symbol)
    } else if character == '<' {
        encoder.push("\\mathrel{\\text{\\textless{}}}")
    } else if character == '>' {
        encoder.push("\\mathrel{\\text{\\textgreater{}}}")
    } else if character.is_ascii_alphanumeric() || ascii_operator(character) {
        encoder.push(character.encode_utf8(&mut [0; 4]))
    } else {
        if !character.is_ascii() {
            encoder.loss(MathLossKind::UnsupportedSymbol)?;
        }
        encoder.push("\\text{")?;
        literal_text(encoder, character.encode_utf8(&mut [0; 4]))?;
        encoder.push("}")
    }
}

fn greek_lowercase(character: char) -> bool {
    // Classic TeX's lower-case Greek symbols belong to the fixed letters
    // symbol font, not the selectable math alphabet: \mathrm{\alpha} does
    // not establish an upright alpha. Do not silently promise that style.
    ('\u{0370}'..='\u{03ff}').contains(&character) && character.is_lowercase()
}

fn literal(encoder: &mut Encoder, value: &str, font: EquationFont) -> Result<(), MathError> {
    if unsupported_whitespace(value) {
        encoder.loss(MathLossKind::LiteralWhitespace)?;
    }
    encoder.push(match font {
        EquationFont::None | EquationFont::Roman => "\\mathord{\\textrm{",
        EquationFont::Italic => "\\mathord{\\textit{",
        EquationFont::Bold | EquationFont::Fat => "\\mathord{\\textbf{",
    })?;
    literal_text(encoder, value)?;
    encoder.push("}}")
}

fn literal_text(encoder: &mut Encoder, value: &str) -> Result<(), MathError> {
    for character in value.chars() {
        let escaped = match character {
            '\\' => Some("\\textbackslash{}"),
            '$' => Some("\\textdollar{}"),
            '^' => Some("\\textasciicircum{}"),
            '~' => Some("\\textasciitilde{}"),
            '<' => Some("\\textless{}"),
            '>' => Some("\\textgreater{}"),
            '{' => Some("\\{"),
            '}' => Some("\\}"),
            '_' => Some("\\_"),
            '&' => Some("\\&"),
            '%' => Some("\\%"),
            '#' => Some("\\#"),
            ' ' => Some("\\ "),
            '\n' | '\r' | '\t' => {
                encoder.loss(MathLossKind::LiteralWhitespace)?;
                Some("\\ ")
            }
            character if !safe_character(character) => {
                encoder.loss(MathLossKind::UnsupportedCharacter)?;
                encoder.push(&format!("[U+{:04X}]", u32::from(character)))?;
                continue;
            }
            _ => None,
        };
        if let Some(escaped) = escaped {
            encoder.push(escaped)?;
        } else {
            if !character.is_ascii() && latex_symbol(character).is_none() {
                encoder.loss(MathLossKind::UnsupportedSymbol)?;
            }
            encoder.push(character.encode_utf8(&mut [0; 4]))?;
        }
    }
    Ok(())
}

fn readable(
    encoder: &mut Encoder,
    expression: &EquationExpression,
    font: EquationFont,
) -> Result<(), MathError> {
    // Preflight already bounded structure and payload before this projection.
    literal(encoder, &expression.readable_text(), font)
}

fn literal_fence(encoder: &mut Encoder, value: &str, opening: bool) -> Result<(), MathError> {
    if value.is_empty() {
        return Ok(());
    }
    encoder.push(if opening {
        "\\mathopen{"
    } else {
        "\\mathclose{"
    })?;
    // Delimiter presentation is independent from a mathematical atom's font.
    literal(encoder, value, EquationFont::Roman)?;
    encoder.push("}")
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
    encoder.push("\\begin{matrix}")?;
    for row in 0..rows {
        if row > 0 {
            encoder.push(" \\\\ ")?;
        }
        for (column_index, column) in matrix.columns.iter().enumerate() {
            if column_index > 0 {
                encoder.push(" & ")?;
            }
            if let Some(cell) = column.children.get(row) {
                encoder.child(0, |encoder| {
                    encoder.child(column_index, |encoder| {
                        encoder.child(row, |encoder| {
                            expression_tex(encoder, cell, effective_font(column, font))
                        })
                    })
                })?;
            }
        }
    }
    encoder.push("\\end{matrix}")
}
