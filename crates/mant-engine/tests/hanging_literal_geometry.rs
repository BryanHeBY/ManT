//! HP's temporary first line is consumed by source line boundaries, not blocks.

use mant_engine::{query_roff_bytes, render_query_text};
use mant_ir::Block;

fn render(body: &str) -> String {
    let source = format!(".TH PROBE 1\n.SH DESCRIPTION\n{body}");
    render_query_text(&query_roff_bytes(source.as_bytes()).unwrap())
}

fn assert_column(text: &str, token: &str, column: usize) {
    let line = text
        .lines()
        .find(|line| line.trim() == token)
        .unwrap_or_else(|| {
            panic!("missing {token}: {text}");
        });
    assert_eq!(line, format!("{}{token}", " ".repeat(column)), "{text}");
}

#[test]
fn entering_literal_mode_consumes_the_hanging_first_line_even_without_text() {
    for (enter, leave) in [("nf", "fi"), ("EX", "EE")] {
        for (prefix, suffix, origin) in [("", "", 0), (".RS 3\n", ".RE\n", 3)] {
            for first in ["", "INTRO\n"] {
                let text = render(&format!(
                    "{prefix}.HP 4\n{first}.{enter}\nFIRST\nSECOND\n.{leave}\nAFTER\n{suffix}.PP\nRESET\n"
                ));
                if !first.is_empty() {
                    assert_column(&text, "INTRO", origin);
                }
                for token in ["FIRST", "SECOND", "AFTER"] {
                    assert_column(&text, token, origin + 4);
                }
                assert_column(&text, "RESET", 0);
            }
        }
    }
}

#[test]
fn hanging_paragraph_entered_in_literal_mode_keeps_only_its_first_line_outdented() {
    for (enter, leave) in [("nf", "fi"), ("EX", "EE")] {
        for blank in ["", "\n"] {
            let text = render(&format!(
                ".{enter}\n.HP 4\nFIRST\n{blank}SECOND\n.{leave}\nAFTER\n"
            ));
            assert_column(&text, "FIRST", 0);
            assert_column(&text, "SECOND", 4);
            assert_column(&text, "AFTER", 4);
            let boundary = if blank.is_empty() { "\n" } else { "\n\n" };
            assert!(
                text.contains(&format!("FIRST{boundary}    SECOND")),
                "{text}"
            );
        }
    }
}

#[test]
fn literal_hanging_source_continuation_stays_on_one_visible_line() {
    // Preserve explicit source \c, as groff does, instead of the mandoc
    // first-no-fill-child artifact that forcibly breaks this joined line.
    let text = render(".nf\n.HP 4\nFIRST\\c\nSECOND\nTHIRD\n.fi\nAFTER\n");
    assert_column(&text, "FIRSTSECOND", 0);
    assert_column(&text, "THIRD", 4);
    assert_column(&text, "AFTER", 4);
}

#[test]
fn nofill_hanging_geometry_is_preserved_in_ir_not_added_by_the_renderer() {
    let query =
        query_roff_bytes(b".TH PROBE 1\n.SH DESCRIPTION\n.nf\n.HP 4\nFIRST\nSECOND\n.fi\nAFTER\n")
            .unwrap();
    let blocks = &query.document.as_ref().unwrap().sections[0].blocks;
    let [
        Block::Preformatted { layout: first, .. },
        Block::Preformatted { layout: second, .. },
        Block::Paragraph { layout: after, .. },
    ] = blocks.as_slice()
    else {
        panic!("unexpected flow: {blocks:#?}");
    };
    assert_eq!(first.indent_columns, 0);
    assert_eq!(second.indent_columns, 4);
    assert_eq!(after.indent_columns, 4);
}

#[test]
fn explicit_re_levels_restore_the_correct_macro_base_and_prevailing_width() {
    // libmandoc's blk_close resolves RE levels into the AST scopes. Lowering
    // must unwind those scopes, not treat the requested level as a distance.
    let text = render(
        ".RS 3\n.TP 11\nT\nBODY\n.RS\nINNER\n.RS 2\nDEEP\n.RE 2\nLEVELTWO\n.RS\nAGAIN\n.RE\n.RE 1\nROOT\n.RS\nDEFAULT\n.RE\n",
    );
    for (token, column) in [
        ("INNER", 14),
        ("DEEP", 16),
        ("LEVELTWO", 3),
        ("AGAIN", 14),
        ("ROOT", 0),
        ("DEFAULT", 7),
    ] {
        assert_column(&text, token, column);
    }
}
