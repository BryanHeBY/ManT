//! Replayed native fields retain their accepted glyph, row and link owners.

use super::{Labels, Visit, description_rows, json_string_roundtrip};
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

type FieldHandoffCase = (
    &'static str,
    &'static [&'static str],
    Option<&'static str>,
    &'static str,
);
const FIELD_HANDOFF_CASES: &[FieldHandoffCase] = &[
    // In UTF-8, term_field() replaces C with J and prints Y on the same
    // accepted row. ASCII ends the row before Y; in both profiles the
    // original link label is gone (term.c:205-220,397-429).
    (
        include_str!("live_replay/replay-retires-link-overwritten-before-body.1"),
        &["JY"],
        None,
        "C",
    ),
    (
        include_str!("live_replay/replay-retires-link-overwritten-after-body-motion.1"),
        &["JY"],
        None,
        "C",
    ),
    (
        // term_word() overwrites the first slot with a marker after
        // absolute HORIZ; term_flushln() stops on zero accepted cells.
        include_str!("live_replay/replay-rejects-absolute-motion-to-field-origin.1"),
        &[""],
        None,
        "CD",
    ),
    (
        // term_field() sends covering glyphs to the same device cell in
        // native order. The gap does not spend an earlier owner's scalar.
        include_str!("live_replay/replay-selects-last-covering-glyph.1"),
        &["A   G"],
        None,
        "F",
    ),
    (
        include_str!("live_replay/replay-retires-covered-source-glyph.1"),
        &["A   X"],
        None,
        "GX",
    ),
    (
        include_str!("live_replay/replay-closes-before-relocated-word.1"),
        &["A   F", "X"],
        None,
        "FX",
    ),
    (
        // CVS term_field() leaves trailing padding buffered. A replayed
        // gap owned by the next word cannot consume this row's scalar.
        include_str!("live_replay/replay-keeps-break-before-detached-gap-owner.1"),
        &["ACFG   FG", "X"],
        None,
        "FG X",
    ),
    (
        // The accepting term_field() pass places this gap after the
        // relocated word, within the replay's accepted scalar stream.
        include_str!("live_replay/replay-keeps-gap-after-relocated-ink.1"),
        &["A  EEm   X"],
        None,
        "EEmX",
    ),
    (
        // The retained separator's first displayed cell originally
        // included a two-column advance before its native anchor.
        include_str!("live_replay/replay-keeps-only-surviving-gap-columns.1"),
        &["AX FG"],
        None,
        "AXFG",
    ),
    (
        // Ordinary SP leaves the pending retreat armed in term.c::encode.
        // The following HORIZ revisits D's decoration before the \\p.
        include_str!("live_replay/replay-rewinds-through-held-glyph-and-blank.1"),
        &["C X", "J"],
        None,
        "D",
    ),
    (
        include_str!("live_replay/replay-retains-held-glyph-and-blank-without-rewind.1"),
        &["C D  J X"],
        None,
        "JX",
    ),
    (
        // A zero-width graph after the pending glyph keeps its decorated
        // slots editable until HORIZ rewrites them (term.c::encode1).
        include_str!("live_replay/replay-retains-held-glyph-before-invisible-graph.1"),
        &["CXD", "J"],
        None,
        "CD",
    ),
    (
        // term_flushln() ends the accepted first row independently of
        // the replacement owner losing its source glyph in that pass.
        include_str!("live_replay/replay-keeps-break-after-source-owner-is-covered.1"),
        &["D", "J"],
        None,
        "DJ",
    ),
    (
        // Native SP and the local BACKBEFORE pair stay editable across
        // the first marker. The later cursor lands in D's decorated run.
        include_str!("live_replay/replay-keeps-first-glyph-after-a-local-backstep.1"),
        &["C", "D", "F", "X"],
        None,
        "J",
    ),
    (
        // term_word() handles named specials before placing their native
        // glyphs. A printable width alone cannot certify the edited slots.
        include_str!("live_replay/replay-rejects-named-glyph-with-unproved-slot-shape.1"),
        &[""],
        None,
        "F",
    ),
    (
        include_str!("live_replay/replay-keeps-two-accepted-gap-columns.1"),
        &["A  X"],
        None,
        "A X",
    ),
    (
        include_str!("live_replay/replay-preserves-leading-motion-before-marker.1"),
        &["A     FG"],
        None,
        "X",
    ),
    (
        // term_word() still writes at col after the first operand has
        // moved col back, leaving lastcol beyond the unprinted marker.
        include_str!("live_replay/replay-replaces-unprinted-leading-marker.1"),
        &["Y"],
        None,
        "YY",
    ),
    (
        // Moving col back after Y preserves the earlier marker and
        // lastcol. term_flushln() rejects the complete all-blank prefix.
        include_str!("live_replay/replay-rejects-trailing-retreat-after-marker.1"),
        &[""],
        None,
        "Y",
    ),
    (
        // A later tight word replaces the earlier authored space. The
        // surviving separator gives term_fill() two accepted passes.
        include_str!("live_replay/replay-revisits-unprinted-authored-space.1"),
        &["X", "Y"],
        None,
        "XY",
    ),
    (
        include_str!("live_replay/replay-keeps-accepted-adjacent-link.1"),
        &["A   GX"],
        Some("G"),
        "X",
    ),
    (
        // term_word() writes X into the first marker, before the
        // surviving authored blank. term_field() prints both separators.
        include_str!("live_replay/replay-overwrites-buffer-origin.1"),
        &["X  Y"],
        None,
        "XY",
    ),
    (
        include_str!("live_replay/replay-keeps-deferred-space-after-tight-rewind.1"),
        &["X Y"],
        None,
        "XY",
    ),
    (
        include_str!("live_replay/replay-keeps-deferred-space-after-clipped-rewind.1"),
        &["X Y"],
        None,
        "XY",
    ),
    (
        include_str!("live_replay/run-in-replays-across-font-overwrite.1"),
        &["-", "Y"],
        None,
        "",
    ),
    (
        include_str!("live_replay/run-in-replays-linked-word-after-marker.1"),
        &["-", "Y"],
        Some("Y"),
        "",
    ),
    (
        include_str!("live_replay/run-in-omits-covered-word.1"),
        &["No"],
        None,
        "Z",
    ),
    (
        include_str!("live_replay/run-in-preserves-linked-overprint.1"),
        &["NZ"],
        Some("Z"),
        "No",
    ),
    (
        include_str!("live_replay/run-in-owns-overprinted-letter-once.1"),
        &["V"],
        None,
        "VV",
    ),
    (
        include_str!("live_replay/run-in-assigns-link-to-covering-letter.1"),
        &["V"],
        Some("V"),
        "VV",
    ),
    (
        include_str!("live_replay/run-in-orders-multiple-covering-words.1"),
        &["Ns NYBC"],
        None,
        "BCY",
    ),
    (
        include_str!("live_replay/run-in-keeps-internal-motion-out-of-seam.1"),
        &["AB"],
        None,
        "A B",
    ),
    (
        include_str!("live_replay/replay-keeps-retained-motion-width.1"),
        &["F  X"],
        None,
        "FX",
    ),
    (
        include_str!("live_replay/run-in-replays-rows-after-relative-motion.1"),
        &["XBV", "Z"],
        None,
        "SXy",
    ),
    (
        include_str!("live_replay/replay-rejects-from-first-pass.1"),
        &[""],
        None,
        "X",
    ),
    (
        include_str!("live_replay/replay-keeps-advance-with-embedded-marker.1"),
        &["G   X"],
        None,
        "GX",
    ),
    (
        include_str!("live_replay/replay-keeps-advance-before-zero-width.1"),
        &["AG  X"],
        None,
        "AGX",
    ),
    (
        include_str!("live_replay/replay-keeps-advance-after-multistyle-prefix.1"),
        &["ABG   X"],
        None,
        "ABGX",
    ),
    (
        include_str!("live_replay/replay-retires-after-two-accepted-passes.1"),
        &["C", "F", ""],
        None,
        "X",
    ),
    (
        include_str!("live_replay/replay-rejects-edited-invisible-prefix.1"),
        &[""],
        None,
        "X",
    ),
    (
        include_str!("live_replay/replay-keeps-advance-across-breakpoint.1"),
        &["AFG", "FG  X"],
        None,
        "FGX",
    ),
    (
        // term_field() prints one gap from four native SP slots after
        // the final HORIZ has borrowed a blank from the prior word.
        include_str!("live_replay/replay-deduplicates-relocated-word-blank.1"),
        &["A    X"],
        None,
        "AX",
    ),
    (
        include_str!("live_replay/replay-rejects-across-pending-break-markers.1"),
        &[""],
        None,
        "X",
    ),
    (
        include_str!("live_replay/run-in-hands-off-a-pass-after-invisible-controls.1"),
        &["Em CV No", "Z"],
        None,
        "NoZ",
    ),
    (
        include_str!("live_replay/run-in-omits-overpainted-space-in-label.1"),
        &["CD", "Y"],
        Some("C"),
        "C D",
    ),
    (
        include_str!("live_replay/replay-keeps-partially-overwritten-advance.1"),
        &["A   FG"],
        None,
        "X",
    ),
    (
        include_str!("live_replay/replay-moves-retained-gap-after-covering-word.1"),
        &["X FG"],
        None,
        "XFG",
    ),
    (
        // term_field's pending advance precedes the glyph. Its projection
        // must not spend the scalar offset of term_flushln's later endline.
        include_str!("live_replay/replay-counts-authored-scalars-after-native-prefix.1"),
        &["FB F", "X"],
        None,
        "FX",
    ),
    (
        // The generated inset operand covers the last visible HEAD glyph.
        // The term_newln() row still exists before either layout request.
        include_str!("live_replay/run-in-keeps-covered-row-before-paragraph.1"),
        &["", "", "X"],
        None,
        "C",
    ),
    (
        include_str!("live_replay/run-in-keeps-covered-row-before-line-break.1"),
        &["", "X"],
        None,
        "C",
    ),
    (
        include_str!("live_replay/run-in-keeps-covered-row-before-display.1"),
        &["", "", "X"],
        None,
        "C",
    ),
    (
        // term_field() joins the generated advance and authored word
        // blanks into one printed run without changing their ownership.
        include_str!("live_replay/replay-preserves-gap-and-authored-blank.1"),
        &["A    FG"],
        None,
        "X",
    ),
    (
        include_str!("live_replay/replay-prints-gap-before-relocated-glyph.1"),
        &["X G"],
        None,
        "XG",
    ),
    (
        // term.c::bufferc() advances over existing ink for a space, but
        // encode1() can still replace its raw slot with a later glyph.
        include_str!("live_replay/replay-overwrites-fixed-blank-after-marker.1"),
        &["AG  X"],
        None,
        "ACFG",
    ),
    (
        include_str!("live_replay/replay-prints-covered-padding-on-second-pass.1"),
        &["A", "CG  X"],
        None,
        "CGX",
    ),
    (
        include_str!("live_replay/replay-overwrites-fixed-blank-at-field-origin.1"),
        &["G  X"],
        None,
        "CFG",
    ),
    (
        include_str!("live_replay/replay-overwrites-fixed-zero-escape-after-motion.1"),
        &["G  X"],
        None,
        "CFG",
    ),
    (
        // term_field() prints buffered positioning before the word's
        // fixed blank and its covering glyph. The positioning has no
        // scalar in the word's accepted coverage.
        include_str!("live_replay/replay-keeps-ink-after-covered-fixed-blank.1"),
        &["CD     X"],
        None,
        "CDX",
    ),
    (
        // Replayed positioning can coexist with an authored word blank.
        // Only the independent advance is outside the scalar stream.
        include_str!("live_replay/replay-keeps-authored-scalar-in-advance.1"),
        &["A   KXG"],
        None,
        "AKXG",
    ),
];

