//! Executed facts remain observable even if validation discards their nodes.
use libmandoc_rs::{Node, NodeKind, Parser};

fn heads(node: &Node, epochs: &mut Vec<usize>) {
    if node.kind == NodeKind::Block && matches!(node.macro_name.as_deref(), Some("TP" | "It")) {
        epochs.push(node.flow_epoch);
    }
    for child in &node.children {
        heads(child, epochs);
    }
}

#[test]
fn flow_generations_follow_execution_and_survive_validation_and_session_release() {
    for (preamble, first, last, paragraph) in [
        (
            ".TH PROBE 1\n.SH OPTIONS\n",
            ".TP\n.B -a\n",
            ".TP\n.B -b\nBody.\n",
            "PP",
        ),
        (
            ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n",
            ".It Fl a\n",
            ".It Fl b\nBody.\n.El\n",
            "Pp",
        ),
        (
            ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n",
            ".It Fl a\n",
            ".It Fl b\nBody.\n.El\n",
            "Lp",
        ),
    ] {
        for (request, boundary) in [
            (format!(".{paragraph}\n"), true),
            (format!(".if 1 .{paragraph}\n"), true),
            (format!(".if 0 .{paragraph}\n"), false),
            (format!(".if 0 \\{{\\\n.{paragraph}\n.\\}}\n"), false),
            (format!(".de PAUSE\n.{paragraph}\n..\n"), false),
            (format!(".de PAUSE\n.{paragraph}\n..\n.PAUSE\n"), true),
        ] {
            let report = Parser::default()
                .parse_bytes(
                    "probe.1",
                    format!("{preamble}{first}{request}{last}").as_bytes(),
                )
                .unwrap();
            let mut epochs = Vec::new();
            heads(&report.document.root, &mut epochs);
            assert_eq!(epochs.len(), 2, "{request:?}");
            assert_eq!(epochs[0] != epochs[1], boundary, "{paragraph} {request:?}");
            let other = Parser::default()
                .parse_bytes("probe.1", format!("{preamble}{first}{last}").as_bytes())
                .unwrap();
            let mut reset = Vec::new();
            heads(&other.document.root, &mut reset);
            assert_eq!(reset[0], reset[1]);
            assert_eq!(reset[0], epochs[0]);
        }
    }
}
