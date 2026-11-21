//! Native word receipts in Markdown readbacks, search and semantic queries.

use super::{
    EDITABLE_FIELD, FIELD_PROBE, assert_link, assert_readbacks, description, document_links,
    render, run_text_input, without_sgr,
};

const WORD: &str =
    include_str!("../../../../mant-engine/tests/roff_lowering/word_receipts/accepted-word.1");

const HARD_ROW: &str =
    include_str!("../../../../mant-engine/tests/roff_lowering/word_receipts/late-hard-row.1");

const UNICODE_LINK: &str = include_str!(
    "../../../../mant-engine/tests/roff_lowering/word_receipts/accepted-unicode-link.1"
);

const BACKLINE: &str = include_str!(
    "../../../../mant-engine/tests/roff_lowering/word_projection/backline-after-zero-width.1"
);

const UNPRINTED: &str = include_str!(
    "../../../../mant-engine/tests/roff_lowering/word_projection/source-prefix-cross-pass.1"
);

const REARM: &str = include_str!(
    "../../../../mant-engine/tests/roff_lowering/word_projection/rearm-prefix-space.1"
);

const LINK_REARM: &str =
    include_str!("../../../../mant-engine/tests/roff_lowering/word_projection/link-rearm.1");

const DEFERRED_MARKER: &str =
    include_str!("../../../../mant-engine/tests/roff_lowering/word_passes/deferred-marker.1");

const REARMED_MARKER: &str =
    include_str!("../../../../mant-engine/tests/roff_lowering/word_passes/rearmed-marker.1");

const LINKED_MARKER: &str =
    include_str!("../../../../mant-engine/tests/roff_lowering/word_passes/linked-marker.1");

const SURVIVING_SEPARATOR: &str =
    include_str!("../../../../mant-engine/tests/roff_lowering/word_passes/surviving-separator.1");

const PASS_LEADING_SPACE: &str = include_str!(
    "../../../../mant-engine/tests/roff_lowering/word_passes/leading-space-after-pass-3.1"
);

const LINK_SPACES: [&str; 3] = [
    include_str!(
        "../../../../mant-engine/tests/roff_lowering/word_projection/link-source-spaces-1.1"
    ),
    include_str!(
        "../../../../mant-engine/tests/roff_lowering/word_projection/link-source-spaces-2.1"
    ),
    include_str!(
        "../../../../mant-engine/tests/roff_lowering/word_projection/link-source-spaces-3.1"
    ),
];

#[test]
fn accepted_words_and_delayed_hard_rows_survive_two_cli_markdown_readbacks() {
    // All three exact .1 sources ran the five pristine profiles. The
    // formatter's accepted ranges and hard rows come from term.c::encode1(),
    // term_fill() and term_flushln(). The addressable subset here is the
    // DESCRIPTION body. Earlier raw HTML anchors have their own read contract.
    for (source, expected, label) in [
        (WORD, "ALPHAX  BETA\nOMEGA", None),
        (HARD_ROW, "ALPHA\nXBETA\nOMEGA", None),
        (
            UNICODE_LINK,
            "ALPHAX  界e\u{301}字: https://x.io\nOMEGA",
            Some(" 界e\u{301}字"),
        ),
    ] {
        assert_readbacks(source, expected, label);
    }
}

#[test]
fn overwritten_cells_and_printed_separators_survive_cli_markdown_readbacks() {
    // The five pristine profiles establish the exact rows. term_field()
    // advances only before a printable cell, including a backspace.
    for (source, expected) in [
        (BACKLINE, "ALPHAXBETA\nOMEGA"),
        (UNPRINTED, "ALPHAX\nBETA\nOMEGA"),
        (REARM, "ALPHAX  ETA\nOMEGA"),
    ] {
        assert_readbacks(source, expected, None);
    }
}

#[test]
fn marker_passes_keep_only_printed_separators_through_cli_readbacks() {
    // The pristine terminal profiles distinguish the authored post-break
    // space from the retired automatic space (term.c:205-220,389-427).
    // A real printed separator still survives the single-marker control.
    for (source, expected, label) in [
        (DEFERRED_MARKER, "ALPHAX\n BETA\nOMEGA", None),
        (REARMED_MARKER, "ALPHAX\nYBETA\nOMEGA", None),
        (
            LINKED_MARKER,
            "ALPHAX\nYBETA:\nhttps://x.io\nOMEGA",
            Some("\nYBETA"),
        ),
        (SURVIVING_SEPARATOR, "ALPHAX BETA\nOMEGA", None),
        (PASS_LEADING_SPACE, "ALPHAX\n\n YBETA", None),
    ] {
        assert_readbacks(source, expected, label);
    }
}