// term.c::encode1() edits the existing buffer after termp_it_pre()
// generates the run-in word. term_flushln() accepts the final slots;
// ownership and hard lines follow the accepted interval, including links.
#[test]
fn field_edits_keep_native_passes_and_reading_owners() {
    for &(source, expected, link, absent) in FIELD_HANDOFF_CASES {
        let mut content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        for _ in 0..3 {
            let rendered = mant_render::render_query_man(&content);
            let rows: Vec<_> = description_rows(&rendered, source)
                .iter()
                .map(|line| line.trim_start().trim_start_matches("• ").trim())
                .collect();
            assert_eq!(rows, expected, "{source}");
            let mut labels = Labels::default();
            labels.visit_document(content.document.as_ref().unwrap());
            let targets: Vec<_> = link
                .into_iter()
                .map(|label| ("https://k.test".into(), label.into()))
                .collect();
            assert_eq!(labels.0, targets, "{source}");
            let visible_words = expected
                .iter()
                .flat_map(|row| row.split_whitespace())
                .filter(|word| word.chars().any(char::is_alphanumeric));
            for (pattern, expected) in visible_words
                .map(|word| (word, 1))
                .chain((!absent.is_empty()).then_some((absent, 0)))
            {
                let hits = mant_query::search_query(
                    &content,
                    &SearchQuery {
                        pattern: pattern.into(),
                        syntax: SearchSyntax::Literal,
                        case: SearchCase::Sensitive,
                        scope: SearchScope::Visible,
                        word: true,
                        context_lines: 0,
                        limit: 10,
                        offset: 0,
                    },
                )
                .unwrap();
                assert_eq!(hits.total, expected, "{pattern}: {source}");
            }
            let markdown = mant_codec::encode::render_markdown(&content);
            content =
                json_string_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
        }
    }
}

