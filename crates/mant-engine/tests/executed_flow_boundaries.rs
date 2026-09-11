//! Original source regressions for executed requests outside display wrappers.
use mant_loader::load_roff_bytes;
use mant_render::render_query_text;

fn rendered(dialect: &str, body: &str) -> String {
    let header = if dialect == "man" {
        ".TH PROBE 1\n.SH TEST\n"
    } else {
        ".Dd September 11, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n"
    };
    let query = load_roff_bytes(format!("{header}{body}\n").as_bytes()).unwrap();
    assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
    render_query_text(&query)
}

#[test]
fn fill_requests_end_the_current_row_even_when_the_mode_is_unchanged() {
    // Pinned roff_term.c dispatches fi/nf to roff_term_pre_br in both dialects.
    for dialect in ["man", "mdoc"] {
        for body in [
            "ALPHA\n.fi\nBETA",
            "ALPHA\\c\n.fi\nBETA",
            "ALPHA\n.fi\n.fi\nBETA",
            ".nf\nALPHA\n.nf\nBETA\n.fi",
            ".nf\nALPHA\\c\n.nf\nBETA\n.fi",
            ".nf\nALPHA\n.fi\n.nf\nBETA\n.fi",
        ] {
            let text = rendered(dialect, body);
            assert!(text.contains("ALPHA\nBETA"), "{dialect}: {body}: {text:?}");
            assert!(
                !text.contains("ALPHA\n\nBETA"),
                "{dialect}: {body}: {text:?}"
            );
        }
    }
}

#[test]
fn raw_mdoc_no_fill_paragraphs_keep_their_independent_vertical_request() {
    // Not Bd: generic block dispatch must execute Pp before no-fill fallback.
    // mdoc_term.c termp_pp_pre always calls term_vspace for a retained Pp.
    for (body, blanks) in [
        ("ALPHA\n.Pp\nBETA", 1),
        ("ALPHA\n.sp 1\n.Pp\nBETA", 2),
        ("ALPHA\n.sp 0\n.Pp\nBETA", 1),
        ("ALPHA\\c\n.Pp\nBETA", 1),
        ("ALPHA\n.Pp\n.Pp\nBETA", 1),
        ("ALPHA\n.Pp\n.sp 1\nBETA", 1),
    ] {
        let text = rendered("mdoc", &format!(".nf\n{body}\n.fi\nAFTER"));
        assert!(
            text.contains(&format!("ALPHA{}BETA", "\n".repeat(blanks + 1))),
            "{body}: {text:?}"
        );
    }
}

#[test]
fn empty_fill_requests_do_not_invent_rows_or_override_skipped_execution() {
    for dialect in ["man", "mdoc"] {
        let text = rendered(dialect, ".fi\n.fi\nALPHA\n.if 0 .fi\nBETA\n.fi\n.fi");
        assert!(text.contains("ALPHA BETA"), "{dialect}: {text:?}");
        let text = rendered(dialect, ".nf\nALPHA\n\n.nf\nBETA\n.fi");
        assert!(text.contains("ALPHA\n\nBETA"), "{dialect}: {text:?}");
    }
}
