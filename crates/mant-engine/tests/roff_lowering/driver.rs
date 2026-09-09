//! Original combinations fixing the single source-order driver's state handoffs.
use super::{anchor_owner_lines, font_boundaries::assert_style};
use mant_ir::{DocumentIndex, NodeId};

fn mdoc(body: &str) -> mant_engine::ResolvedContent {
    mant_engine::query_roff_bytes(
        format!(".Dd September 9, 2026\n.Dt DRIVER 1\n.Os\n.Sh DESCRIPTION\n{body}\n").as_bytes(),
    )
    .unwrap()
}

#[test]
fn driver_font_scope_restores_current_but_keeps_previous_selection() {
    let query = mdoc(".ft B\nALPHA\n.Bf -emphasis\nBETA\n.ft R\nINNER\n.Ef\nGAMMA\n.ft P\nDELTA");
    for (word, style) in [
        ("ALPHA", 1),
        ("BETA", 2),
        ("INNER", 0),
        ("GAMMA", 1),
        ("DELTA", 2),
    ] {
        assert_style(&query, word, style);
    }
    let text = mant_engine::render_query_text(&query);
    for word in ["ALPHA", "BETA", "INNER", "GAMMA", "DELTA"] {
        assert_eq!(text.matches(word).count(), 1, "{text}");
    }
}

#[test]
fn driver_nested_containers_inherit_only_executed_spacing_state() {
    // The list makes this an actual structural enclosure. Inline-only
    // enclosures use the separate native inline policy, not this driver.
    let query = mdoc(
        ".Ao\n.Bl -item -compact\n.It\n.Bf -emphasis\n.No LEFT\n.Sm off\n.No RIGHT NEXT\n.Ef\n.El\n.Ac\n.No TAIL\n.Sm on\n.No END",
    );
    let text = mant_engine::render_query_text(&query);
    // The first boundary survives `.Sm off`; later arguments concatenate.
    // Leaving the structural child inherits that state without replaying it.
    assert!(
        text.contains("LEFT RIGHTNEXT") && text.contains(">TAIL END"),
        "{text}"
    );
    // `.No` explicitly selects normal font even inside the emphasis scope.
    assert_style(&query, "RIGHT", 0);
    assert_style(&query, "TAIL", 0);
    for word in ["LEFT", "RIGHT", "NEXT", "TAIL", "END"] {
        assert_eq!(text.matches(word).count(), 1, "{text}");
    }
}

#[test]
fn driver_empty_mode_changes_consume_hp_first_line_only_once() {
    for boundary in [".nf\n.fi", ".EX\n.EE", ".nf\n.fi\n.nf\n.fi"] {
        let source = format!(
            ".TH DRIVER 1\n.SH DESCRIPTION\n.HP 12\n{boundary}\nFIRST\n.br\nSECOND\n.PP\nRESET\n"
        );
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let text = mant_engine::render_query_text(&query);
        for (word, expected) in [("FIRST", 12), ("SECOND", 12), ("RESET", 0)] {
            let column = text.lines().find_map(|line| line.find(word));
            assert_eq!(column, Some(expected), "{source}\n{text}");
        }
    }
}

#[test]
fn driver_font_containers_keep_capture_break_and_final_flush_distinct() {
    for request in ["ce", "rj"] {
        for literal in [false, true] {
            let body = format!(".Bf -symbolic\n.{request} 2\nALPHA\\c\n.br\nBETA\n.Ef\nTAIL");
            let query = mdoc(&if literal {
                format!(".Bd -literal -compact\n{body}\n.Ed")
            } else {
                body
            });
            let text = mant_engine::render_query_text(&query);
            assert!(text.contains("ALPHA\n\nBETA"), "{text}");
            assert!(!text.contains("ALPHA\n\n\nBETA"), "{text}");
            assert_style(&query, "ALPHA", 1);
            assert_style(&query, "BETA", 1);
            assert_style(&query, "TAIL", 0);
            assert_eq!(text.matches("ALPHA").count(), 1);
        }
    }
}

#[test]
fn driver_invisible_controls_keep_pending_target_spelling_and_source() {
    let query =
        mdoc(".Tg Boundary.Target\n.Sm off\n.ft B\n.Pp\n.Bd -literal -compact\nBODY\n.Ed\nTAIL");
    let document = query.document.as_ref().unwrap();
    let index = DocumentIndex::build(document);
    assert_eq!(
        index.fragment_target("Boundary.Target").map(NodeId::as_str),
        Some("boundary-target")
    );
    assert!(anchor_owner_lines(document).contains(&("boundary-target".into(), 5)));
    let text = mant_engine::render_query_text(&query);
    assert_eq!(text.matches("BODY").count(), 1, "{text}");
    assert_eq!(text.matches("TAIL").count(), 1, "{text}");
    assert!(
        !text.contains("Boundary.Target") && !text.contains("off"),
        "{text}"
    );
}

#[test]
fn driver_author_split_executes_a_boundary_without_consuming_the_name() {
    let query = mdoc(
        ".Bf -emphasis\n.An -split\n.An ALPHA\n.An BETA\n.An -nosplit\n.An GAMMA\n.An DELTA\n.Ef\nTAIL",
    );
    let text = mant_engine::render_query_text(&query);
    assert!(text.contains("ALPHA\nBETA GAMMA DELTA TAIL"), "{text}");
    for word in ["ALPHA", "BETA", "GAMMA", "DELTA", "TAIL"] {
        assert_eq!(text.matches(word).count(), 1, "{text}");
    }
    assert_style(&query, "TAIL", 0);
}