#[test]
fn rejected_or_covered_links_publish_no_label() {
    // term_flushln() accepts no glyph in the first page. In the second,
    // term_field() overwrites F before the terminal leaves the cell.
    for (source, expected) in [
        (
            include_str!("live_replay/replay-rejects-link-after-absolute-motion.1"),
            &[""][..],
        ),
        (
            include_str!("live_replay/replay-retires-covered-link-label.1"),
            &["A   G"][..],
        ),
    ] {
        let mut content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        for _ in 0..3 {
            let rendered = mant_render::render_query_man(&content);
            assert_eq!(description_rows(&rendered, source), expected, "{source}");
            let mut labels = Labels::default();
            labels.visit_document(content.document.as_ref().unwrap());
            assert!(
                labels.0.iter().all(|(_, label)| label.is_empty()),
                "{source}"
            );
            let markdown = mant_codec::encode::render_markdown(&content);
            content =
                json_string_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
        }
    }
}

#[test]
fn replayed_separators_preserve_all_accepted_words() {
    let source = include_str!("live_replay/replay-keeps-interleaved-ink-after-gap.1");
    let mut content =
        json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    for _ in 0..3 {
        let rendered = mant_render::render_query_man(&content);
        let rows = description_rows(&rendered, source);
        // Protect the words and native row independently of gap width.
        assert_eq!(rows.len(), 1, "{source}");
        assert!(rows[0].split_whitespace().eq(["AX", "FG"]), "{rows:?}");
        let markdown = mant_codec::encode::render_markdown(&content);
        content = json_string_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
    }
}

