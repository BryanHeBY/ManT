//! RR01: semantic wrapper entry is an ownership checkpoint, never a word.
//!
//! Every expectation below was frozen from the pinned reference binary
//! (`target/mandoc-migration/reference/mandoc`, oracle identity
//! `cvs-20260927T130954Z-linux-x86_64-gcc-16.2.1-reproducible-20260930`)
//! with `-Tutf8 -Owidth=78` and `-Tlint` before being asserted. The two
//! main cases are lint-clean; Xr probes may report a missing installed
//! manual, which does not change their formatter or ownership path:
//!
//! * `termp_lk_pre` (mdoc_term.c:1881-1912) only `term_fontpush`es before
//!   its first real operand, then runs `term_word()` per description word,
//!   the generated `:`, and the URI. Wrapper entry executes no word.
//! * `term_word` (term.c:559-600) is the only place inter-word blanks are
//!   buffered, NOSPACE/KEEP consumed, and `skipvsp` cleared.
//! * `encode1` (term.c:886-930) makes a pending `\z` glyph retreat over
//!   the next word's separator blank (`col--`) or strike a non-blank cell.
//! * `term_flushln` (term.c:96-253) re-arms `breakline` from a `\p` marker
//!   byte that a following pass still reads; a pass with `nbr == 0` after
//!   it resets the buffer (term.c:233-237), killing the whole suffix.
//!
//! The pre-fix failure modes (RR01): the wrapper-entry pre-execution
//! consumed the first operand's separator (A0080 lost the pending `P` in
//! IR truncation) and resolved the glyph ahead of the operand's own
//! marker-blank decision (A0212 revived the rejected suffix).

use super::*;

const MDOC: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
const MAN: &str = ".TH TEST 1 \"September 28, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n";

fn description_inlines(source: &str) -> Vec<Inline> {
    let document = parse_manual_bytes(
        std::path::Path::new("semantic-checkpoint.1"),
        source.as_bytes(),
    )
    .expect("parse semantic checkpoint fixture");
    document.sections[1]
        .blocks
        .iter()
        .flat_map(|block| match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                children.clone()
            }
            _ => Vec::new(),
        })
        .collect()
}

/// Text of every node matching the predicate, in document order.
fn matching_text(nodes: &[Inline], matches: impl Fn(&Inline) -> bool + Copy) -> Vec<String> {
    let mut found = Vec::new();
    for node in nodes {
        if matches(node) {
            found.push(inline_text(std::slice::from_ref(node)));
        }
        match node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::PortableDisplay { children, .. }
            | Inline::Link { children, .. } => {
                found.extend(matching_text(children, matches));
            }
            _ => {}
        }
    }
    found
}

/// Oracle DESCRIPTION: `     P_\bY` — one row, `P` outside the underline.
#[test]
fn pending_glyph_survives_into_a_rejected_label_prefix() {
    let output = description_inlines(&format!(
        "{MDOC}.No \"\\zP\"\n.Lk https://ex.org \"Y\\p\\& \\p Z\"\n.No AFTER\n.Sh NEXT\n.No END\n"
    ));
    assert_eq!(
        inline_text(&output).trim_end_matches('\n'),
        "PY",
        "{output:#?}"
    );
    // P belongs to the preceding plain word: it stays a top-level text
    // node before the link, never inside it (oracle underlines Y only).
    assert!(
        output
            .iter()
            .any(|node| matches!(node, Inline::Text { value } if value == "P")),
        "{output:#?}"
    );
    let labels = matching_text(&output, |node| matches!(node, Inline::Link { .. }));
    assert_eq!(labels, ["Y"], "{output:#?}");
}

/// Oracle DESCRIPTION: `     P` — the whole rejected suffix stays dead and
/// the typed link identity survives with no visible activation range.
#[test]
fn marker_blank_label_dies_whole_without_reviving_the_suffix() {
    let output = description_inlines(&format!(
        "{MDOC}.No \"\\zP\\z\"\n.Lk https://ex.org \"\\p  Y\"\n.No AFTER\n.Sh NEXT\n.No END\n"
    ));
    assert_eq!(
        inline_text(&output).trim_end_matches('\n'),
        "P",
        "{output:#?}"
    );
    let text = inline_text(&output);
    assert!(!text.contains(':'), "{output:#?}");
    assert!(!text.contains("ex.org"), "{output:#?}");
    assert!(!text.contains("AFTER"), "{output:#?}");
    // The authored destination is identity data and outlives its rejected
    // label: an empty typed link node remains, with no clickable glyph.
    assert!(
        output.iter().any(|node| matches!(
            node,
            Inline::Link {
                target: mant_ir::LinkTarget::External { uri },
                children,
                ..
            } if uri == "https://ex.org" && children.is_empty()
        )),
        "{output:#?}"
    );
}

