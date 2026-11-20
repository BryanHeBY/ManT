//! Source word ownership survives the public text and Markdown entry points.

use mant_ir::visit::{self, Visit};
use mant_ir::{Inline, LinkTarget, inline_plain_text};

use super::execution_scopes::without_sgr;
use super::process_support::run_text_input;

const WORD: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_receipts/accepted-word.1");
const HARD_ROW: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_receipts/late-hard-row.1");
const UNICODE_LINK: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_receipts/accepted-unicode-link.1");
const BACKLINE: &str = include_str!(
    "../../../mant-engine/tests/roff_lowering/word_projection/backline-after-zero-width.1"
);
const UNPRINTED: &str = include_str!(
    "../../../mant-engine/tests/roff_lowering/word_projection/source-prefix-cross-pass.1"
);
const REARM: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_projection/rearm-prefix-space.1");
const LINK_REARM: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_projection/link-rearm.1");
const DEFERRED_MARKER: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_passes/deferred-marker.1");
const REARMED_MARKER: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_passes/rearmed-marker.1");
const LINKED_MARKER: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_passes/linked-marker.1");
const SURVIVING_SEPARATOR: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_passes/surviving-separator.1");
const PASS_LEADING_SPACE: &str = include_str!(
    "../../../mant-engine/tests/roff_lowering/word_passes/leading-space-after-pass-3.1"
);
const EMPTY_AT_EOF: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-at-eof.1");
const EMPTY_BEFORE_PP: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-before-pp.1");
const EMPTY_LINK_BEFORE_PP: &str = include_str!(
    "../../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-link-before-pp.1"
);
const EMPTY_BEFORE_SPACE: &str = include_str!(
    "../../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-before-space.1"
);
const LINK_SPACES: [&str; 3] = [
    include_str!("../../../mant-engine/tests/roff_lowering/word_projection/link-source-spaces-1.1"),
    include_str!("../../../mant-engine/tests/roff_lowering/word_projection/link-source-spaces-2.1"),
    include_str!("../../../mant-engine/tests/roff_lowering/word_projection/link-source-spaces-3.1"),
];

#[derive(Default)]
struct Links(Vec<(LinkTarget, String)>);

impl<'a> Visit<'a> for Links {
    fn visit_inline(&mut self, inline: &'a Inline) {
        if let Inline::Link {
            target, children, ..
        } = inline
        {
            self.0.push((target.clone(), inline_plain_text(children)));
        }
        visit::walk_inline(self, inline);
    }
}

fn render(source: &str, input: &str, format: &str, color: &str, addressable: bool) -> String {
    let mut args = vec![
        "--input",
        "-",
        "--input-format",
        input,
        "--format",
        format,
        "--display",
        "direct",
        "--color",
        color,
    ];
    if addressable {
        args.push("--preserve-anchors");
    }
    let result = run_text_input(&args, source);
    assert!(result.status.success(), "{result:?}");
    String::from_utf8(result.stdout).unwrap()
}

fn description(text: &str) -> &str {
    let (_, body) = text.split_once("\nDESCRIPTION\n").unwrap();
    body.strip_suffix('\n').unwrap_or(body)
}

fn document_links(source: &str, input: &str) -> Vec<(LinkTarget, String)> {
    let json = render(source, input, "json", "never", false);
    let bundle: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let mut links = Links::default();
    for section in &bundle.document.unwrap().sections {
        links.visit_section(section);
    }
    links.0
}

fn assert_link(source: &str, input: &str, label: &str) {
    assert_eq!(
        document_links(source, input),
        [(
            LinkTarget::External {
                uri: "https://x.io".into(),
            },
            label.into(),
        )],
    );
}

fn assert_readbacks(source: &str, expected: &str, label: Option<&str>) {
    assert_readback_modes(source, expected, label, &[false, true]);
}

fn assert_readback_modes(source: &str, expected: &str, label: Option<&str>, modes: &[bool]) {
    let plain = render(source, "roff", "text", "never", false);
    let ansi = render(source, "roff", "text", "always", false);
    assert_eq!(description(&plain), expected, "{source}");
    assert_eq!(without_sgr(&ansi), plain, "{source}");
    if let Some(label) = label {
        assert_link(source, "roff", label);
    }
    for &addressable in modes {
        let mut input = source.to_owned();
        let mut input_format = "roff";
        for round in 1..=2 {
            input = render(&input, input_format, "markdown", "never", addressable);
            input_format = "markdown";
            let text = render(&input, input_format, "text", "never", false);
            assert_eq!(
                description(&text),
                expected,
                "addressable={addressable}, round {round}: {input:?}"
            );
            if let Some(label) = label {
                assert_link(&input, input_format, label);
            }
        }
    }
}

#[test]
fn later_native_writes_resolve_backlines_without_losing_source_owners() {
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
    // term_field() skips NBRZW after BS. A following word can print a
    // blank or glyph in the old column; term_newln() alone cannot.
    for overprint in [r"P\o'CD'", r"P\zCD"] {
        for (tail, result) in [
            (" No Y", "P Y"),
            (" Ns No Y", "PY"),
            ("", "PC"),
            ("\n.br\n.No Y", "PC\nY"),
        ] {
            let source = format!("{header}.No \"{overprint}\" Ns No \"\\h'-1n'\\&\"{tail}\n");
            assert_readbacks(&source, result, None);
            assert_eq!(
                search_visible_word(&source, "PC"),
                u64::from(result.starts_with("PC")),
                "{source}"
            );
            if matches!(result, "P Y" | "PY") {
                assert_eq!(search_visible_word(&source, result), 1, "{source}");
            }
        }
    }
    // encode1() writes both author cells and a real BS. Replacing BS
    // reveals M, while replacing A leaves M's overstrike active.
    for (word, retreat, result, label) in [
        (r"\zMA", 2, "T", None),
        (r"\zMA", 3, "MTA", None),
        (
            r"\zM\X'tty: link https://x.io'A\X'tty: link'",
            4,
            "MTA",
            Some("A"),
        ),
    ] {
        let source = format!("{header}.No \"{word}\" No \"\\h'-{retreat}n'T\"\n");
        assert_readbacks(&source, result, label);
        assert_eq!(search_visible_word(&source, result), 1, "{source}");
    }
    // The two native bold paints are one linked author, even when the
    // neighboring invisible link cells are included in the raw replay.
    let source = format!(
        "{header}.Sy \"\\X'tty: link https://x.io'M\\X'tty: link'\" No \"\\h'-1n'T\" No \"\\h'-2n'Y\"\n"
    );
    assert_readbacks(&source, "MY", Some("M"));
    let ansi = render(&source, "roff", "text", "always", false);
    assert!(
        ansi.lines().last().unwrap().starts_with("\u{1b}[1m"),
        "{ansi:?}"
    );
    assert_eq!(search_visible_word(&source, "MY"), 1);
}