#[test]
fn armed_run_in_word_keeps_the_retained_cursor() {
    const HEADER: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
    // termp_it_pre(BODY) writes the generated fixed blank at the retained
    // cursor. term.c::encode1() consumes BACKAFTER after writing it, so
    // attaching a bare \z to either end of the marker leaves one pass.
    // ASCII preserves only an underline stroke there; UTF-8 has no
    // readable glyph. A real BODY break closes the same retained row.
    for marker in [r"\p", r"\p\z", r"\z\p"] {
        for (body, rows) in [
            (".br\n.No X", &["", "X"][..]),
            (".Pp\n.No X", &["", "", "X"][..]),
            (".Bd -literal\n.No X\n.Ed", &["", "", "X"][..]),
            (".No X", &["X"][..]),
            (".No \\cX", &["X"][..]),
        ] {
            let source = format!(
                "{HEADER}.Bl -inset\n.It Em \"\\zC\\pJ\" No \"\\h'-2n'{marker}\"\n{body}\n.El\n.Sh ENDTEST\nDone.\n"
            );
            let content =
                json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
            let rendered = mant_render::render_query_man(&content);
            let actual = description_rows(&rendered, &source);
            assert!(
                actual
                    .iter()
                    .map(|row| row.trim_start())
                    .eq(rows.iter().copied()),
                "{source}: {actual:?}"
            );
        }
    }
}