/// W04: each of the seven semantic wrappers, with no pending glyph, a
/// pending `\zP`, and a second bare `\z` after it. The pending glyph keeps
/// its owner and stays outside the wrapper's annotation; the armed glyph
/// overstrikes the wrapper's first real graph exactly as the reference
/// device prints it (encode1's `\b` over a non-blank cell).
#[test]
fn seven_wrappers_execute_real_words_with_pending_glyphs() {
    // (wrapper line, expected visible DESCRIPTION rows per prefix state
    // [none, `\zP`, `\zP\z`])
    let mdoc_wrappers = [
        (
            ".Lk https://ex.org LABEL",
            [
                "LABEL: https://ex.org AFTER",
                "PLABEL: https://ex.org AFTER",
                "PABEL: https://ex.org AFTER",
            ],
        ),
        (
            ".Mt x@example.org",
            [
                "x@example.org AFTER",
                "Px@example.org AFTER",
                "P@example.org AFTER",
            ],
        ),
        (
            ".Sx TARGET",
            ["TARGET AFTER", "PTARGET AFTER", "PARGET AFTER"],
        ),
        (
            ".In stdio.h",
            ["<stdio.h> AFTER", "P<stdio.h> AFTER", "Pstdio.h> AFTER"],
        ),
        (".Bx 4.4", ["4.4BSD AFTER", "P4.4BSD AFTER", "P.4BSD AFTER"]),
        (
            ".Xr printf 3",
            ["printf(3) AFTER", "Pprintf(3) AFTER", "Printf(3) AFTER"],
        ),
    ];
    for (line, expected) in mdoc_wrappers {
        for (prefix, expected) in ["", "\\zP", "\\zP\\z"].iter().zip(expected) {
            let source = format!(
                "{MDOC}{}{line}\n.No AFTER\n.Sh NEXT\n.No END\n.Sh TARGET\n.No TGT\n",
                if prefix.is_empty() {
                    String::new()
                } else {
                    format!(".No \"{prefix}\"\n")
                }
            );
            let output = description_inlines(&source);
            assert_eq!(
                inline_text(&output).trim_end_matches('\n'),
                expected,
                "{line} after {prefix:?}: {output:#?}"
            );
            // The delayed glyph never joins the wrapper's annotation: the
            // leading P stays in plain source text outside every Link the
            // wrapper projected.
            assert!(
                !matching_text(&output, |node| matches!(node, Inline::Link { .. }))
                    .iter()
                    .any(|text| text.starts_with('P')),
                "{line} after {prefix:?} captured the glyph: {output:#?}"
            );
        }
    }
    // MR is the man dialect form of Xr.
    for (prefix, expected) in [
        ("", "printf(3) AFTER"),
        ("\\zP", "Pprintf(3) AFTER"),
        ("\\zP\\z", "Printf(3) AFTER"),
    ] {
        let source = format!(
            "{MAN}{}.MR printf 3\n.B AFTER\n",
            if prefix.is_empty() {
                String::new()
            } else {
                format!(".B \"{prefix}\"\n")
            }
        );
        let output = description_inlines(&source);
        assert_eq!(
            inline_text(&output).trim_end_matches('\n'),
            expected,
            "{output:#?}"
        );
        assert!(
            !matching_text(&output, |node| matches!(node, Inline::Link { .. }))
                .iter()
                .any(|text| text.starts_with('P')),
            "MR after {prefix:?} captured the glyph: {output:#?}"
        );
    }
}

/// W05: the pending glyph keeps its own source, style, and link identity
/// across the next wrapper: it never inherits the wrapper's annotation,
/// font run, or clickable range (the old owner's styling survives).
#[test]
fn pending_glyph_keeps_its_own_identity_across_wrappers() {
    let carriers = [
        (".No \"\\zP\"", None),
        (".Em \"\\zP\"", Some("emphasis")),
        (".Sy \"\\zP\"", Some("strong")),
    ];
    for (carrier, styled) in carriers {
        for wrapper in [
            ".In stdio.h\n.No AFTER",
            ".Xr printf 3\n.No AFTER",
            ".Lk https://new.example NEWLBL\n.No AFTER",
        ] {
            let source = format!("{MDOC}{carrier}\n{wrapper}\n");
            let output = description_inlines(&source);
            match styled {
                None => assert!(
                    output
                        .iter()
                        .any(|node| matches!(node, Inline::Text { value } if value == "P")),
                    "{carrier} then {wrapper}: {output:#?}"
                ),
                Some(kind) => assert!(
                    output.iter().any(|node| match (kind, node) {
                        ("emphasis", Inline::Emphasis { children }) => {
                            inline_text(children) == "P"
                        }
                        ("strong", Inline::Strong { children }) => inline_text(children) == "P",
                        _ => false,
                    }),
                    "{carrier} then {wrapper} lost its {kind} run: {output:#?}"
                ),
            }
            let captured = matching_text(&output, |node| {
                matches!(node, Inline::Link { .. }) || matches!(node, Inline::Code { .. })
            });
            assert!(
                captured.iter().all(|text| !text.starts_with('P')),
                "{carrier} then {wrapper} captured the glyph: {output:#?}"
            );
        }
    }
}