#[test]
fn printed_link_spaces_retain_their_owner_on_both_cli_markdown_readbacks() {
    // term_word() adds one separator before the label and encode1() may
    // retract a raw blank. For one raw blank the equivalent separator
    // remains in the label. Additional raw blanks retain their ownership.
    let word = "界e\u{301}字";
    for (index, source) in LINK_SPACES.into_iter().enumerate() {
        let expected = format!("ALPHAX{}{word}: https://x.io\nOMEGA", " ".repeat(index + 1));
        let label = format!("{}{word}", " ".repeat(index.max(1)));
        assert_readbacks(source, &expected, Some(&label));
    }
    assert_readbacks(LINK_REARM, "ALPHAX ETA: https://x.io\nOMEGA", Some("ETA"));
}

#[test]
fn linked_annotations_consume_the_same_native_cell_as_other_ignored_escapes() {
    // Fixed ASCII and UTF-8 agree: ESCAPE_IGNORE writes one ASCII_NBRZW
    // (term.c::term_word). Rewinding that cell leaves a nonprinting field
    // and rejects the tail. The URL annotation does not add another cell.
    let header = ".TH CHECK 1 \"November 19, 2026\"\n.SH NAME\ncheck \\- test\n.SH DESCRIPTION\n";
    for (control, close) in [
        (r"\X'tty: link'", ""),
        (r"\X'tty: link https://x.io'", r"\X'tty: link'"),
        (r"\&", ""),
        (r"\m[red]", ""),
    ] {
        for rewind in ["", r"\h'-1n'", r"\h'-2n'"] {
            let accepted = rewind.is_empty();
            let word = format!(r"\l'1n.'\h'-1n'{control}{rewind}\p XYZ{close}");
            let source = format!("{header}{word}\nTAILWORD\n.PP\nENDTEXT\n");
            let label = (!close.is_empty()).then_some(if accepted { "\nXYZ" } else { "" });
            if accepted {
                assert_readbacks(&source, "\nXYZ TAILWORD\n\nENDTEXT", label);
            } else {
                let plain = render(&source, "roff", "text", "never", false);
                let ansi = render(&source, "roff", "text", "always", false);
                assert_eq!(description(&plain), "\n\nENDTEXT", "{source}");
                assert_eq!(without_sgr(&ansi), plain, "{source}");
                if let Some(label) = label {
                    assert_link(&source, "roff", label);
                }
                // Both native blanks are layout spacing in the absence of
                // accepted words. Markdown projects that gap as one row.
                for addressable in [false, true] {
                    let mut markdown = render(&source, "roff", "markdown", "never", addressable);
                    for _ in 0..2 {
                        let readback = render(&markdown, "markdown", "text", "never", false);
                        assert_eq!(description(&readback), "\nENDTEXT", "{source}");
                        if let Some(label) = label {
                            assert_link(&markdown, "markdown", label);
                        }
                        markdown = render(&markdown, "markdown", "markdown", "never", addressable);
                    }
                }
            }

            let result = run_text_input(
                &[
                    "--input",
                    "-",
                    "--input-format",
                    "roff",
                    "--format",
                    "json",
                    "--display",
                    "direct",
                    "--word",
                    "--search",
                    "XYZ",
                ],
                &source,
            );
            assert!(result.status.success(), "{result:?}");
            let response: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(response["total"], usize::from(accepted), "{source}");
        }
    }
}

#[test]
fn covered_tty_labels_leave_native_hard_lines_outside_the_link() {
    // term.c::term_field emits the hard line before the next pass writes
    // X, backspace, and YTAIL. The accepted replacement owns the glyph.
    let source = r#".TH CHECK 1 "November 19, 2026"
.SH NAME
check \- test
.SH DESCRIPTION
Q\o'ab'\p LEFT\X'tty: link https://x.io'\p \zX\X'tty: link'YTAIL
TAILWORD
.PP
STOP
"#;
    assert_readbacks(source, "Qb\nLEFT\nYTAIL TAILWORD\n\nSTOP", None);
    assert_eq!(document_links(source, "roff"), []);
    for addressable in [false, true] {
        let markdown = render(source, "roff", "markdown", "never", addressable);
        assert_eq!(document_links(&markdown, "markdown"), []);
    }
}

