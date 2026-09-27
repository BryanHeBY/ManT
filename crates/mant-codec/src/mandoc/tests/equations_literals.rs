//! Quoted eqn atoms and parser-specific compatibility normalizations.

use super::*;

fn equation_from_native(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::EquationBox> {
    node.equation
        .as_ref()
        .or_else(|| node.children.iter().find_map(equation_from_native))
}

fn ldots_eligibility(box_node: &libmandoc_rs::EquationBox, flags: &mut Vec<bool>) {
    if box_node.text.as_deref() == Some("ldots") {
        flags.push(box_node.gnu_ldots);
    }
    for child in &box_node.children {
        ldots_eligibility(child, flags);
    }
}

#[test]
fn gnu_ldots_requires_a_complete_unquoted_post_substitution_token() {
    // Every exact source below was checked with fixed CVS -Tutf8/-Thtml/
    // -Tlint before these assertions. CVS eqn.c::eqn_next expands define
    // aliases before eqn_parse creates EQN_TEXT; its default branch then
    // splits punctuation/digits for font selection. A split "ldots" fragment
    // is not a complete eqn token, and quoted text bypasses lookup.
    let cases: &[(&str, &str, &[bool])] = &[
        ("\"ldots\"", "ldots", &[false]),
        ("ldots", "...", &[true]),
        ("\"ldots\" ldots", "ldots ...", &[false, true]),
        ("ldots2", "ldots 2", &[false]),
        ("2ldots", "2 ldots", &[false]),
        ("ldots_name", "ldots _ name", &[false]),
        ("ldots+1", "ldots + 1", &[false]),
        ("define xx /ldots2/ xx", "ldots 2", &[false]),
        ("define xx /ldots/ xx", "...", &[true]),
        ("roman ldots2", "ldots2", &[]),
        ("italic ldots2", "ldots2", &[]),
        ("bold ldots2", "ldots2", &[]),
        ("roman ldots", "...", &[true]),
        ("italic ldots", "...", &[true]),
        ("bold ldots", "...", &[true]),
    ];
    for &(source_expression, expected, expected_flags) in cases {
        let source = format!(
            ".TH REVIEW 7 \"September 28, 2026\"\n.SH DESCRIPTION\n.EQ\n{source_expression}\n.EN\n"
        );
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("review.7", source.as_bytes())
            .expect("native equation parse");
        let equation = equation_from_native(&native.document.root).expect("native equation");
        let mut flags = Vec::new();
        ldots_eligibility(equation, &mut flags);
        assert_eq!(
            flags, expected_flags,
            "native eligibility: {source_expression}"
        );
        assert_eq!(
            equation.readable_text(),
            expected,
            "native: {source_expression}"
        );

        let document = parse_manual_bytes(std::path::Path::new("review.7"), source.as_bytes())
            .expect("IR equation parse");
        let Block::Equation {
            value,
            expression: Some(expression),
            ..
        } = &document.sections[0].blocks[0]
        else {
            panic!("IR equation missing");
        };
        assert_eq!(value, expected, "IR value: {source_expression}");
        assert_eq!(
            expression.readable_text(),
            expected,
            "IR tree: {source_expression}"
        );
    }
}