/// W06/W07 minimal representatives: the incoming separator of the
/// wrapper's first real operand executes exactly once, and control-shaped
/// operands stay control events (no phantom word at wrapper entry).
#[test]
fn wrapper_separator_and_control_operands_execute_once() {
    // A `\c` continuation joins the wrapper without an extra boundary; a
    // bare `\z` and an escaped blank `\&` prefix contribute no glyph.
    // (Every row below is the reference device's, -Owidth=78.)
    for (source_suffix, expected) in [
        (
            ".No \"BEFORE\\c\"\n.Lk https://ex.org LABEL\n.No AFTER\n",
            "BEFORELABEL: https://ex.org AFTER",
        ),
        (
            ".No \"BEFORE\\c\"\n.In stdio.h\n.No AFTER\n",
            "BEFORE<stdio.h> AFTER",
        ),
        (
            ".No \"BEFORE\\c\"\n.Xr printf 3\n.No AFTER\n",
            "BEFOREprintf(3) AFTER",
        ),
        (
            ".No \"BEFORE\\c\"\n.Bx 4.4\n.No AFTER\n",
            "BEFORE4.4BSD AFTER",
        ),
        (
            ".No \"\\z\"\n.Lk https://ex.org LABEL\n.No AFTER\n",
            "ABEL: https://ex.org AFTER",
        ),
        (".No \"\\&\"\n.In stdio.h\n.No AFTER\n", "<stdio.h> AFTER"),
    ] {
        let output = description_inlines(&format!("{MDOC}{source_suffix}"));
        assert_eq!(
            inline_text(&output).trim_end_matches('\n'),
            expected,
            "{source_suffix}: {output:#?}"
        );
    }
}

/// Cached glyph annotation comes from its source word, not the visible size
/// of the next wrapper. Exact inputs were run through pinned CVS first:
/// `termp_under_pre/term_word/encode1` preserve the arm-time font; Mt/Sx's
/// HTML handlers annotate the original operand, independently of its delay
/// on the terminal device. In particular P stays in the *old* typed link.
#[test]
fn pending_glyph_retains_the_authored_annotation_without_new_click_ranges() {
    for (carrier, prefix, old_link) in [
        (".Mt \"old@example.org\\zP\"", "old@example.orgP", true),
        (".Sx \"OLD\\zP\"", "OLDP", true),
        (".Li \"\\zP\"", "P", false),
        (".Sy \"\\zP\"", "P", false),
        (".Em \"\\zP\"", "P", false),
    ] {
        for (wrapper, tail) in [
            (".No NEXT", "NEXT AFTER"),
            (".In stdio.h", "<stdio.h> AFTER"),
            (".Xr printf 3", "printf(3) AFTER"),
            (
                ".Lk https://new.example NEW",
                "NEW: https://new.example AFTER",
            ),
        ] {
            let source = format!(
                "{MDOC}{carrier}\n{wrapper}\n.No AFTER\n.Sh NEXT\n.No END\n.Sh \"OLD\\zP\"\n.No OLD_TARGET\n"
            );
            let output = description_inlines(&source);
            assert_eq!(
                inline_text(&output).trim_end_matches('\n'),
                format!("{prefix}{tail}"),
                "{carrier} then {wrapper}: {output:#?}"
            );
            let labels = matching_text(&output, |node| matches!(node, Inline::Link { .. }));
            if old_link {
                assert_eq!(
                    labels.iter().filter(|label| *label == prefix).count(),
                    1,
                    "the old link must contain P exactly once: {output:#?}"
                );
            }
            assert!(
                labels.iter().all(|label| !label.starts_with('P')),
                "the new annotation captured P: {output:#?}"
            );
            if carrier.starts_with(".Li") {
                assert!(
                    matching_text(&output, |node| matches!(node, Inline::Code { .. }))
                        .iter()
                        .any(|text| text == "P"),
                    "{output:#?}"
                );
            }
        }
    }
}

