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
    let plain = render(source, "roff", "text", "never", false);
    let ansi = render(source, "roff", "text", "always", false);
    assert_eq!(description(&plain), expected, "{source}");
    assert_eq!(without_sgr(&ansi), plain, "{source}");
    if let Some(label) = label {
        assert_link(source, "roff", label);
    }
    for addressable in [false, true] {
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
fn unmodeled_word_entry_cannot_authorize_a_native_rejection() {
    // This exact pinned CVS input moves back far enough to replace the
    // ordinary A, yielding ALPH / BETA / OMEGA. ManT's documented bounded
    // rewind excludes ordinary authored glyphs; its conservative spelling
    // preserves both words instead of pruning from mispositioned markers.
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
    assert_readbacks(source, "ALPHAX BETA\nOMEGA", None);
    // If motion occurs inside one operand, the independent leading-word
    // plan is not applicable either. Retain the new label even when the
    // native terminal moves the cursor and changes the marker's pass.
    for distance in ["-1n", "-2n"] {
        let inline = format!(
            "{}.No \"ALPHA\\zX \\h'{distance}'\\p\\: BETA\"\n.br\n.No OMEGA\n",
            source.split_once(".No").unwrap().0
        );
        assert_readbacks(&inline, "ALPHAX  BETA\nOMEGA", None);
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
