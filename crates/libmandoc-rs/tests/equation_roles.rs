//! Complete-token evidence must not change native equation execution.

use libmandoc_rs::{EquationAtomRole, EquationBox, EquationFont, Node, Parser};

const SOURCE: &[u8] = b".TH EQN 7\n.SH BODY\n.EQ\nsin \"sin\" roman \"sin\" define f /sin/ f define q /\"sin\"/ q x2 12 + alpha\n.EN\n";

fn equation(node: &Node) -> Option<&EquationBox> {
    node.equation
        .as_ref()
        .or_else(|| node.children.iter().find_map(equation))
}

fn atoms<'a>(expression: &'a EquationBox, output: &mut Vec<&'a EquationBox>) {
    if expression.text.is_some() {
        output.push(expression);
    }
    for child in &expression.children {
        atoms(child, output);
    }
}

#[test]
fn complete_function_and_literal_atoms_survive_macro_expansion_and_owned_transfer() {
    use EquationAtomRole::{Function, Literal, Unknown};

    // The exact SOURCE was run with the pinned -Ttree and -Thtml oracle.
    // CVS eqn_next substitutes definitions before returning FUNC/QUOTED;
    // eqn_parse then keeps only font/text unless this evidence is retained.
    // Quoted "sin" is an italic mathematical atom, not ordinary prose.
    let report = Parser::default().parse_bytes("roles.7", SOURCE).unwrap();
    let expression = equation(&report.document.root).unwrap();
    let mut found = Vec::new();
    atoms(expression, &mut found);
    let actual = found
        .iter()
        .map(|atom| (atom.text.as_deref().unwrap(), atom.atom_role))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            ("sin", Function),
            ("sin", Literal),
            ("sin", Literal),
            ("sin", Function),
            ("sin", Literal),
            ("x", Unknown),
            ("2", Unknown),
            ("12", Unknown),
            ("+", Unknown),
            (r"\[*a]", Unknown),
        ]
    );
    assert_eq!(found[0].font, EquationFont::Roman);
    assert_eq!(found[1].font, EquationFont::Italic);
    assert_eq!(found[2].font, EquationFont::Roman);
    assert_eq!(expression.atom_role, Unknown);
}

#[test]
#[cfg(feature = "render")]
fn retaining_atom_roles_does_not_change_reference_mathml() {
    // Exact pinned -Thtml -Ofragment SOURCE output, checked before assertion.
    // eqn_html.c chooses mi/mn/mo from spelling, independently of our evidence.
    let report = libmandoc_rs::Renderer::new(libmandoc_rs::RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("roles.7", SOURCE)
        .unwrap();
    let math = report
        .output
        .split_once("<math class=\"eqn\">")
        .unwrap()
        .1
        .split_once("</math>")
        .unwrap()
        .0
        .trim();
    assert_eq!(
        math,
        concat!(
            "<mrow><mi>sin</mi><mi fontstyle=\"italic\">sin</mi>",
            "<mrow><mi>sin</mi></mrow><mi>sin</mi>",
            "<mi fontstyle=\"italic\">sin</mi><mi>x</mi>",
            "<mn>2</mn><mn>12</mn><mo>+</mo><mo>&#x03B1;</mo></mrow>"
        )
    );
}

#[test]
fn matrix_reading_keeps_authored_fences_on_the_columns_owner() {
    // Exact source was run with pinned -Ttree/-Thtml/-Tutf8 first. The tree
    // owns both fences; eqn_term.c:135-148 and eqn_html.c:49-94 skip the
    // columns wrapper while printing a grid. ManT's readable projection is
    // deliberately content-preserving, not a copy of that renderer omission.
    let source = b".TH EQN 7\n.SH BODY\n.EQ\nmatrix left ( lcol { a above b } right )\n.EN\n";
    let report = Parser::default().parse_bytes("matrix.7", source).unwrap();
    let expression = equation(&report.document.root).unwrap();
    assert_eq!(expression.readable_text(), "matrix((a b))");
}

#[test]
fn matrix_reading_does_not_skip_public_header_decorations_or_fences() {
    // The exact unmodified source was run with pinned tree/HTML/UTF-8.
    // Mutations below independently exercise the public owned model; they
    // do not claim that the native parser creates decorated column headers.
    let source = b".TH EQN 7\n.SH BODY\n.EQ\nmatrix { lcol { a above b } }\n.EN\n";
    let report = Parser::default().parse_bytes("matrix.7", source).unwrap();
    let original = &equation(&report.document.root).unwrap().children[0];
    for column in [false, true] {
        for (field, glyph) in [(0, "["), (1, "]"), (2, "@"), (3, "_")] {
            let mut expression = original.clone();
            let header = if column {
                &mut expression.children[0].children[0]
            } else {
                &mut expression.children[0]
            };
            match field {
                0 => header.left = Some(glyph.into()),
                1 => header.right = Some(glyph.into()),
                2 => header.top = Some(glyph.into()),
                _ => header.bottom = Some(glyph.into()),
            }
            let readable = expression.readable_text();
            assert!(readable.contains(glyph), "{column}/{field}: {readable}");
            assert!(readable.contains('a') && readable.contains('b'));
        }
    }
    let mut styled = original.clone();
    styled.children[0].font = EquationFont::Bold;
    styled.children[0].children[0].font = EquationFont::Italic;
    assert_eq!(styled.readable_text(), "matrix(a; b)");
}

#[test]
fn native_accent_wrapper_keeps_the_complete_pile_as_its_operand() {
    // Exact source was run with fixed CVS -Ttree/-Thtml/-Tutf8 before this
    // assertion. eqn.c:716-770 puts the bar on a unary List around the Pile;
    // eqn_term.c:135-148 still skips singleton row wrappers inside that Pile.
    // The shared readable contract makes the complete accent operand explicit.
    let source = b".TH EQN 7\n.SH BODY\n.EQ\npile { a above b } bar\n.EN\n";
    let report = Parser::default().parse_bytes("accent.7", source).unwrap();
    let expression = equation(&report.document.root).unwrap();
    assert_eq!(expression.children[0].top.as_deref(), Some(r"\[rn]"));
    assert_eq!(expression.readable_text(), r"(a b)\[rn]");
}