#[test]
fn run_in_space_stays_editable_after_its_reading_owner_is_assigned() {
    const PAGE: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n.Ss CASE0CASE\n";
    const LINK: &str = r"\X'tty: link https://k.test'V\X'tty: link'";
    // termp_it_pre(BODY) writes a fixed space over the retained SP and
    // encode1 turns BACKAFTER into BACKBEFORE. The BODY motion consumes
    // it before placing any new glyph (term.c:678-714,903-929).
    for (head, body, ink, linked) in [
        (r#""V" "\h'-2n'\&\z""#, LINK.to_string(), "V", true),
        (r#""VV" "\h'-2n'\z\&""#, "V".to_string(), "VV", false),
        (r#""V" "\c\h'-2n'\&\z""#, LINK.to_string(), "V", true),
        (r#""V" "\h'-2n'\z\&""#, format!(r"\z{LINK}"), "V", true),
    ] {
        let source = format!(
            "{PAGE}.Bl -inset\n.It Sy {head}\n.No \"\\h'-2n'{body}\"\n.El\n.Ss CASE999CASE\nDone.\n"
        );
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let mut labels = Labels::default();
        labels.visit_document(content.document.as_ref().unwrap());
        assert_eq!(labels.0.len(), usize::from(linked), "{source}");
        if linked {
            assert_eq!(
                labels.0[0],
                ("https://k.test".into(), "V".into()),
                "{source}"
            );
        }
        let rendered = mant_render::render_query_man(&content);
        let (_, region) = rendered.split_once("CASE0CASE\n").unwrap();
        let (region, _) = region.split_once("CASE999CASE\n").unwrap();
        let rows = region
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty());
        let actual: Vec<String> = rows
            .map(|row| {
                row.chars()
                    .filter(|character| !character.is_whitespace())
                    .collect()
            })
            .collect();
        // The ASCII and UTF-8 fixed profiles disagree on spacing in
        // the two-V variant, but agree on ink and the accepted row.
        assert_eq!(actual, [ink], "{source}\n{rendered}");
    }
}

#[test]
fn motion_preserves_the_run_in_gap_written_before_invisible_cells() {
    let source = r#".Dd November 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd field probe
.Sh DESCRIPTION
.Bl -inset
.It No "\X'tty: link https://k.test'C\X'tty: link'V" Em "\h'-1n'\z"
.No ""
.No "\h'-1n'Y"
.El
.Sh ENDTEST
Done.
"#;
    // The generated BODY NBSP consumes BACKAFTER at the retained cursor.
    // The later HORIZ clears BACKBEFORE after intervening zero-width cells,
    // leaving the earlier gap inside the native pass (term.c:701-714).
    // ASCII and UTF-8 encode different gap widths; both preserve the words.
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let rendered = mant_render::render_query_man(&content);
    let rows = description_rows(&rendered, source);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].split_whitespace().eq(["CV", "Y"]), "{rows:?}");
    let mut labels = Labels::default();
    labels.visit_document(content.document.as_ref().unwrap());
    assert_eq!(labels.0, [("https://k.test".into(), "C".into())]);
}

#[test]
fn accepted_run_in_gap_separates_words_after_projection_replay() {
    let source = r#".Dd November 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd field probe
.Sh DESCRIPTION
.Bl -diag
.It Em "V" No "\h'-1n'\z\:"
.No "\h'-5n'\p"
.Sy "\pZ"
.El
.Sh ENDTEST
Done.
"#;
    // term_fill()/term_field() accept the UTF-8 generated NBSP between
    // the replayed markers. ASCII ends a row at that point. Both preserve
    // the lexical boundary between the final label word and the BODY.
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let rendered = mant_render::render_query_man(&content);
    let rows = description_rows(&rendered, source);
    assert!(
        rows.iter()
            .flat_map(|row| row.split_whitespace())
            .eq(["Em", "V", "No", "Z"])
    );
    let markdown = mant_codec::encode::render_markdown(&content);
    let markdown = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let rendered = mant_render::render_query_man(&markdown);
    assert!(rendered.split_whitespace().any(|word| word == "Z"));
    assert!(!rendered.contains("NoZ"));
}

#[test]
fn replayed_retreats_keep_their_native_replacement_order() {
    const HEADER: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
    // encode1() buffers C, a backspace, and the italic X's paint. Another
    // cursor rewrite restates the coverage events in a different order.
    // term_field() still paints X over C, independent of replay order.
    let source = format!(
        r#"{HEADER}.Bl -inset
.It Sy "C\zJ" Em "\h'-4n'\&X"
.No "\h'-1n' B\zY" Ns No "X"
.El
.Sh ENDTEST
Done.
"#
    );
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let markup = mant_codec::encode::render_markdown(&content);
    let readback = mant_loader::load_markdown_text(&markup, None).unwrap();
    for (content, row) in [(&content, "X BX"), (&readback, "• X BX")] {
        let rendered = mant_render::render_query_man(content);
        assert_eq!(description_rows(&rendered, &source), [row]);
    }

    // The second rewind accepts the clipped first word and its separator.
    // Both fixed devices agree on these words, including the literal
    // annotation spelling after the initial escaped backslash.
    let source = format!(
        r#"{HEADER}.Bl -inset
.It Em "\\0X'tty: link https://k.test'C\X'tty: link' D" Sy "\h'-6n'\h'-6n'\&\&"
.No "\p"
.br
.No "\h'-1n'Y"
.El
.Sh ENDTEST
Done.
"#
    );
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let rendered = mant_render::render_query_man(&content);
    assert!(
        description_rows(&rendered, &source)
            .iter()
            .flat_map(|row| row.split_whitespace())
            .eq(["X'tty:", "link", "https://k.testC", "D", "Y"]),
        "{rendered}"
    );
}
