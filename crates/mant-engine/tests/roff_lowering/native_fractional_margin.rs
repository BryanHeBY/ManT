//! Native measured HEAD capacity remains in basic units through layout.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::{Block, HeadBodyRelation, Inline};

const HEADER: &str =
    ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn find(node: &Node, predicate: impl Fn(&Node) -> bool + Copy) -> Option<&Node> {
    if predicate(node) {
        Some(node)
    } else {
        node.children
            .iter()
            .find_map(|child| find(child, predicate))
    }
}

fn has_link(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Link { .. } => true,
        Inline::Strong { children } | Inline::Emphasis { children } => has_link(children),
        _ => false,
    })
}

#[test]
fn fractional_head_margins_control_the_actual_head_body_relation() {
    // Each exact source below ran on pristine CVS -Tascii/-Tutf8/-Ttree/
    // -Tlint before these assertions. Filled tabs report their expected
    // warning; the no-fill inputs have clean lint. mdoc_term.c:735-747
    // resolves width+2n in basic units, and 846-856 sets the HEAD rmargin.
    // term.c:250-253 compares vbr+trailspace with vfield+half an EN. A
    // trailing default tab therefore overruns 4.4n, but fits 4.5n/4.6n.
    // Styles and semantic links cannot change that native row decision.
    for width in ["4.4", "4.5", "4.6"] {
        for mode in [
            "plain",
            "strong",
            "emphasis",
            "code",
            "section",
            "nofill",
            "fill_switch",
        ] {
            let macro_name = match mode {
                "strong" => "Sy ",
                "emphasis" => "Em ",
                "code" => "Li ",
                "section" => "Sx ",
                _ => "",
            };
            let prefix = if mode == "nofill" { ".nf\n" } else { "" };
            let suffix = if mode == "nofill" { ".fi\n" } else { "" };
            let label = if mode == "fill_switch" {
                ".It Xo\n.nf\n.No \"X\t\"\n.fi\n.Xc\n".to_owned()
            } else {
                format!(".It {macro_name}\"X\t\"\n")
            };
            let mut source = format!(
                "{HEADER}{prefix}.Bl -tag -width {width}n\n{label}.No BodyWord\n.El\n{suffix}"
            );
            if mode == "section" {
                // Resolve Sx to a real heading so the JSON/consumer path
                // preserves the semantic Link instead of unwrapping it.
                source.push_str(".Sh X\nother\n");
            }
            let runs_in = width != "4.4" && !matches!(mode, "nofill" | "fill_switch");
            assert_case(&source, mode, runs_in);
        }
    }
}

fn assert_case(source: &str, mode: &str, runs_in: bool) {
    let report = Parser::default()
        .parse_bytes("fractional-head.1", source.as_bytes())
        .unwrap();
    let item = find(&report.document.root, |node| {
        node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("It")
    })
    .unwrap();
    let head = item
        .children
        .iter()
        .find(|node| node.kind == NodeKind::Head)
        .unwrap();
    let text = find(head, |node| node.text.as_deref() == Some("X\t"))
        .expect("the measured tab operand belongs to It HEAD");
    assert_eq!(
        text.flags.no_fill,
        matches!(mode, "nofill" | "fill_switch"),
        "{source}"
    );
    if mode == "fill_switch" {
        assert!(find(head, |node| node.macro_token.as_deref() == Some("nf")).is_some());
        assert!(find(head, |node| node.macro_token.as_deref() == Some("fi")).is_some());
    }

    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let decoded: mant_ir::ResolvedContent = decoded.into();
    let document = decoded.document.as_ref().unwrap();
    let item = document
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .find_map(|block| match block {
            Block::DefinitionList { items, .. } => items.first(),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        (item.head_body_relation, item.layout.body_alignment),
        if runs_in {
            (
                HeadBodyRelation::separated(),
                mant_ir::DefinitionBodyAlignment::Indented,
            )
        } else {
            (
                HeadBodyRelation::Separate,
                mant_ir::DefinitionBodyAlignment::Indented,
            )
        },
        "{source}"
    );
    if mode == "section" {
        assert!(item.terms.iter().any(|term| has_link(term)), "{source}");
    }
    let rendered = mant_render::render_query_man(&decoded);
    let body = rendered.split_once("DESCRIPTION\n").unwrap().1;
    let body = if mode == "section" {
        body.split_once("\n\nX\n").unwrap().0
    } else {
        body
    };
    let rows: Vec<_> = body
        .trim_end_matches('\n')
        .lines()
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    let expected = if runs_in {
        vec!["X BodyWord"]
    } else if mode == "nofill" {
        vec!["", "X", "BodyWord"]
    } else {
        vec!["X", "BodyWord"]
    };
    assert_eq!(rows, expected, "{source}");
}
