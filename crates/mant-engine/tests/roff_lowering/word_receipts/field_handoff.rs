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
