//! `ce`/`rj` counts are control operands; their captured lines remain content.
use mant_loader::load_roff_bytes;
use mant_render::render_query_text;

fn source_for(mode: &str, body: &str) -> String {
    let (header, open, close) = match mode {
        "man" => (".TH PROBE 1\n.SH DESCRIPTION", "", ""),
        "nf" => (".TH PROBE 1\n.SH DESCRIPTION", ".nf\n", ".fi\n"),
        "EX" => (".TH PROBE 1\n.SH DESCRIPTION", ".EX\n", ".EE\n"),
        "mdoc" => (
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            "",
            "",
        ),
        "literal" => (
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Bd -literal -compact\n",
            ".Ed\n",
        ),
        "unfilled" => (
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Bd -unfilled -compact\n",
            ".Ed\n",
        ),
        _ => unreachable!(),
    };
    format!("{header}\n{open}BEFORE\n{body}\nAFTER\n{close}")
}

#[test]
fn alignment_control_counts_never_replace_their_captured_numeric_lines() {
    for mode in ["man", "nf", "EX", "mdoc", "literal", "unfilled"] {
        for request in ["ce", "rj"] {
            for operand in ["2", "", "0", "invalid-count"] {
                let source = source_for(mode, &format!(".{request} {operand}\n17\n23"));
                let query = load_roff_bytes(source.as_bytes()).unwrap();
                let text = render_query_text(&query);
                assert!(
                    text.lines().any(|line| line.trim() == "BEFORE"),
                    "{source}\n{text}"
                );
                assert!(
                    !text
                        .lines()
                        .any(|line| !operand.is_empty() && line.trim() == operand),
                    "{source}\n{text}"
                );
                assert!(text.contains("17"), "{source}\n{text}");
                assert!(text.contains("23"), "{source}\n{text}");
                if operand == "2" {
                    assert!(text.contains("BEFORE\n17\n23\nAFTER"), "{source}\n{text}");
                }
            }
        }
    }
}

#[test]
fn alignment_flushes_override_continuations_and_stop_at_native_payload_boundaries() {
    for mode in ["man", "nf", "EX", "mdoc", "literal", "unfilled"] {
        for request in ["ce", "rj"] {
            let source = source_for(mode, &format!(".{request} 2\nALPHA\\c\nBETA"));
            let text = render_query_text(&load_roff_bytes(source.as_bytes()).unwrap());
            assert!(text.contains("ALPHA\nBETA\nAFTER"), "{source}\n{text}");
            let source = source_for(mode, ".rj 2\nALPHA\n.ce 1\nBETA");
            let text = render_query_text(&load_roff_bytes(source.as_bytes()).unwrap());
            assert!(
                text.contains("BEFORE\nALPHA\nBETA\nAFTER"),
                "{source}\n{text}"
            );
        }
    }
}

#[test]
fn alignment_payload_preserves_font_and_independent_spacing_requests() {
    for mode in ["man", "nf", "EX", "mdoc", "literal", "unfilled"] {
        for request in ["ce", "rj"] {
            let source = source_for(mode, &format!(".{request} 2\n.ft B\n17\n23"));
            let query = load_roff_bytes(source.as_bytes()).unwrap();
            let text = render_query_text(&query);
            assert!(text.contains("BEFORE\n\n17\n23\nAFTER"), "{source}\n{text}");
            assert!(
                !text.lines().any(|line| matches!(line.trim(), "B" | "2")),
                "{text}"
            );
            let json = serde_json::to_string(query.document.as_ref().unwrap()).unwrap();
            assert!(json.contains("strong"), "font was not preserved: {json}");
            let source = source_for(mode, &format!(".{request} 2\nALPHA\n.sp 1\nBETA"));
            let text = render_query_text(&load_roff_bytes(source.as_bytes()).unwrap());
            assert!(text.contains("ALPHA\n\n\nBETA"), "{source}\n{text}");
        }
    }
}

#[test]
fn captured_breaks_flush_before_the_group_end_in_every_fill_mode() {
    for mode in ["man", "nf", "EX", "mdoc", "literal", "unfilled"] {
        for alignment in ["ce", "rj"] {
            for request in [".br", ".fi", ".nf", ".ti 3n", ".ti", ".br\n.br", ".fi\n.nf"] {
                for word in ["ALPHA", "ALPHA\\c"] {
                    let source =
                        source_for(mode, &format!(".{alignment} 2\n{word}\n{request}\nBETA"));
                    let query = load_roff_bytes(source.as_bytes()).unwrap();
                    let text = render_query_text(&query);
                    assert!(text.contains("ALPHA\n\nBETA"), "{source}\n{text}");
                    assert!(!text.contains("ALPHA\n\n\nBETA"), "{source}\n{text}");
                }
            }
        }
    }
}

#[test]
fn captured_empty_requests_and_last_line_flush_do_not_escape_the_capture() {
    for mode in ["man", "nf", "EX", "mdoc", "literal", "unfilled"] {
        for alignment in ["ce", "rj"] {
            for request in [".br", ".fi", ".nf", ".ti 3n"] {
                // A request-only group still has its own unconditional flush.
                let source = source_for(mode, &format!(".{alignment} 2\n{request}\nALPHA\nBETA"));
                let text = render_query_text(&load_roff_bytes(source.as_bytes()).unwrap());
                assert!(
                    text.contains("BEFORE\n\nALPHA\nBETA\nAFTER"),
                    "{source}\n{text}"
                );
                // Explicitly end the capture. Native can retain controls
                // after its final text line until the next text is parsed.
                let source = source_for(
                    mode,
                    &format!(".{alignment} 1\nALPHA\n.{alignment} 0\n.br\nBETA"),
                );
                let text = render_query_text(&load_roff_bytes(source.as_bytes()).unwrap());
                assert!(text.contains("ALPHA\nBETA"), "{source}\n{text}");
            }
        }
    }
}

#[test]
fn ordinary_breaks_do_not_acquire_alignment_group_spacing() {
    for mode in ["man", "nf", "EX", "mdoc", "literal", "unfilled"] {
        let source = source_for(mode, "ALPHA\n.br\n.br\nBETA");
        let text = render_query_text(&load_roff_bytes(source.as_bytes()).unwrap());
        assert!(text.contains("ALPHA\nBETA"), "{source}\n{text}");
    }
}
