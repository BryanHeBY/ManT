//! Detached definition bodies retain the native pending-head line boundary.
use mant_ir::{Block, DefinitionItem};
use mant_loader::load_roff_bytes;
use mant_render::render_query_text;

fn item(query: &mant_ir::ResolvedContent) -> &DefinitionItem {
    let Block::DefinitionList { items, .. } =
        &query.document.as_ref().unwrap().sections[0].blocks[0]
    else {
        panic!("expected definition list")
    };
    items.last().unwrap()
}

#[test]
fn explicit_initial_body_requests_end_short_heads_without_adding_blank_rows() {
    // Pinned CVS roff_term.c maps fi/nf to br; groff agrees on these original
    // inputs. The tag is already buffered when each body request executes.
    for head in [
        ".TP\n.B window\n",
        ".IP window\n",
        ".TP\n.B first\n.TQ\n.B window\n",
    ] {
        for request in [
            ".br\n",
            ".br\n.br\n",
            ".fi\n",
            ".nf\n",
            ".fi\n.nf\n",
            ".sp 0\n",
            ".in +2n\n",
            ".ti 0\n",
            ".ce 0\n",
            ".ce 1\n",
            ".rj 0\n",
            ".rj 1\n",
            ".EX\n",
            ".EE\n",
        ] {
            for prefix in ["", ".ft B\n", ".PD 0\n", "\\fB\n"] {
                let source = format!(".TH PROBE 1\n.SH TEST\n{head}{prefix}{request}BODY\n");
                let query = load_roff_bytes(source.as_bytes()).unwrap();
                let text = render_query_text(&query);
                let owner = item(&query);
                assert!(!owner.layout.inline_term, "{source}\n{text}");
                let lines: Vec<_> = text.lines().collect();
                let head_row = lines
                    .iter()
                    .position(|line| line.trim() == "window")
                    .unwrap();
                assert_eq!(lines[head_row + 1].trim(), "BODY", "{source}\n{text}");
                assert!(lines[head_row + 1].starts_with(' '), "{source}\n{text}");
                assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
                if prefix == ".ft B\n" {
                    assert!(
                        serde_json::to_string(&owner.description)
                            .unwrap()
                            .contains("strong")
                    );
                }
            }
        }
    }
}

#[test]
fn requests_after_printable_body_do_not_retroactively_stack_the_head() {
    for prefix in ["", ".ft B\n", ".PD 0\n", ".ta 4n\n", ".ll 50n\n"] {
        for request in [".br", ".fi", ".nf"] {
            let source =
                format!(".TH PROBE 1\n.SH TEST\n.TP\n.B x\n{prefix}FIRST\n{request}\nSECOND\n");
            let query = load_roff_bytes(source.as_bytes()).unwrap();
            let text = render_query_text(&query);
            assert!(item(&query).layout.inline_term, "{source}\n{text}");
            assert!(
                text.lines()
                    .any(|line| line.starts_with('x') && line.contains("FIRST")),
                "{source}\n{text}"
            );
            assert!(
                text.lines().any(|line| line.trim() == "SECOND"),
                "{source}\n{text}"
            );
        }
    }
}

#[test]
fn ordinary_fitting_and_mdoc_targeted_definitions_keep_their_existing_contracts() {
    for (head, inline) in [
        (".TP\n.B x\n", true),
        (".TP\n.B longer-than-the-tag-width\n", false),
        (".TP\n.B longer-than-the-tag-width\n.TQ\n.B x\n", true),
    ] {
        let source = format!(".TH PROBE 1\n.SH TEST\n{head}BODY\n");
        let query = load_roff_bytes(source.as_bytes()).unwrap();
        assert_eq!(item(&query).layout.inline_term, inline, "{source}");
    }
    let query = load_roff_bytes(b".Dd September 11, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bl -tag -width Ds\n.Tg Exact.Target\n.It Fl x\nBODY\n.El\n").unwrap();
    assert!(item(&query).layout.inline_term);
    assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
    let json = serde_json::to_string(query.document.as_ref().unwrap()).unwrap();
    assert!(json.contains("Exact.Target"));
    assert!(render_query_text(&query).contains("BODY"));
}
