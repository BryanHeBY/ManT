//! Delimiter origin is independent from native source-line position.
use super::*;

const BODY: &str =
    ".EQ\ndelim $$\n.EN\nBefore $a$ and\n$b$ after.\n.EQ\nx over y\n.EN\nThen\n$c$ done.\n";

fn equations<'a>(node: &'a Node, output: &mut Vec<&'a Node>) {
    if node.equation.is_some() {
        output.push(node);
    }
    for child in &node.children {
        equations(child, output);
    }
}

#[test]
fn executed_delimiters_keep_their_origin_across_lines_and_explicit_equations() {
    // Exact man/mdoc sources ran through pinned tree/HTML/UTF-8 first.
    // roff.c::roff_eqndelim toggles eqn_inline before reparsing EQ/EN;
    // roff_EQ sets NODE_LINE independently from the delimiter origin.
    for header in [
        ".TH MATH 1\n.SH DESCRIPTION\n",
        ".Dd November 9, 2026\n.Dt MATH 1\n.Os\n.Sh DESCRIPTION\n",
    ] {
        let source = format!("{header}{BODY}");
        let report = Parser::default()
            .parse_bytes("placement.1", source.as_bytes())
            .unwrap();
        let mut nodes = Vec::new();
        equations(&report.document.root, &mut nodes);
        assert_eq!(nodes.len(), 5);
        assert_eq!(
            nodes.iter().map(|n| n.equation_inline).collect::<Vec<_>>(),
            [Some(false), Some(true), Some(true), Some(false), Some(true)]
        );
        assert!(nodes[2].flags.line_start);
        assert!(nodes[4].flags.line_start);
        assert!(!nodes[1].flags.line_start);
    }
}

#[test]
fn macro_expansion_keeps_executed_delimiter_origin() {
    // Exact input ran pinned tree/HTML/UTF-8. roff_eqndelim executes while
    // reparsing the macro body, before the node enters the owned snapshot.
    let source = b".TH MATH 1\n.EQ\ndelim $$\n.EN\n.de Show\nBefore\n$a$ after\n..\n.SH DESCRIPTION\n.Show\n";
    let report = Parser::default().parse_bytes("macro.1", source).unwrap();
    let mut nodes = Vec::new();
    equations(&report.document.root, &mut nodes);
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[1].equation_inline, Some(true));
    // The pinned tree retains these macro-reparse tokens in eqn_read's
    // payload. Copy the same expression along with its delimiter origin.
    assert_eq!(
        nodes[1].equation.as_ref().unwrap().readable_text(),
        r"a \& . EQ after"
    );
}

#[test]
#[cfg(feature = "serde")]
fn equation_origin_round_trips_and_accepts_older_owned_snapshots() {
    fn remove_origin(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                object.remove("equation_inline");
                for child in object.values_mut() {
                    remove_origin(child);
                }
            }
            serde_json::Value::Array(array) => {
                for child in array {
                    remove_origin(child);
                }
            }
            _ => {}
        }
    }
    let source = format!(".TH MATH 1\n.SH DESCRIPTION\n{BODY}");
    let report = Parser::default()
        .parse_bytes("placement.1", source.as_bytes())
        .unwrap();
    let mut wire = serde_json::to_value(&report).unwrap();
    assert_eq!(
        serde_json::from_value::<crate::ParseReport>(wire.clone()).unwrap(),
        report
    );
    remove_origin(&mut wire);
    let old: crate::ParseReport = serde_json::from_value(wire).unwrap();
    let mut nodes = Vec::new();
    equations(&old.document.root, &mut nodes);
    assert!(nodes.iter().all(|node| node.equation_inline.is_none()));
}
