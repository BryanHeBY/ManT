//! Retired native units expose accepted loop rows exactly once to consumers.

const HEADER: &str = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n";

fn body_from_json(source: &str) -> String {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let query: mant_ir::ResolvedContent = restored.into();
    let output = mant_render::render_query_man(&query);
    output
        .split_once("DESCRIPTION\n")
        .unwrap()
        .1
        .split_once("\n\nNEXT\n")
        .unwrap()
        .0
        .to_owned()
}

#[test]
fn accepted_empty_loops_and_rejected_tails_keep_exact_physical_rows() {
    // Every complete source ran pinned pristine ASCII/UTF-8/HTML/tree/lint
    // before these assertions. term_fill() treats NBRZW as graph (340-349),
    // term_field() emits no scalar for it (397), and term_flushln()217 still
    // closes each accepted loop row before the rejected suffix's own tail.
    for count in [1, 2, 64, 128, 256, 512, 1024, 2048, 4096, 8192] {
        let prefix = "\\&\\p ".repeat(count);
        for (inherited, before) in [(false, ""), (true, ".No BEFORE\\c\n")] {
            let printed = if inherited { "BEFORE" } else { "" };
            for (word, expected) in [
                (
                    format!("{prefix}\\p REJECTED"),
                    format!("{printed}{}AFTER", "\n".repeat(count + 1)),
                ),
                (
                    format!("{prefix}ACCEPTED"),
                    format!("{printed}{}ACCEPTED\nAFTER", "\n".repeat(count)),
                ),
                (prefix.clone(), format!("{printed}\nAFTER")),
                (
                    format!("{}REJECTED", "\\p ".repeat(count)),
                    if inherited && count == 1 {
                        "BEFORE\nREJECTED\nAFTER".to_owned()
                    } else if inherited {
                        "BEFORE\n\nAFTER".to_owned()
                    } else {
                        "\nAFTER".to_owned()
                    },
                ),
            ] {
                let source =
                    format!("{HEADER}{before}.No \"{word}\"\n.No AFTER\n.fi\n.Sh NEXT\n.No END\n");
                assert_eq!(body_from_json(&source), expected, "{source}");
            }
        }
    }
}

#[test]
fn native_unit_projection_preserves_retired_rows_and_semantic_wrappers() {
    // Forty-eight exact sources ran all five pinned profiles first.
    // End-only .mc flushes with NOBREAK but installs no decoration
    // (roff_term.c:147-156). br/sp retire that earlier row and spacing
    // before the new buffer starts. Lk executes its label/colon/target
    // through term_word (mdoc_term.c:1881-1915); wrappers do not change
    // native accepted intervals or authorize an extra empty-row projection.
    for count in [1, 2, 64, 1024] {
        let word = format!("{}\\p REJECTED", "\\&\\p ".repeat(count));
        for (wrapper, argument) in [("No", ""), ("Em", ""), ("Lk", "https://example.org ")] {
            for (control, extra_rows) in [("", 0), (".mc\n", 0), (".br\n", 1), (".sp 1\n", 2)] {
                let source = format!(
                    "{HEADER}.No BEFORE\\c\n{control}.{wrapper} {argument}\"{word}\"\n.No AFTER\n.fi\n.Sh NEXT\n.No END\n"
                );
                assert_eq!(
                    body_from_json(&source),
                    format!("BEFORE{}AFTER", "\n".repeat(count + 1 + extra_rows)),
                    "{source}"
                );
            }
        }
    }
}

#[test]
fn accepted_scalar_offsets_exclude_consumed_and_rejected_word_blanks() {
    // Eighteen exact sources ran the five pristine profiles first.
    // term_fill()299-312 accepts a prior graph before the space/marker,
    // and term_flushln()205-207 consumes that space before its endline.
    // Only accepted owner scalars position the event after native filtering.
    for (word, accepted, breaks) in [
        ("\\& \\p Y", "", 2),
        ("\\& \\p \\& \\p \\p REJECTED", "", 2),
        ("A\\p \\& \\p B\\p \\& \\p \\p REJECTED", "A", 3),
    ] {
        for (wrapper, argument) in [("No", ""), ("Em", ""), ("Lk", "https://example.org ")] {
            for (inherited, before) in [(false, ""), (true, ".No BEFORE\\c\n")] {
                let source = format!(
                    "{HEADER}{before}.{wrapper} {argument}\"{word}\"\n.No AFTER\n.fi\n.Sh NEXT\n.No END\n"
                );
                let printed = if inherited { "BEFORE" } else { "" };
                assert_eq!(
                    body_from_json(&source),
                    format!("{printed}{accepted}{}AFTER", "\n".repeat(breaks)),
                    "{source}"
                );
            }
        }
    }
}