/// Recovery coverage, deliberately separate from legal Xr/MR arity above:
/// CVS diagnoses the absent section but still executes the empty name word
/// (`termp_xr_pre`), leaving the previous P visible and outside the new link.
#[test]
fn empty_recovered_manual_reference_cannot_capture_the_preceding_glyph() {
    for carrier in [".No \"\\zP\"", ".Li \"\\zP\"", ".Em \"\\zP\""] {
        for operand in ["\\&", "\"\""] {
            let source = format!("{MDOC}{carrier}\n.Xr {operand}\n.No AFTER\n.Sh NEXT\n.No END\n");
            let output = description_inlines(&source);
            assert_eq!(inline_text(&output), "P AFTER", "{output:#?}");
            assert!(
                matching_text(&output, |node| matches!(node, Inline::Link { .. }))
                    .iter()
                    .all(String::is_empty),
                "{output:#?}"
            );
        }
    }
}

/// The seam for one delayed tail must never merge the next authored Mt
/// occurrence just because its destination is the same. The exact pristine
/// runs have three and two separate addresses respectively; the first owns P.
#[test]
fn delayed_link_tail_rejoins_only_its_authored_occurrence() {
    for (middle, expected, occurrences) in [
        (
            ".Mt old@example.org",
            "old@example.orgPold@example.org old@example.org AFTER",
            3,
        ),
        (".No NEXT", "old@example.orgPNEXT old@example.org AFTER", 2),
    ] {
        let source = format!(
            "{MDOC}.Mt \"old@example.org\\zP\"\n{middle}\n.Mt old@example.org\n.No AFTER\n.Sh NEXT\n.No END\n"
        );
        let output = description_inlines(&source);
        assert_eq!(inline_text(&output), expected, "{output:#?}");
        let labels = matching_text(&output, |node| matches!(node, Inline::Link { .. }));
        assert_eq!(labels.len(), occurrences, "{output:#?}");
        assert_eq!(labels[0], "old@example.orgP", "{output:#?}");
        assert!(
            labels[1..].iter().all(|label| label == "old@example.org"),
            "{output:#?}"
        );
    }
}

/// Two source Sx occurrences remain two links even when each has only a
/// delayed glyph and no annotation prefix to rejoin. Pinned CVS lint=0;
/// `termp_under_pre/term_word` produce both glyphs independently, and each
/// `mdoc_sx_pre` opens a distinct anchor. No target-based coalescing is valid.
#[test]
fn pending_only_section_links_keep_independent_occurrences() {
    for (middle, expected) in [
        ("", "PPNEXT"),
        (".No \"\"\n", "P PNEXT"),
        (".br\n", "P\nPNEXT"),
    ] {
        let source = format!(
            "{MDOC}.Sx \"\\zP\"\n{middle}.Sx \"\\zP\"\n.No NEXT\n.Sh NEXT\n.No END\n.Sh \"\\zP\"\n.No OLD_TARGET\n"
        );
        let output = description_inlines(&source);
        assert_eq!(inline_text(&output), expected, "{output:#?}");
        let labels = matching_text(&output, |node| matches!(node, Inline::Link { .. }));
        assert_eq!(labels, ["P", "P"], "{output:#?}");
    }
}

/// BACKBEFORE and BACKAFTER coexist after `\zP\z`: the completed P
/// survives the marker's blank, while the second arm still affects the next
/// graph. CVS `term_word` buffers an automatic separator independently of
/// both flags; `encode1` retreats over the marker's author blank instead
/// (term.c:573-589,901-927). All fifteen exact inputs were run through the
/// pristine ASCII/UTF-8/HTML/lint oracle before these assertions. Xr's
/// missing installed manual is a host STYLE diagnostic; the other inputs
/// are lint-clean. Internal hard rows remain significant here.
#[test]
fn marker_blank_keeps_the_word_separator_with_a_buffered_and_armed_glyph() {
    for (wrapper, completed, completed_and_armed) in [
        (".Sx \"\\p Y\"", "P Y\nAFTER", "P YAFTER"),
        (".Xr \"\\p Y\" 3", "P Y(3)\nAFTER", "P (3)\nAFTER"),
        (".Mt \"\\p Y\"", "P Y\nAFTER", "P YAFTER"),
        (
            ".Lk https://ex.org \"\\p Y\"",
            "P Y:\nhttps://ex.org AFTER",
            "P :\nhttps://ex.org AFTER",
        ),
        (".Bx \"\\p Y\"", "P YBSD\nAFTER", "P BSD\nAFTER"),
    ] {
        for (prefix, expected) in [
            ("\\zP", completed),
            ("\\zP\\z", completed_and_armed),
            ("\\z", ""),
        ] {
            let source =
                format!("{MDOC}.No \"{prefix}\"\n{wrapper}\n.No AFTER\n.Sh NEXT\n.No END\n");
            let output = description_inlines(&source);
            assert_eq!(
                inline_text(&output).trim_end_matches('\n'),
                expected,
                "{prefix} then {wrapper}: {output:#?}"
            );
        }
    }
}