#[test]
fn replay_retires_or_moves_the_exact_gap_and_retains_surviving_bold_copies() {
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
    // term_word() traverses preserved cells, but term_field() prints a
    // surviving advance where it actually lands, including after a rewind.
    for (word, expected) in [
        (r#".No PMAB No "\h'-2n'T" No Y No "\h'-3n'Z""#, "PMATZY"),
        (r#".No PMAB No "\h'-2n'T" No Y No "\h'-5n'Z""#, "PMZT Y"),
        (r#".No PMAB No "\h'-2n'T" No Y Ns No "\h'-3n'Z""#, "PMAZ Y"),
        (r#".No "P MA B" No "\h'-2n'T" No "\h'-3n'Y""#, "P MAYT"),
    ] {
        let source = format!("{header}{word}\n");
        assert_readbacks(&source, expected, None);
        assert_eq!(search_visible_word(&source, expected), 1, "{source}");
    }
    // term.c::encode1() writes both bold copies. When the original
    // projection cell is replaced, its earlier copy can still own ink.
    let word = r#".Sy "P\X'tty: link https://x.io'M\X'tty: link'AB""#;
    for (tail, result, label) in [
        (r#"No "\h'-9n'T" No "\h'-6n'Y""#, "YTAB", ""),
        (r#"Ns No "\h'-7n'T" Ns No "\h'-6n'Y""#, "YMTAB", "M"),
        (r#"Ns No "\h'-9n'T" Ns No "\h'-4n'Y""#, "YMTAB", "M"),
    ] {
        let source = format!("{header}{word} {tail}\n");
        let plain = render(&source, "roff", "text", "never", false);
        assert_eq!(description(&plain), result, "{source}");
        assert_link(&source, "roff", label);
        // Addressable export writes the implicit Sy tag as raw anchor HTML,
        // which the documented Markdown reimport contract retains as HTML.
        let markdown = render(&source, "roff", "markdown", "never", false);
        assert_eq!(
            description(&render(&markdown, "markdown", "text", "never", false)),
            result,
            "{markdown}"
        );
        assert_link(&markdown, "markdown", label);
        assert_eq!(search_visible_word(&source, result), 1, "{source}");
    }
}

#[test]
fn combining_overstrike_uses_the_accepted_field_to_keep_or_cover_its_base() {
    let header = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    // Fixed CVS UTF-8 writes the base, a backspace and the non-advancing
    // accent (term.c::encode1 and term_field). The portable reading stream
    // keeps their source order. A later cell on the same native pass can
    // cover the base, while a subsequent physical line cannot.
    for (input, expected) in [
        (".No \"ALPHA\\ze\u{301}\"", "ALPHAe\u{301}"),
        (r#".No "ALPHA\ze\[u0301]""#, "ALPHAe\u{301}"),
        (".No \"ALPHA\\ze\u{301}\u{300}\"", "ALPHAe\u{301}\u{300}"),
        (".No \"ALPHA\\ze\u{301}TAIL\"", "ALPHA\u{301}TAIL"),
        (r#".No "ALPHA\ze\[u0301]TAIL""#, "ALPHA\u{301}TAIL"),
        (".No \"ALPHA\\ze\\z\u{301}TAIL\"", "ALPHA\u{301}TAIL"),
        (".No \"ALPHA\\ze\u{301}\" Ns No TAIL", "ALPHA\u{301}TAIL"),
        (
            ".No \"ALPHA\\ze\u{301}\"\n.No \"TAIL\"",
            "ALPHA\u{301} TAIL",
        ),
        (".No \"ALPHA\\ze \u{301}TAIL\"", "ALPHAe\u{301}TAIL"),
        (
            ".No \"ALPHA\\ze\u{301}\"\n.br\n.No TAIL",
            "ALPHAe\u{301}\nTAIL",
        ),
    ] {
        assert_readbacks(&format!("{header}{input}\n"), expected, None);
    }

    let styled = format!("{header}.No \"ALPHA\\fB\\ze\\fI\u{301}\"\n");
    let bundle: mant_protocol::QueryBundle =
        serde_json::from_str(&render(&styled, "roff", "json", "never", false)).unwrap();
    let document = bundle.document.unwrap();
    let mant_ir::Block::Paragraph { children, .. } = &document.sections.last().unwrap().blocks[0]
    else {
        panic!("expected the combining glyph's paragraph");
    };
    let [
        Inline::Text { value },
        Inline::Strong { children: base },
        Inline::Emphasis { children: mark },
    ] = children.as_slice()
    else {
        panic!("expected separate base and mark styles: {children:?}");
    };
    assert_eq!(value, "ALPHA");
    assert_eq!(inline_plain_text(base), "e");
    assert_eq!(inline_plain_text(mark), "\u{301}");
}

#[test]
fn rejected_kept_suffix_does_not_repeat_its_accepted_prefix_endline() {
    // Exact rejected, accepted and unkept controls ran fixed CVS in all
    // five profiles. term.c::term_flushln() ends the accepted field at
    // the first breakpoint, then nbr=0 ends just one empty row. The
    // invisible graph accepted by the first field cannot end another row.
    let rejected = r#".Dd November 10, 2026
.Dt CHECK 1
.Os
.Sh NAME
.Nm check
.Nd word audit
.Sh DESCRIPTION
.Bk -words
.No "ALPHA\zX"
.No "\&\p\: \p\:\&\p\: \zY\p BETA"
.br
.No OMEGA
.Ek
"#;
    let accepted = rejected.replacen(r"\p\:\&", r"\&", 1);
    let unkept = rejected.replace(".Bk -words\n", "").replace(".Ek\n", "");
    for (source, expected) in [
        (rejected, "ALPHAX\n\nOMEGA"),
        (&accepted, "ALPHAX\n\nYBETA\nOMEGA"),
        (&unkept, "ALPHAX\n\nOMEGA"),
    ] {
        assert_readbacks(source, expected, None);
    }
}

#[test]
fn man_link_annotation_keeps_the_native_boundary_between_literal_rows() {
    let header = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- word audit\n.SH DESCRIPTION\n.nf";
    // Fixed CVS accepted the first label row and the subsequent empty pass
    // but rejected the rest of that buffer, including the post_UR() target.
    // term.c::term_flushln() cannot revoke the committed first label row.
    let rejected_word = r"\&\p\: \p BETA";
    for (open, close, address, target) in [
        (
            "UR",
            "UE",
            "https://x.io",
            LinkTarget::External {
                uri: "https://x.io".into(),
            },
        ),
        (
            "MT",
            "ME",
            "u@matrix.test",
            LinkTarget::Email {
                address: "u@matrix.test".into(),
            },
        ),
    ] {
        let rejected = format!(
            "{header}\n.{open} {address}\n.B \"ALPHA\\zX\\p\\: \"\n.B \"{rejected_word}\"\n.{close}\n.fi\n.br\nOMEGA\n"
        );
        let accepted = rejected.replace(rejected_word, "BETA");
        let accepted_text = format!("ALPHAX\nBETA <{address}>\nOMEGA");
        for (source, expected, label) in [
            (&rejected, "ALPHAX\n\n\nOMEGA", "ALPHAX\n\n"),
            (&accepted, accepted_text.as_str(), "ALPHAX\nBETA"),
        ] {
            assert_readbacks(source, expected, None);
            assert_eq!(
                document_links(source, "roff"),
                [(target.clone(), label.into())],
                "{source}"
            );
        }
    }
}

#[test]
fn leading_horizontal_motion_changes_native_acceptance_without_replaying_padding() {
    // Each source was run through the fixed CVS. term.c:678–714 makes
    // HORIZ clear or rewind BACKBEFORE before term_fill() accepts cells.
    let heading = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    for (first, second, expected) in [
        ("ALPHA", r"\h'-1n'BETA", "ALPHABETA\nOMEGA"),
        (
            r"ALPHA\p\:\zX",
            r"\h'-1n'\&\p\: \p BETA",
            "ALPHA\nX\n\nOMEGA",
        ),
        (
            r"ALPHA\zX\p\: ",
            r"\h'-1n'\&\p\: \p BETA",
            "ALPHAX\n\n\nOMEGA",
        ),
        (r"ALPHA\zX", r"\h'1n'\z \p BETA", "ALPHAX\n\nOMEGA"),
    ] {
        let source =
            format!("{heading}.No \"{first}\"\n.No \"{second}\"\n.No \"\"\n.br\n.No OMEGA\n");
        assert_readbacks(&source, expected, None);
    }
}

#[test]
fn negative_word_entry_preserves_each_surviving_cell_and_owner() {
    // These exact inputs were run with the pinned CVS. term.c:701–714
    // rewinds without shortening lastcol; an SP advances across the old
    // cells without overwriting them (term.c:948–952). Markers can replace
    // the tab reference or the live BACKBEFORE glyph before term_fill().
    let heading = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    for (word, expected, label) in [
        (r#".No "\h'-1n'   BETA""#, "ALPHAX  BETA\nOMEGA", None),
        (r#".No "\h'-2n'   BETA""#, "ALPHAX BETA\nOMEGA", None),
        (r#".No "\h'-1n'    BETA""#, "ALPHAX   BETA\nOMEGA", None),
        (r#".No "\h'-2n' BETA""#, "ALPHAXBETA\nOMEGA", None),
        (r#".No "\h'-1n'\p BETA""#, "ALPHAX\nBETA\nOMEGA", None),
        (r#".No "\h'-2n'\p BETA""#, "ALPHABETA\nOMEGA", None),
        (r#".No "\h'-2n'\: BETA""#, "ALPHABETA\nOMEGA", None),
        (
            r#".No "\h'-1n'\p\:    BETA""#,
            "ALPHAX\n    BETA\nOMEGA",
            None,
        ),
        (r#".No "\h'-24u'\p BETA""#, "ALPHAX\nBETA\nOMEGA", None),
        (r#".No "\h'-25u'\p BETA""#, "ALPHAX\nBETA\nOMEGA", None),
        (r#".No "\h'-47u'\p BETA""#, "ALPHAX\nBETA\nOMEGA", None),
        (r#".No "\h'-48u'\p\: BETA""#, "ALPHA\n BETA\nOMEGA", None),
        (r#".Em "\h'-2n'\p\: BETA""#, "ALPHA\n BETA\nOMEGA", None),
        (
            r#".Lk https://x.io "\h'-2n'\p\: BETA""#,
            "ALPHA\n BETA: https://x.io\nOMEGA",
            Some("BETA"),
        ),
        // Zero distance does not take the negative HORIZ branch. The
        // unchanged marker position rejects BETA in the actual pass.
        (r#".No "\h'0n'\p\: BETA""#, "ALPHAX\n\nOMEGA", None),
    ] {
        let source = format!("{heading}.No \"ALPHA\\zX\"\n{word}\n.No \"\"\n.br\n.No OMEGA\n");
        assert_readbacks(&source, expected, label);
    }
}

#[test]
fn a_proved_word_entry_rewinds_ordinary_glyphs_before_accepting_the_break() {
    // term.c::term_word() rewinds the live field through ordinary A and
    // writes the marker before term_fill(). Both fixed devices accept
    // ALPH and BETA on separate rows before the following OMEGA.
    let source = r#".Dd November 10, 2026
.Dt CHECK 1
.Os
.Sh NAME
.Nm check
.Nd word audit
.Sh DESCRIPTION
.No "ALPHA\zX"
.No "\h'-3n'\p\: BETA"
.No ""
.br
.No OMEGA
"#;
    assert_readbacks(source, "ALPH\nBETA\nOMEGA", None);
}

#[test]
fn same_operand_motion_uses_the_written_slots_before_its_marker() {
    // CVS ASCII and UTF-8 agree. term.c:678–714 evaluates both forms of HORIZ
    // from col; term_fill() receives the marker after that retreat.
    let heading = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    for (distance, expected) in [
        ("-1n", "ALPHA\n BETA\nOMEGA"),
        ("-2n", "ALPH\n BETA\nOMEGA"),
        ("0n", "ALPHAX\n\nOMEGA"),
        ("1n", "ALPHAX\n\nOMEGA"),
        ("|6n", "ALPHA\n BETA\nOMEGA"),
    ] {
        let inline =
            format!("{heading}.No \"ALPHA\\zX \\h'{distance}'\\p\\: BETA\"\n.br\n.No OMEGA\n");
        assert_readbacks(&inline, expected, None);
    }
}

#[test]
fn drawing_word_uses_the_native_field_lifetime() {
    let heading = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    let source = format!(
        "{heading}.No \"ALPHA\\zX\"\n.br\n.No \"P\\l'1n '\\h'0n'\\h'|5n'\\p NEEDLE\"\n.No \"\"\n.br\n.No OMEGA\n"
    );
    // Both fixed CVS devices output these four rows. br retires the old
    // field, so the following absolute HORIZ starts from a fresh cursor.
    assert_readbacks(&source, "ALPHAX\nP\nNEEDLE\nOMEGA", None);

    // Without br, both devices still print NEEDLE. The motion can address
    // the earlier word, so an incomplete geometry receipt cannot reject it.
    let live = source.replacen("\n.br\n", "\n", 1);
    assert!(
        description(&render(&live, "roff", "text", "never", false)).contains("NEEDLE"),
        "{live}"
    );
}

#[test]
fn local_slot_edits_keep_their_effect_after_a_prior_word() {
    // Pinned term.c:678–714 and 790–796. Here the retreat and overstrike trim
    // affect only bytes of the arriving word, even with an occupied field.
    let heading = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test page\n.Sh DESCRIPTION\n";
    for (word, expected) in [
        (r"Q\p \o' '\p XYZ", "BEFORE Q\nXYZ LAST\nEND"),
        (r"\l'1n.'\h'-1n'\p XYZ", "BEFORE\n\nEND"),
    ] {
        let source = format!("{heading}.No BEFORE\n.No \"{word}\"\n.No LAST\n.br\n.No END\n");
        assert_readbacks(&source, expected, None);
    }
}

#[test]
fn negative_word_entry_keeps_the_exact_prior_scalar_and_backafter_order() {
    // Pinned term.c:678–681 cancels an armed BACKAFTER without moving
    // the cursor. With BACKBEFORE alone, only its final X can be
    // replaced; the identically spelled earlier X keeps its own style.
    let heading = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    for (first, second, expected) in [
        (r"ALPHA\zX", r"\h'-2n'\p\: BETA", "ALPHA\n BETA\nOMEGA"),
        (r"ALPHAX\zX", r"\h'-2n'\p\: BETA", "ALPHAX\n BETA\nOMEGA"),
        (
            r"ALPHA\fBX\fR\zX",
            r"\h'-2n'\p\: BETA",
            "ALPHAX\n BETA\nOMEGA",
        ),
        (r"ALPHA\zX\z", r"\h'-1n'\:\p BETA", "ALPHAX BETA\nOMEGA"),
        (r"ALPHA\z ", r"\h'-47u' BETA", "ALPHA   BETA\nOMEGA"),
    ] {
        let source = format!("{heading}.No \"{first}\"\n.No \"{second}\"\n.br\n.No OMEGA\n");
        assert_readbacks(&source, expected, None);
    }
}

#[test]
fn negative_word_entry_places_crossed_spaces_at_their_source_position() {
    // Fixed CVS term.c:948–952 lets an internal source SP cross TABREF
    // (no visible contribution) or the automatic SP (represented once
    // at its new position). encode1() writes style backspaces before the
    // graph, so in the link case it also replaces the old padding.
    let heading = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    for (word, expected, label) in [
        (r#".No "\h'-1n'B ETA""#, "ALPHAXB ETA\nOMEGA", None),
        (r#".No "\h'-2n'B ETA""#, "ALPHABETA\nOMEGA", None),
        (r#".Em "\h'-2n'BE TA""#, "ALPHABE TA\nOMEGA", None),
        (r#".No "\h'-2n'B  ETA""#, "ALPHAB ETA\nOMEGA", None),
        (r#".No "\h'-2n'B   ETA""#, "ALPHAB  ETA\nOMEGA", None),
        (
            r#".Lk https://x.io "\h'-1n'B ETA""#,
            "ALPHAXB ETA: https://x.io\nOMEGA",
            Some("B ETA"),
        ),
        (
            r#".Lk https://x.io "\h'-2n'B ETA""#,
            "ALPHAB ETA: https://x.io\nOMEGA",
            Some("B ETA"),
        ),
        (r#".No "\h'-2n'\p\zY  BETA""#, "ALPHAY\nBETA\nOMEGA", None),
    ] {
        let source = format!("{heading}.No \"ALPHA\\zX\"\n{word}\n.br\n.No OMEGA\n");
        assert_readbacks(&source, expected, label);
    }
}

#[test]
fn rewound_literal_spaces_keep_existing_cells_before_word_end_breaks() {
    // Fixed CVS ran these exact words with ASCII, UTF-8, HTML, tree and
    // lint. term.c::encode() advances across old cells for raw SP but
    // keeps their bytes. The next marker can replace the automatic blank
    // or a tight word's still-pending X. Only ETA prints after rearming.
    let source = r#".Dd November 10, 2026
.Dt CHECK 1
.Os
.Sh NAME
.Nm check
.Nd word audit
.Sh DESCRIPTION
.No "ALPHA\zX"
.No "\h'-1n'\z \p BETA"
.No ""
.br
.No OMEGA
"#;
    let word = r"\h'-1n'\z \p BETA";
    let same_line = source.replace("\"\n.No \"\\h", "\" No \"\\h");
    let joined = source.replace("\"\n.No \"\\h", "\" Ns No \"\\h");
    let styled = source.replace(".No \"\\h", ".Em \"\\h");
    for (input, expected) in [
        (source.to_owned(), "ALPHAX\nETA\nOMEGA"),
        (same_line, "ALPHAX\nETA\nOMEGA"),
        (joined, "ALPHA\nETA\nOMEGA"),
        (styled, "ALPHAX\nETA\nOMEGA"),
        (
            source.replace(word, r"\h'-1n' \p BETA"),
            "ALPHAX\nBETA\nOMEGA",
        ),
        (
            source.replace(word, "\\h'-1n'\\z\t\\p BETA"),
            "ALPHAX\nETA\nOMEGA",
        ),
        (
            source.replace(word, r"\h'-1n'\z  \p BETA"),
            "ALPHAX\n\nOMEGA",
        ),
        (source.replace(r"\h'-1n'", ""), "ALPHAX  ETA\nOMEGA"),
        (
            source.replace("\n.No \"\"\n.br\n.No OMEGA", ""),
            "ALPHAX\nETA",
        ),
    ] {
        assert_readbacks(&input, expected, None);
    }
    // The tight rewind can remove the pending combining mark while
    // leaving its preceding native backline. A graph on that same cell
    // covers the base only when it prints on the same accepted pass.
    let combining = source
        .replace("\"\n.No \"\\h", "\" Ns No \"\\h")
        .replace(r"ALPHA\zX", "ALPHA\\ze\\z\u{301}");
    for (input, expected) in [
        (combining.clone(), "ALPHAe\nETA\nOMEGA"),
        (combining.replace(r"\z \p BETA", " B"), "ALPHAB\nOMEGA"),
        (
            combining.replace(r"\z \p BETA", r" B\p TAIL"),
            "ALPHAB\nTAIL\nOMEGA",
        ),
        (combining.replace(word, " B"), "ALPHA\u{301}B\nOMEGA"),
    ] {
        assert_readbacks(&input, expected, None);
    }
}

#[test]
fn body_flush_returns_only_printed_blanks_to_a_detached_linked_term() {
    // Both exact inputs ran fixed CVS. mdoc_term.c::termp_it_post() leaves
    // inset HEAD cells live until the BODY flush; term_field() advances
    // authored spaces only when the accepted pass prints them.
    let source = r#".Dd November 10, 2026
.Dt CHECK 1
.Os
.Sh NAME
.Nm check
.Nd word audit
.Sh DESCRIPTION
.Bl -inset -compact
.It Xo
.No "ALPHA\zX"
.Lk https://x.io "\&\p\: \zY\p \&\p\: \p BETA"
.No ""
.Xc
.br
.No OMEGA
.El
"#;
    let overwritten = source.replace(r"\&\p\: \p BETA", r"\&\p\:\fB \p\fIBETA");
    for (input, word) in [(source, " BETA"), (&overwritten, "BETA")] {
        let label = format!("\nY\n\n{word}");
        assert_link(input, "roff", &label);
        let direct = render(input, "roff", "text", "never", false);
        assert_eq!(
            description(&direct),
            format!("ALPHAX\nY\n\n{word}:\nhttps://x.io\nOMEGA")
        );
        assert_eq!(
            without_sgr(&render(input, "roff", "text", "always", false)),
            direct
        );
        for addressable in [false, true] {
            let markdown = render(input, "roff", "markdown", "never", addressable);
            assert_link(&markdown, "markdown", &label);
            let reexport = render(&markdown, "markdown", "markdown", "never", addressable);
            assert_link(&reexport, "markdown", &label);
        }
    }
}

#[test]
fn a_detached_head_does_not_replay_its_endline_in_a_rejecting_body_flush() {
    // The exact sources ran through fixed CVS. mdoc_term.c::termp_it_post()
    // leaves the inset HEAD's cells pending. The first body flush accepts
    // HEAD X, rejects Y, and leaves one actual empty row; a positive sp
    // adds its own row. The accepted HEAD endline closes the term once.
    let source = r#".Dd September 28, 2026
.Dt TEST 1
.Os
.Sh NAME
.Nm test
.Nd probe
.Sh DESCRIPTION
.Bl -inset
.It Xo
.No "X\p \p Y"
.Xc
.br
.No BodyWord
.El
"#;
    for (input, expected, markdown_expected) in [
        (source.to_owned(), "X\n\nBodyWord", "• X\n\n  BodyWord"),
        (
            source.replace(".br\n", ".sp 0\n"),
            "X\n\nBodyWord",
            "• X\n\n  BodyWord",
        ),
        (
            source.replace(".br\n", ".sp 1\n"),
            "X\n\n\nBodyWord",
            "• X\n\n  BodyWord",
        ),
        (
            source.replace(".No \"X", ".Em \"X"),
            "X\n\nBodyWord",
            "• X\n\n  BodyWord",
        ),
        (source.replace(".br\n", ""), "X\n", "• X\n"),
    ] {
        let plain = render(&input, "roff", "text", "never", false);
        assert_eq!(description(&plain), expected, "{input}");
        assert_eq!(
            without_sgr(&render(&input, "roff", "text", "always", false)),
            plain
        );
        let wire = render(&input, "roff", "json", "never", false);
        let bundle: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let document = bundle.document.unwrap();
        let [mant_ir::Block::DefinitionList { items, .. }] = document.sections[1].blocks.as_slice()
        else {
            panic!("expected one definition after JSON transport");
        };
        assert_eq!(inline_plain_text(&items[0].terms[0].content), "X");

        // Raw addressable HTML is outside the semantic reimport subset.
        let mut markdown = render(&input, "roff", "markdown", "never", false);
        for _ in 0..2 {
            let readback = render(&markdown, "markdown", "text", "never", false);
            assert_eq!(description(&readback), markdown_expected, "{markdown}");
            markdown = render(&markdown, "markdown", "markdown", "never", false);
        }
    }
}

#[test]
fn accepted_empty_word_passes_keep_hard_rows_before_a_successor_or_eof() {
    // These exact inputs ran all five pristine profiles. term.c:205–220
    // closes each accepted loop pass, including NBRZW with no printed
    // scalar. The last rejected pass also ends its row (250–253). The
    // following .Pp independently executes term_vspace().
    let before_break = include_str!(
        "../../../mant-engine/tests/roff_lowering/word_passes/empty-passes-before-break.1"
    );
    let at_eof =
        include_str!("../../../mant-engine/tests/roff_lowering/word_passes/empty-passes-at-eof.1");
    let kept =
        include_str!("../../../mant-engine/tests/roff_lowering/word_passes/empty-passes-in-keep.1");
    let literal_at_eof = at_eof.replace(".Sh DESCRIPTION\n", ".Sh DESCRIPTION\n.nf\n") + ".fi\n";
    let before_paragraph = before_break.replace(".br\n", ".Pp\n");
    let one_less_pass = before_break.replacen("\\&\\p\\: ", "", 1);
    for (source, expected) in [
        (before_break, "ALPHAX\n\n\nOMEGA"),
        (at_eof, "ALPHAX\nY\n\n\n"),
        (&literal_at_eof, "ALPHAX\nY\n\n\n"),
        (&before_paragraph, "ALPHAX\n\n\n\nOMEGA"),
        (&one_less_pass, "ALPHAX\n\nOMEGA"),
        (kept, "ALPHA\n\n\nOMEGA"),
    ] {
        assert_readbacks(source, expected, None);
    }
}

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
fn deferred_source_space_survives_only_in_the_printed_native_pass() {
    // The pinned terminal prints the SP following an accepted breakpoint.
    // In the negative control encode1() emits a backspace instead of
    // retreating over an immediately preceding SP (term.c:901-908).
    for (source, expected, label) in [
        (
            include_str!(
                "../../../mant-engine/tests/roff_lowering/word_passes/deferred-source-blank.1"
            ),
            "ALPHAX\nY\n\n BETA\nOMEGA",
            None,
        ),
        (
            include_str!(
                "../../../mant-engine/tests/roff_lowering/word_passes/deferred-source-blank-in-style.1"
            ),
            "ALPHAX\nY\n\n BETA\nOMEGA",
            None,
        ),
        (
            include_str!(
                "../../../mant-engine/tests/roff_lowering/word_passes/deferred-source-blank-in-link.1"
            ),
            "ALPHAX\nY\n\n BETA:\nhttps://x.io\nOMEGA",
            Some("\nY\n\n BETA"),
        ),
        (
            include_str!(
                "../../../mant-engine/tests/roff_lowering/word_passes/overwritten-source-blank.1"
            ),
            "ALPHAX\n\nBETA\nOMEGA",
            None,
        ),
    ] {
        assert_readbacks(source, expected, label);
    }
}

#[test]
fn rejected_plain_tail_and_positive_spacing_keep_independent_rows() {
    // In pinned term.c:143-146,205-220,250-253, the flush closes its
    // accepted pass and then its empty rejected tail. A following .Pp
    // independently runs term_vspace() via mdoc_term.c::termp_pp_pre().
    for (source, expected, label) in [
        (EMPTY_AT_EOF, "ALPHAX\n", None),
        (EMPTY_BEFORE_PP, "ALPHAX\n\n\nOMEGA", None),
        (EMPTY_LINK_BEFORE_PP, "ALPHAX\n\n\nOMEGA", Some("")),
    ] {
        assert_readbacks(source, expected, label);
    }

    // Markdown can simplify a positive .sp 2 to one layout row. The
    // preceding hard empty row must survive that simplification.
    assert_eq!(
        description(&render(EMPTY_BEFORE_SPACE, "roff", "text", "never", false)),
        "ALPHAX\n\n\n\nOMEGA"
    );
    let json = render(EMPTY_BEFORE_SPACE, "roff", "json", "never", false);
    let bundle: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let document = bundle.document.unwrap();
    let section = document.sections.last().unwrap();
    let [
        mant_ir::Block::Paragraph { children, .. },
        mant_ir::Block::VerticalSpace { lines: 2, .. },
        mant_ir::Block::Paragraph { .. },
    ] = section.blocks.as_slice()
    else {
        panic!("expected hard rows and independent .sp 2: {section:?}");
    };
    assert_eq!(inline_plain_text(children), "ALPHAX\n\n");
    for addressable in [false, true] {
        let mut input = render(EMPTY_BEFORE_SPACE, "roff", "markdown", "never", addressable);
        for _ in 0..2 {
            assert_eq!(
                description(&render(&input, "markdown", "text", "never", false)),
                "ALPHAX\n\n\nOMEGA"
            );
            input = render(&input, "markdown", "markdown", "never", addressable);
        }
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
fn trimming_the_native_field_can_reveal_an_earlier_overstrike_glyph() {
    // term.c::term_word trims raw slots. term_field writes the earlier
    // graph and its backspace even when the graph that covered it is gone.
    let header = ".TH CHECK 1 \"November 19, 2026\"\n.SH NAME\ncheck \\- test\n.SH DESCRIPTION\n";
    for (word, text, label) in [
        (r"HEAD\o'AB'\o' '", "HEADA", None),
        (
            r"HEAD\X'tty: link https://x.io'\o'AB'\o' '\X'tty: link'",
            "HEADA",
            Some("A"),
        ),
        (
            r"HEAD\X'tty: link https://x.io'\o'AB'\X'tty: link'\o' '",
            "HEADB",
            Some("B"),
        ),
        (
            r"HEAD\X'tty: link https://x.io'\o'AB'\o' 'C\X'tty: link'",
            "HEADC",
            Some("C"),
        ),
        (concat!(r"HEAD\o'AB'\o' '\c", "\nZ"), "HEADZ", None),
        (
            concat!(
                r"HEAD\X'tty: link https://x.io'\o'AB'\o' '\X'tty: link'\c",
                "\nZ",
            ),
            "HEADZ",
            None,
        ),
    ] {
        let source = format!("{header}{word}\n.br\nENDTEXT\n");
        assert_readbacks(&source, &format!("{text}\nENDTEXT"), label);
        if label.is_none() {
            assert_eq!(document_links(&source, "roff"), []);
        }
    }
}

#[test]
fn later_operands_can_expose_or_trim_author_slots_from_the_active_field() {
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n.ll 140n\n";
    // In the first case the final operand rewinds over the drawing and
    // leaves PREX followed by a terminal backspace at the end of the pass.
    // In the second case each blank overstrike trims an earlier letter.
    for (head, first) in [
        (
            r#".No "PRE\zX" Ns No "\l'1n.'" Ns No "\h'-1n'\p NEEDLE""#,
            "PREX",
        ),
        (r#".No "PRE" Ns No "\o' '" Ns No "\o' '\p NEEDLE""#, "P"),
    ] {
        let source = format!("{header}{head}\nTAILWORD\n.Pp\nENDTEXT\n");
        assert_readbacks(
            &source,
            &format!("{first}\nNEEDLE TAILWORD\n\nENDTEXT"),
            None,
        );
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
                first,
            ],
            &source,
        );
        assert!(result.status.success(), "{head}: {result:?}");
        let response: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(response["total"], 1, "{head}");
    }
}

#[test]
fn bold_device_copies_keep_one_author_until_the_accepted_pass_covers_it() {
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n.ll 140n\n";
    // Fixed ASCII and UTF-8 exercise term.c::encode1()'s earlier bold
    // copy and term_field()'s delayed blank. A blank is only printed
    // over the earlier copy when another device write follows on that pass.
    for font in ["B", "[BI]"] {
        for (after_head, tail, expected) in [
            (
                r#"Ns No "\o' '" No "\l'1n.'\p NEEDLE""#,
                "\nTAILWORD",
                "PR \nNEEDLE TAILWORD\n\nENDTEXT",
            ),
            (
                r#"No "\o' '" No "\l'1n.'\p NEEDLE""#,
                "\nTAILWORD",
                "PRE \nNEEDLE TAILWORD\n\nENDTEXT",
            ),
            (
                r#"No "\o' '" Ns No "\h'-1n'\p NEEDLE""#,
                "\nTAILWORD",
                "PRE\nNEEDLE TAILWORD\n\nENDTEXT",
            ),
            (
                r#"No "\o' '" Ns No "\o' '\p NEEDLE""#,
                "\nTAILWORD",
                "PRE\nNEEDLE TAILWORD\n\nENDTEXT",
            ),
            (r#"Ns No "\o' '" No "\p""#, "", "PRE\n\nENDTEXT"),
        ] {
            let source =
                format!("{header}.No \"\\f{font}PRE\\fR\" {after_head}{tail}\n.Pp\nENDTEXT\n");
            assert_readbacks(&source, expected, None);
        }
    }
}

#[test]
fn native_overstrike_keeps_the_style_and_link_of_a_surviving_bold_copy() {
    let header =
        ".TH SAMPLE 1 \"November 19, 2026\"\n.SH NAME\nsample \\- sample\n.SH DESCRIPTION\n";
    // CVS encode1 emits the first bold copy before the backline and the
    // final copy. A later draw can replace the final copy without replacing
    // the earlier one, whether this author was literal or inside \o.
    for (word, expected, label) in [
        (r"P\fB\o'A'\fR\h'-2n'XYZ", "PAXYZ", None),
        (r"P\f[BI]\o'A'\fR\h'-2n'XYZ", "PAXYZ", None),
        (r"P\fBA\fR\o' '\o' 'Z", "PAZ", None),
        (
            r"P\X'tty: link https://x.io'\fB\o'A'\fR\X'tty: link'\h'-3n'XYZ",
            "PAXYZ",
            Some("A"),
        ),
    ] {
        let source = format!("{header}{word}\n.br\nENDTEXT\n");
        assert_readbacks(&source, &format!("{expected}\nENDTEXT"), label);
        if label.is_none() {
            let ansi = render(&source, "roff", "text", "always", false);
            let bold_copy = if word.contains("[BI]") {
                "\x1b[1m\x1b[3mA\x1b[0m"
            } else {
                "\x1b[1mA\x1b[0m"
            };
            assert!(ansi.contains(bold_copy), "{source}");
        }
        assert_eq!(search_visible_word(&source, expected), 1, "{source}");
    }
}

#[test]
fn empty_overstrike_uses_the_live_entry_slots_across_words() {
    let header = ".Dd November 19, 2026\n.Dt SAMPLE 1\n.Os\n.Sh NAME\n.Nm sample\n.Nd sample\n.Sh DESCRIPTION\n";
    // term_word trims the native buffer after every overstrike. A surviving
    // device space can belong to earlier operands without either current
    // automatic entry spacing or authored output for that cell.
    for (word, expected) in [
        (r#".No P No "\o' '" No "\o' '\o' 'Z""#, "P Z"),
        (r#".No P No "\o' '" No "\o' 'Z""#, "PZ"),
        (r#".No P No "\o' '" No Z"#, "P Z"),
        (r#".No P Ns No "\o' '" Ns No Z"#, "P Z"),
        (r#".No "P\zQ" No "\o''" No Z"#, "PZ"),
        (r#".No "P\zQ" Ns No "\o''" Ns No Z"#, "PZ"),
        (r#".No "P\zQ" Ns No "\o''" Ns No "\o' 'Z""#, "PQZ"),
        (r#".No "P\fBA\fR" No "\:" No "\o' 'Z""#, "PA Z"),
        (r#".No "P\fBA\fR" No "\&" No "\o' '\o' 'Z""#, "PA Z"),
    ] {
        let source = format!("{header}STARTFLAG\n.br\n{word}\n.br\nENDMARK\n");
        assert_readbacks(&source, &format!("STARTFLAG\n{expected}\nENDMARK"), None);
        assert_eq!(
            search_visible_word(&source, "PZ"),
            u64::from(expected == "PZ"),
            "{source}"
        );
    }
}

fn search_visible_word(source: &str, word: &str) -> u64 {
    search_visible_word_with_format(source, "roff", word)
}

fn search_visible_word_with_format(source: &str, format: &str, word: &str) -> u64 {
    let result = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            format,
            "--format",
            "json",
            "--display",
            "direct",
            "--word",
            "--search",
            word,
        ],
        source,
    );
    assert!(result.status.success(), "{result:?}");
    let response: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    response["total"].as_u64().unwrap()
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

const FIELD_PROBE: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample manual\n.Sh DESCRIPTION\n";

#[test]
fn interword_spacing_uses_the_last_accepted_cell_before_a_pending_glyph() {
    // CVS writes an ordinary SP before \&, then an empty overstrike
    // either trims an extra separator or leaves the old SP for T to
    // overstrike. Observe both directions across native font scopes.
    for (word, expected, search, count) in [
        (r#".Sy A No "\&" No "\z\o''T""#, "A T", "AT", 0),
        (r#".No A No "\&" No "\z\o''T""#, "A T", "AT", 0),
        (r#".Ic abc No "\&" Ns No "\z\o''T""#, "abc T", "abcT", 0),
        (r#".Fl abc No "\:" No "\z\o''T""#, "-abc T", "abcT", 0),
        (r#".No "P\zQ" No "\&" Ns No "\z\o''T""#, "PQT", "PQT", 1),
        (r#".No "P\zQ" No "\:" Ns No "\o''T""#, "PQT", "PQT", 1),
    ] {
        let source = format!("{FIELD_PROBE}STARTFLAG\n.br\n{word}\n.br\nENDFLAG\n");
        let expected_rendering = format!("STARTFLAG\n{expected}\nENDFLAG");
        let plain = render(&source, "roff", "text", "never", false);
        assert_eq!(description(&plain), expected_rendering, "{source}");
        let ansi = render(&source, "roff", "text", "always", false);
        assert_eq!(without_sgr(&ansi), plain, "{source}");
        let markdown = render(&source, "roff", "markdown", "never", false);
        let roundtrip = render(&markdown, "markdown", "text", "never", false);
        assert_eq!(description(&roundtrip), expected_rendering, "{source}");
        assert_eq!(search_visible_word(&source, search), count, "{source}");
    }
}

#[test]
fn native_backspace_only_removes_a_blank_when_its_replacement_is_accepted() {
    // With BACKBEFORE, encode1 can replace the SP directly or step
    // over it after an ignored ASCII_BREAK cell. The next glyph
    // still has to be accepted by term_fill on the same pass.
    for (arm, separator, expected) in [
        ("", r"\p T", &["PREB", "T"][..]),
        ("", r" \:T", &["PREB T"][..]),
        (r"\z", r"\p T", &["PREBT"][..]),
        (r"\z", r" \:T", &["PREBT"][..]),
    ] {
        let word = format!("PRE{arm}\\o'AB'{separator}");
        for body in [
            word.clone(),
            format!(".Bd -literal -compact\n{word}\n.Ed"),
            format!(".TS\nl.\n{word}\nWWWWWWWWWWWWWWWW\n.TE"),
        ] {
            let source = format!("{FIELD_PROBE}STARTFLAG\n.br\n{body}\n.br\nENDFLAG\n");
            let markdown = render(&source, "roff", "markdown", "never", false);
            for text in [
                render(&source, "roff", "text", "never", false),
                render(&markdown, "markdown", "text", "never", false),
            ] {
                let rows: Vec<_> = description(&text).lines().map(str::trim).collect();
                let mut wanted = vec!["STARTFLAG"];
                wanted.extend(expected);
                if body.starts_with(".TS") {
                    wanted.push("WWWWWWWWWWWWWWWW");
                }
                wanted.push("ENDFLAG");
                assert_eq!(rows, wanted, "{source}");
            }
        }
    }
}

#[test]
fn relative_word_rewind_needs_no_overstrike_or_line_break_escape() {
    // term_word's relative HORIZ retreats one native slot. T overwrites
    // M both in one operand and across an independently executed No.
    for (word, expected) in [
        (r"PM\h'-1n'T", "PT"),
        (r#".No PM Ns No "\h'-1n'T""#, "PT"),
        ("PMT", "PMT"),
    ] {
        let source = format!("{FIELD_PROBE}STARTFLAG\n.br\n{word}\n.br\nENDFLAG\n");
        assert_readbacks(&source, &format!("STARTFLAG\n{expected}\nENDFLAG"), None);
        assert_eq!(
            search_visible_word(&source, "PT"),
            u64::from(expected == "PT")
        );
    }
}

const EDITABLE_FIELD: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";

#[test]
fn accepted_cells_keep_their_glyphs_across_replayed_separators() {
    // CVS term.c::term_flushln and term_field consume gaps separately
    // from visible glyphs. The last motion-only input changes col, not
    // lastcol, so it cannot erase a previously buffered row end.
    for (word, reading, hit, miss, labels) in [
        (
            r#".No "\zQ\p" Ns No A No "\pT" Ns No "\h'-2n'Y""#,
            "A\nYT",
            "YT",
            "Q",
            &[][..],
        ),
        (
            r#".No "\X'tty: link https://one.test'P\X'tty: link' \pQ" No "\h'2n'\p B""#,
            "P Q\n\nB",
            "B",
            "QB",
            &["P"][..],
        ),
        (
            r#".No "\X'tty: link https://one.test'P\X'tty: link'\p Q" No "\p " No "\h'-1n'""#,
            "P\nQ",
            "Q",
            "P Q",
            &["P"][..],
        ),
    ] {
        let source = format!("{EDITABLE_FIELD}{word}\n");
        assert_readbacks(&source, reading, None);
        for (input, format) in [
            (source.clone(), "roff"),
            (
                render(&source, "roff", "markdown", "never", false),
                "markdown",
            ),
        ] {
            let found: Vec<_> = document_links(&input, format)
                .into_iter()
                .map(|(target, label)| {
                    assert_eq!(
                        target,
                        LinkTarget::External {
                            uri: "https://one.test".into()
                        }
                    );
                    label
                })
                .collect();
            assert_eq!(found, labels, "{source}");
            assert_eq!(
                search_visible_word_with_format(&input, format, hit),
                1,
                "{source}"
            );
            assert_eq!(
                search_visible_word_with_format(&input, format, miss),
                0,
                "{source}"
            );
        }
    }
}

#[test]
fn retained_cell_receipts_survive_both_definition_owners() {
    // The It HEAD and BODY flush separately. Both accept the restored T
    // after A's word-end break (mdoc_term.c::termp_it_post, term.c::term_field).
    let word = r#"No "\zQ\p" Ns No A No "\pT" Ns No "\h'-2n'Y""#;
    for kind in ["tag", "hang"] {
        let head = format!("{EDITABLE_FIELD}.Bl -{kind} -width 12n\n.It {word}\nBODYWORD\n.El\n");
        let body = format!("{EDITABLE_FIELD}.Bl -{kind} -width 12n\n.It alpha\n.{word}\n.El\n");
        let header_tail = if kind == "tag" {
            "A\nYT\nBODYWORD"
        } else {
            "A\nYT BODYWORD"
        };
        for (source, id, expected) in [
            (&head, "term-a-yt", header_tail),
            (&body, "term-alpha", "alpha         A\nYT"),
        ] {
            let json = render(source, "roff", "json", "never", false);
            let document: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(
                document["document"]["sections"][1]["blocks"][0]["items"][0]["entry"]["id"],
                id
            );
            let reading = render(source, "roff", "text", "never", false);
            let rows: Vec<_> = description(&reading).lines().map(str::trim_start).collect();
            assert_eq!(rows, expected.lines().collect::<Vec<_>>(), "{source}");
        }
        let explanation = run_text_input(
            &[
                "--input",
                "-",
                "--input-format",
                "roff",
                "--format",
                "json",
                "--explain",
                "alpha",
            ],
            &body,
        );
        assert!(explanation.status.success(), "{explanation:?}");
        let result: serde_json::Value = serde_json::from_slice(&explanation.stdout).unwrap();
        assert_eq!(result["counts"]["directEntry"]["total"], 1);
        assert_eq!(search_visible_word(&body, "YT"), 1);
    }
}

#[test]
fn live_markers_and_backspaces_commit_the_same_text_and_link_owners() {
    // CVS term_word writes \p into the buffer. term_fill reads the
    // marker after all writes, including writes reaching a preceding word.
    for (body, rendered, visible, hidden) in [
        (
            r#".No "\zA\p\X'tty: link https://one.test'B\X'tty: link'" No "\h'-3n'T""#,
            "T",
            "T",
            "B",
        ),
        (r#".No "A\zB\pC" No "\h'-2n'T""#, "AT", "AT", "ABC"),
        (
            r#".No PA No "B\pC" No "\h'-5n'T""#,
            "PATBC",
            "PATBC",
            "PA BC",
        ),
        (
            r#".No PA No "B\zC D" No "\h'-5n'T""#,
            "PATBCD",
            "PATBCD",
            "BCD",
        ),
        (r#".No "\pAB" No T Ns No "\h'-3n'Y""#, "AY\nT", "AY", "AB"),
        (r#".No "A\pB" No T Ns No "\h'-3n'Y""#, "AY\nT", "AY", "AB"),
        (
            r#".No "A\pB\:C" No T Ns No "\h'-3n'Y""#,
            "AB\nY T",
            "AB",
            "ABC",
        ),
    ] {
        let source = format!("{EDITABLE_FIELD}{body}\n");
        assert_readbacks(&source, rendered, None);
        assert_eq!(search_visible_word(&source, visible), 1, "{source}");
        assert_eq!(search_visible_word(&source, hidden), 0, "{source}");
        assert!(
            document_links(&source, "roff")
                .iter()
                .all(|(_, text)| text.is_empty())
        );
    }
}

#[test]
fn adjacent_markers_leave_the_live_field_editable_across_word_boundaries() {
    // term_word writes both markers into one field before term_fill
    // accepts it. The terminal rewind can land in either source word.
    for (join, distance, expected) in [
        ("Ns No", 3, "PTA"),
        ("Ns No", 4, "PTA"),
        ("Ns No", 5, "TA"),
        ("No", 4, "PTA"),
        ("No", 5, "PT A"),
    ] {
        let source =
            format!(r#"{EDITABLE_FIELD}.No "P\p" {join} "\pA" No "\h'-{distance}n'T""#) + "\n";
        assert_readbacks(&source, expected, None);
        if expected != "PT A" {
            assert_eq!(search_visible_word(&source, expected), 1, "{source}");
        }
        assert_eq!(search_visible_word(&source, "PA T"), 0, "{source}");
    }
}

#[test]
fn word_end_markers_preserve_the_cells_and_owners_of_inner_separators() {
    // term_fill treats ASCII_NBRSP as a printable cell and accepts the
    // earlier breakable blank as a row end. A later term_word can replace
    // the linked glyph or the glyph after a horizontal advance.
    let link = r#".No "A\p \X'tty: link https://one.test'B\X'tty: link'" No "#;
    let overwritten = format!("{EDITABLE_FIELD}{link}\"\\h'-3n'T\"\n");
    let retained = format!("{EDITABLE_FIELD}{link}\"T\"\n");
    assert_readbacks(&overwritten, "A\nT", None);
    assert_readbacks(&retained, "A\nB T", None);
    for (source, expected) in [(&overwritten, &[][..]), (&retained, &["B"][..])] {
        let markdown = render(source, "roff", "markdown", "never", false);
        for (text, format) in [(source.as_str(), "roff"), (markdown.as_str(), "markdown")] {
            let labels: Vec<_> = document_links(text, format)
                .into_iter()
                .filter_map(|(target, label)| {
                    assert_eq!(
                        target,
                        LinkTarget::External {
                            uri: "https://one.test".into()
                        }
                    );
                    (!label.is_empty()).then_some(label)
                })
                .collect();
            assert_eq!(labels, expected, "{source}");
        }
    }
    let source = format!(r#"{EDITABLE_FIELD}.No "A\p\h'2n'B" No "\h'-2n'T""#) + "\n";
    assert_readbacks(&source, "A  T", None);
}

#[test]
fn section_transitions_publish_only_rows_completed_by_the_device() {
    // pre_SH uses TERMP_NOBREAK with trailspace equal to the field width.
    // A zero-width tail that fits never reaches endline in term_flushln.
    let head = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n";
    let tail = "\nGG\n.PD 1\n.SH ENDTEST\nZZ\n";
    for (request, id, heading, compiled_gap) in [
        ("SH", "section", r"\&", 0),
        ("SH", "p-dd", r"\p DD", 0),
        ("SH", "p-p-dd", r"\p\:\&\p DD", 0),
        ("SS", "p-dd", r"\p DD", 1),
    ] {
        for (pd, mode) in [(0, ""), (0, ".nf\n"), (1, "")] {
            let source = format!("{head}.PD {pd}\n{mode}AA\n.{request} {heading}{tail}");
            let bundle: mant_protocol::QueryBundle =
                serde_json::from_str(&render(&source, "roff", "json", "never", false)).unwrap();
            let document = bundle.document.unwrap();
            let section = if request == "SH" {
                &document.sections[2]
            } else {
                &document.sections[1].children[0]
            };
            let gap = pd + compiled_gap;
            assert_eq!(section.id.as_str(), id, "{source}");
            assert_eq!(section.heading.plain_text(), "", "{source}");
            assert_eq!(usize::from(section.spacing_before_lines), gap, "{source}");
            let output = render(&source, "roff", "text", "never", false);
            let between = output.split_once("\nFIRST\n").unwrap().1;
            let rows = between.split_once("\nENDTEST").unwrap().0.trim_end();
            let rows: Vec<_> = rows.lines().map(str::trim_start).collect();
            let expected = format!("AA\n{}GG", "\n".repeat(gap));
            assert_eq!(rows, expected.lines().collect::<Vec<_>>(), "{source}");
            let selector = format!("id:{id}");
            let excerpt = run_text_input(
                &[
                    "--input",
                    "-",
                    "--input-format",
                    "roff",
                    "--format",
                    "json",
                    "--node",
                    &selector,
                ],
                &source,
            );
            assert!(excerpt.status.success(), "{excerpt:?}");
            let excerpt: mant_protocol::QueryExcerpt =
                serde_json::from_slice(&excerpt.stdout).unwrap();
            let [
                mant_protocol::ExcerptSelection::DocumentSection {
                    section: selected, ..
                },
            ] = excerpt.selections.as_slice()
            else {
                panic!("expected one selected section for {source}");
            };
            assert_eq!(selected.id, section.id);
            assert_eq!(selected.heading, section.heading);
            assert_eq!(selected.spacing_before_lines, section.spacing_before_lines);
        }
    }
}

#[test]
fn an_inherited_marker_keeps_the_pending_glyph_before_a_discretionary_break() {
    // CVS term_fill reads both words in one field. ASCII_BREAK ends the
    // pending \p pass, while NBRZW leaves the overstrike on the same row.
    for (word, expected, link) in [
        (r#".No "\pB" No "\zD" Ns No "\:C""#, "B\nC", false),
        (
            r#".No "\pB" No "\X'tty: link https://one.test'\zD\X'tty: link'" Ns No "\:C""#,
            "B\nC",
            false,
        ),
        (r#".No "\zA\p" No "\zB\:C""#, "AB\nC", false),
        (r#".No "\zA\p" Ns No "\z\:C""#, "A\nC", false),
        (r#".No "\zA\p" Ns No "\z" Ns No "\:C""#, "A\nC", false),
        (r#".No "\zA\p" Ns No "\:C""#, "A\nC", false),
        (r#".No "\zA\p\:C""#, "A\nC", false),
        (r#".No "\zA\p" Ns No "\&C""#, "C", false),
        (r#".No "\zA\p" No "\:C""#, "A\nC", false),
        (
            r#".No "\X'tty: link https://one.test'\zA\p\X'tty: link'" Ns No "\:C""#,
            "A\nC",
            true,
        ),
        (
            r#".Sy "\X'tty: link https://one.test'\zA\p\X'tty: link'" Ns No "\:C""#,
            "A\nC",
            true,
        ),
    ] {
        let source = format!("{EDITABLE_FIELD}{word}\n");
        assert_readbacks(&source, expected, None);
        let markdown = render(&source, "roff", "markdown", "never", false);
        for (input, format) in [(source.as_str(), "roff"), (markdown.as_str(), "markdown")] {
            assert_eq!(
                search_visible_word_with_format(input, format, "A"),
                u64::from(expected.starts_with("A\n"))
            );
            assert_eq!(
                search_visible_word_with_format(input, format, "AB"),
                u64::from(expected.starts_with("AB\n"))
            );
            assert_eq!(search_visible_word_with_format(input, format, "DC"), 0);
            assert_eq!(search_visible_word_with_format(input, format, "AC"), 0);
            let targets = document_links(input, format);
            if link {
                assert_eq!(
                    targets,
                    [(
                        LinkTarget::External {
                            uri: "https://one.test".into()
                        },
                        "A".into()
                    )]
                );
            } else {
                assert_eq!(targets, Vec::<(LinkTarget, String)>::new());
            }
        }
    }
}

#[test]
fn a_breakpoint_keeps_the_pending_link_in_its_definition_owner() {
    let word = r#".No "\X'tty: link https://one.test'\zA\p\X'tty: link'" Ns No "\:C""#;
    for kind in ["tag", "hang"] {
        let head = format!(
            "{EDITABLE_FIELD}.Bl -{kind} -width 12n\n.It {}\nBODYWORD\n.El\n",
            &word[1..]
        );
        let body = format!("{EDITABLE_FIELD}.Bl -{kind} -width 12n\n.It alpha\n{word}\n.El\n");
        for (source, id, term, description) in [
            (&head, "term-a-c", "A\nC", "BODYWORD"),
            (&body, "term-alpha", "alpha", "A\nC"),
        ] {
            let bundle: mant_protocol::QueryBundle =
                serde_json::from_str(&render(source, "roff", "json", "never", false)).unwrap();
            let document = bundle.document.unwrap();
            let mant_ir::Block::DefinitionList { items, .. } = &document.sections[1].blocks[0]
            else {
                panic!("{source}");
            };
            let item = &items[0];
            let mant_ir::Block::Paragraph { children, .. } = &item.description[0] else {
                panic!("{source}");
            };
            assert_eq!(item.entry.as_ref().unwrap().id.as_str(), id, "{source}");
            assert_eq!(mant_ir::inline_plain_text(&item.terms[0]), term, "{source}");
            assert_eq!(
                mant_ir::inline_plain_text(children),
                description,
                "{source}"
            );
            assert_eq!(
                document_links(source, "roff"),
                [(
                    LinkTarget::External {
                        uri: "https://one.test".into()
                    },
                    "A".into()
                )]
            );
        }
    }
}

#[test]
fn alternating_fonts_place_a_delayed_glyph_before_the_next_owners_break() {
    // man_term.c::pre_alternate executes separate words without a space.
    // term_fill accepts both graphs before ASCII_BREAK, even though the
    // second glyph only enters the projection with the third operand.
    let head = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH DESCRIPTION\n";
    for style in ["BR", "BI"] {
        for control in ["", r"\&"] {
            let source = format!(r#"{head}.{style} "\pB" "{control}\zD" "\:C""#) + "\n";
            assert_readbacks(&source, "BD\nC", None);
            assert_eq!(search_visible_word(&source, "BD"), 1, "{source}");
            assert_eq!(search_visible_word(&source, "DC"), 0, "{source}");
        }
    }
}

#[test]
fn consecutive_and_detached_man_fields_share_the_device_minimum() {
    // term_flushln assigns minbl from trailspace even when no glyph
    // reaches term_field. An endline consumes that minimum.
    let head = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n";
    for (pd, predecessor, heading, count, gap) in [
        (0, "", r".SH \&", 1, 0),
        (0, "", r".SH \&", 2, 1),
        (0, "", r".SH \&", 3, 1),
        (0, ".IP \\&\n", r".SH \&", 1, 1),
        (0, ".TP\n\\&\n", r".SH \&", 1, 1),
        (0, ".HP\n\\&\n", r".SH \&", 1, 1),
        (1, ".IP \\&\n", r".SH \&", 1, 2),
        (0, ".sp 1\n", r".SH \&", 1, 1),
        (0, "", r".SS \~", 1, 0),
        (0, "", r".SS \0", 1, 0),
        (0, "", r".SH \0", 1, 1),
        (0, "", r".SS \p DD", 1, 1),
    ] {
        let headings = format!("{heading}\n").repeat(count);
        let source =
            format!("{head}.PD {pd}\nAA\n{predecessor}{headings}.PD 0\nDD\n.SH ENDTEST\nZZ\n");
        let full = render(&source, "roff", "text", "never", false);
        let actual = full.split_once("\nFIRST\n").unwrap().1;
        let actual = actual.split_once("\nENDTEST").unwrap().0.trim_end();
        let rows: Vec<_> = actual.lines().map(str::trim).collect();
        let expected = format!("AA\n{}DD", "\n".repeat(gap));
        assert_eq!(rows, expected.lines().collect::<Vec<_>>(), "{source}");
    }
}

#[test]
fn subsection_excerpts_keep_only_the_heading_owned_hard_rows() {
    // man_term.c::pre_SS with PD=0 leaves the old field live. A pass
    // ending the old owner's row must consume its padding without
    // appearing as a leading hard row in the new heading.
    let head = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n.PD 0\n";
    let tail = "\nGG\n.PD 1\n.SH ENDTEST\nZZ\n";
    for (word, heading, id, rows, title) in [
        (r"AA\p\~BB", "DD", "dd", "AA\u{a0}BB\nDD\nGG", "DD"),
        (r"AA\p\:BB", r"\&\p DD", "p-dd", "AA\nBB\nDD\nGG", "DD"),
        (r"AA\p BB", r"\&\p DD", "p-dd", "AA\nBB\nDD\nGG", "DD"),
        (
            r"AA\p\~BB",
            r"\&\p DD",
            "p-dd",
            "AA\u{a0}BB\n\nDD\nGG",
            "\nDD",
        ),
    ] {
        let source = format!("{head}{word}\n.SS {heading}{tail}");
        let bundle: mant_protocol::QueryBundle =
            serde_json::from_str(&render(&source, "roff", "json", "never", false)).unwrap();
        let section = &bundle.document.unwrap().sections[1].children[0];
        assert_eq!(section.heading.plain_text(), title, "{source}");
        let full = render(&source, "roff", "text", "never", false);
        let between = full.split_once("\nFIRST\n").unwrap().1;
        let body = between.split_once("\nENDTEST").unwrap().0.trim_end();
        assert_eq!(
            body.lines().map(str::trim_start).collect::<Vec<_>>(),
            rows.lines().collect::<Vec<_>>(),
            "{source}"
        );
        let selector = format!("id:{id}");
        let excerpt = run_text_input(
            &[
                "--input",
                "-",
                "--input-format",
                "roff",
                "--format",
                "text",
                "--display",
                "direct",
                "--color",
                "never",
                "--node",
                &selector,
            ],
            &source,
        );
        assert!(excerpt.status.success(), "{source}: {excerpt:?}");
        let excerpt = String::from_utf8(excerpt.stdout).unwrap();
        assert_eq!(
            excerpt.splitn(3, "\n\n").last().unwrap().trim_end(),
            format!("{title}\nGG"),
            "{source}"
        );
        if title == "DD" {
            let markdown = render(&source, "roff", "markdown", "never", false);
            assert!(markdown.lines().any(|row| row == "### DD"), "{markdown}");
        }
    }
}

#[test]
fn subsection_padding_shared_with_the_previous_row_is_not_a_heading_row() {
    // man_term.c::pre_SS and term.c::term_flushln share the still open
    // row with the heading's first accepted padding pass at PD=0.
    // A new SH executes separately, and a later blank pass remains.
    let head = ".TH TOOL 1\n.SH DESCRIPTION\n.PD 0\nAA\n";
    for (request, tail, expected) in [
        ("SS", "B", &["AA", "B", "DD"][..]),
        ("SS", r"\0", &["AA", "", "DD"][..]),
        ("SH", "B", &["AA", "", "B", "DD"][..]),
        ("SH", r"\0", &["AA", "", "", "DD"][..]),
    ] {
        let source = format!("{head}.{request} \\~\\p {tail}\nDD\n.SH ENDPROBE\nzz\n");
        let text = render(&source, "roff", "text", "never", false);
        let body = text
            .split_once("DESCRIPTION\n")
            .unwrap()
            .1
            .split_once("\nENDPROBE")
            .unwrap()
            .0;
        assert_eq!(
            body.trim_end_matches('\n')
                .lines()
                .map(str::trim)
                .collect::<Vec<_>>(),
            expected,
            "{source}"
        );
        if request == "SS" {
            let bundle: mant_protocol::QueryBundle =
                serde_json::from_str(&render(&source, "roff", "json", "never", false)).unwrap();
            let document = bundle.document.unwrap();
            let heading = &document.sections[0].children[0].heading;
            assert_eq!(
                heading.plain_text(),
                if tail == "B" { "B" } else { "\u{a0}" }
            );
        }
    }
}

#[test]
fn subsection_padding_can_share_a_row_already_presented_by_a_hanging_body() {
    // CVS post_HP(BODY) calls term_newln with NOBREAK still active.
    // A fitting field prints its cells and leaves viscol open for pre_SS.
    let head = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH CASE0CASE\n.PD 0\nAA\n.HP 4n\n\\&\n\\~\n";
    let tail = "\nDD\n.PD 0\n.SH ENDTEST\nZZ\n";
    for (heading, gaps) in [
        (r".SS \0", 1),
        (r".SS \~", 1),
        (r".SS \p \~", 1),
        (r".SS \&\p \~", 2),
        (r".SS \~\p \0", 2),
        (r".SS \~\p\:\0", 2),
        (".SS \\~\n.SH \\&", 1),
    ] {
        let source = format!("{head}{heading}{tail}");
        let text = render(&source, "roff", "text", "never", false);
        let between = text
            .split_once("CASE0CASE\n")
            .unwrap()
            .1
            .split_once("\nENDTEST")
            .unwrap()
            .0
            .trim_end_matches('\n');
        let expected = format!("AA\n{}DD", "\n".repeat(gaps));
        assert_eq!(
            between.lines().map(str::trim).collect::<Vec<_>>(),
            expected.lines().collect::<Vec<_>>(),
            "{source}"
        );
    }
}

#[test]
fn table_word_end_breaks_reach_their_accepted_interior_row() {
    // tbl_term.c::tbl_word buffers the word before term_fill. BACKBEFORE
    // consumes one SP; two SPs leave the first one to end the current pass.
    for prefix in ["", r"\z"] {
        for blanks in 0..=4 {
            for continued in [false, true] {
                let word = format!("{prefix}A\\p{}C", " ".repeat(blanks));
                let cell = if continued {
                    format!("T{{\n{word}\nT}}")
                } else {
                    word
                };
                let source = format!(
                    ".TH TOOL 1\n.SH DESCRIPTION\n.TS\nlw(20n).\n{cell}\nzzzzzzzzzzzzzzzz\n.TE\n.SH ENDPROBE\nzz\n"
                );
                let expected = match (prefix.is_empty(), blanks) {
                    (false, 0) => "C",
                    (false, 1) | (true, 0) => "AC",
                    _ => "A\nC",
                };
                let markdown = render(&source, "roff", "markdown", "never", false);
                for (input, format) in [(source.as_str(), "roff"), (markdown.as_str(), "markdown")]
                {
                    let text = render(input, format, "text", "never", false);
                    let body = text
                        .split_once("DESCRIPTION\n")
                        .unwrap()
                        .1
                        .split_once("\nENDPROBE")
                        .unwrap()
                        .0
                        .trim_end_matches('\n');
                    assert_eq!(
                        body.lines().map(str::trim).collect::<Vec<_>>(),
                        expected
                            .lines()
                            .chain(["zzzzzzzzzzzzzzzz"])
                            .collect::<Vec<_>>(),
                        "{source}"
                    );
                    assert_eq!(
                        search_visible_word_with_format(input, format, "AC"),
                        u64::from(expected == "AC")
                    );
                }
            }
        }
    }
}

#[test]
fn long_device_advances_leave_complete_words_editable_after_them() {
    // The pinned term_word() advances col independently of its later
    // term_fill()/term_field() view. These short rewinds replace B or C.
    for length in [5, 63, 64, 65, 96, 128] {
        for gap in [
            format!(r"\h'{length}n'"),
            format!(r"\h'|{}n'", length + 1),
            format!(r"\h'1n'\h'{}n'", length - 1),
            format!(r"\l'{length}n '"),
        ] {
            for (after, wanted, erased) in [("", "AX", 'B'), (r#" Ns No "DC""#, "ABDX", 'C')] {
                let source =
                    format!(r#"{EDITABLE_FIELD}.No "A{gap}B"{after} No "\h'-2n'X""#) + "\n";
                let markdown = render(&source, "roff", "markdown", "never", false);
                for text in [
                    render(&source, "roff", "text", "never", false),
                    render(&markdown, "markdown", "text", "never", false),
                ] {
                    let body = description(&text);
                    assert_eq!(
                        body.chars()
                            .filter(char::is_ascii_uppercase)
                            .collect::<String>(),
                        wanted,
                        "{source}"
                    );
                    assert!(!body.contains(erased), "{source}");
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
fn active_words_keep_raw_glyphs_and_advances_through_later_edits() {
    // Pinned term.c::term_word/encode1 keeps the word-end marker, spaces,
    // and overstrike bytes editable until term_flushln() accepts a pass.
    for (word, expected, hit, miss, label) in [
        (r#".No "P \zMA" No "\h'-3n'T""#, "P MTA", "MTA", "TA", None),
        (
            r#".No P No "\zMA" No "\h'-3n'T""#,
            "P MTA",
            "MTA",
            "TA",
            None,
        ),
        (r#".Sy "A\pB" No "\h'-2n'T""#, "AT", "AT", "AB T", None),
        (r#".No "A\h'2n'B" No "\h'-2n'T""#, "A  T", "T", "B", None),
        (r#".No "A\l'1n 'B" No "\h'-2n'T""#, "A T", "T", "B", None),
        (
            r#".Sy "P\h'2n'\X'tty: link https://x.io'M\X'tty: link'AB" No "\h'-2n'T""#,
            "P  MAT",
            "MAT",
            "MAB",
            Some("M"),
        ),
        (
            r#".Sy "P\l'1n '\X'tty: link https://x.io'M\X'tty: link'AB" No "\h'-2n'T""#,
            "P MAT",
            "MAT",
            "MAB",
            Some("M"),
        ),
        (
            r#".Sy "P\X'tty: link https://x.io'M\pA\X'tty: link'B" No "T" Ns No "\h'-3n'Y""#,
            "PMAY\nT",
            "PMAY",
            "PMAB",
            Some("MA"),
        ),
    ] {
        let source = format!("{FIELD_PROBE}{word}\n");
        // Addressable Markdown uses raw HTML anchor tags, which are
        // outside the documented semantic reimport subset.
        assert_readback_modes(&source, expected, label, &[false]);
        assert_eq!(search_visible_word(&source, hit), 1, "{source}");
        assert_eq!(search_visible_word(&source, miss), 0, "{source}");
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