#[test]
fn generated_option_dashes_follow_the_same_native_writes_as_authored_dashes() {
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test page\n.Sh DESCRIPTION\nSTARTPROBE\n.br\n";
    // termp_fl_pre performs term_word("\\-") in bold. The two terminal
    // bytes carrying its single glyph tolerate one or two overwrites.
    for movement in [r"\h'|0n'", r"\h'-3n'"] {
        for (markers, expected) in [
            (0, ["STARTPROBE", "-", "STOPPROBE"].as_slice()),
            (1, ["STARTPROBE", "-", "STOPPROBE"].as_slice()),
            (2, ["STARTPROBE", "-", "STOPPROBE"].as_slice()),
            (3, ["STARTPROBE", "", "STOPPROBE"].as_slice()),
        ] {
            let breaks = r"\p".repeat(markers);
            for option in ["Fl", "Sy -"] {
                let source = format!(
                    "{header}.Dl {option} Ns No \"{movement}{breaks}\"\n.br\nSTOPPROBE\n.Sh ENDPROBE\nDone.\n"
                );
                let markdown = render(&source, "roff", "markdown", "never", false);
                for text in [
                    render(&source, "roff", "text", "never", false),
                    render(&markdown, "markdown", "text", "never", false),
                ] {
                    let (rows, _) = description(&text).split_once("\n\nENDPROBE").unwrap();
                    let rows: Vec<_> = rows.lines().map(str::trim).collect();
                    assert_eq!(rows, expected, "{source}");
                }
            }
        }
    }
}

#[test]
fn pending_native_writes_define_only_the_final_label() {
    // term_word overwrites B and retires the following slots before the
    // list HEAD is flushed. The definition spans the surviving A and T.
    for list in ["tag", "hang"] {
        for (head, name, parts_count, rejected) in [
            (r#"No "A\zB\pC" No "\h'-2n'T""#, "AT", 2, "ABC T"),
            (r#"No "P\p" Ns No "\pA" No "\h'-4n'T""#, "PTA", 3, "PA T"),
            (r#"No "P\p" Ns No "\pA" No "\h'-5n'T""#, "TA", 2, "PA T"),
        ] {
            let source =
                format!("{EDITABLE_FIELD}.Bl -{list} -width 12n\n.It {head}\nBODYWORD\n.El\n");
            let json = |query: &[&str]| {
                let mut args = vec!["--input", "-", "--input-format", "roff", "--format", "json"];
                args.extend_from_slice(query);
                let result = run_text_input(&args, &source);
                assert!(result.status.success(), "{source}: {result:?}");
                serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap()
            };
            let doc = json(&[]);
            let item = &doc["document"]["sections"][1]["blocks"][0]["items"][0];
            let entry = &item["entry"];
            assert_eq!(entry["names"], serde_json::json!([name]), "{item}");
            let parts = &entry["nameBindings"][0]["occurrences"][0]["parts"];
            assert_eq!(parts.as_array().unwrap().len(), parts_count, "{item}");
            assert!(
                item["description"].to_string().contains("BODYWORD"),
                "{item}"
            );
            for (query, count) in [(name, 1), (rejected, 0)] {
                assert_eq!(
                    json(&["--explain", query])["counts"]["directEntry"]["total"],
                    count
                );
            }
        }
    }
}

#[test]
fn generated_closers_retire_covered_option_names_before_explain() {
    let closing = r#".Bq Cm v No "\o''""#;
    let source = format!("{FIELD_PROBE}STARTFLAG\n.br\n{closing}\n.br\nENDFLAG\n");
    assert_readbacks(&source, "STARTFLAG\n[]\nENDFLAG", None);
    // termp_quote_post writes its tight delimiter through term_word.
    // Only a complete surviving option spelling can own a definition.
    for holder in ["Op", "Bq"] {
        for option in ["a", "ab"] {
            for (tail, survives) in [
                ("", true),
                (r#" No "\o''""#, false),
                (r#" No "\o' '""#, true),
            ] {
                let source = format!(
                    "{FIELD_PROBE}.Bl -tag -width Ds\n.It {holder} Fl {option}{tail}\nXBODY\n.El\n"
                );
                let query = format!("--explain=-{option}");
                let result = run_text_input(
                    &[
                        "--input",
                        "-",
                        "--input-format",
                        "roff",
                        "--format",
                        "json",
                        "--display",
                        "direct",
                        &query,
                    ],
                    &source,
                );
                assert!(result.status.success(), "{source}: {result:?}");
                let facts: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
                assert_eq!(
                    facts["counts"]["directEntry"]["total"],
                    u64::from(survives),
                    "{source}"
                );
            }
        }
    }
}
