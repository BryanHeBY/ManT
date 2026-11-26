//! A marked run-in HEAD stays writable until the native field flushes.

use mant_ir::ResolvedContent;

const REWIND: &str = "\\h'|0n'B C\n";
const ITALIC: &str = r#"No "\fIA\p ""#;
const DIAGONAL: &str = r#""A\p ""#;

fn source(style: &str, head: &str, body: &str) -> String {
    let description = if style == "diag" {
        "field execution review"
    } else {
        "field review"
    };
    format!(
        ".Dd November 26, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd {description}\n\
         .Sh DESCRIPTION\n.Bl -{style}\n.It {head}\n{body}.El\n.Sh ENDTEST\nDone.\n"
    )
}

fn description_rows(content: &ResolvedContent) -> Vec<String> {
    let rendered = mant_render::render_query_man(content);
    let description = rendered
        .split_once("DESCRIPTION\n")
        .unwrap()
        .1
        .split_once("\nENDTEST\n")
        .unwrap()
        .0;
    description
        .trim_end()
        .lines()
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

fn load(source: &str) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    super::json_roundtrip::json_string_roundtrip(&content)
}

#[test]
fn italic_inset_head_is_writable_after_a_marked_trailing_blank() {
    // termp_it_post leaves inset HEAD slots live. BODY pre appends NBSP,
    // then term_word's HORIZ revisits the field before term_fill reads the
    // marker (mdoc_term.c:757–772, 939–945 and term.c:659–660, 678–718).
    let source = source("inset", ITALIC, REWIND);
    assert_eq!(description_rows(&load(&source)), ["C"], "{source}");
}

#[test]
fn implicitly_bold_diagonal_head_is_writable_after_a_marked_trailing_blank() {
    // diag HEAD has no parsed child macros (mdoc_macro.c:1140–1142).
    // termp_it_pre supplies its font and appends two NBSPs on BODY entry.
    let source = source("diag", DIAGONAL, REWIND);
    assert_eq!(description_rows(&load(&source)), ["C"], "{source}");
}

#[test]
fn a_body_word_does_not_commit_the_live_head() {
    // term_word() can write B and then return to the same raw origin.
    // At BODY post, term_fill() reads the resulting slots (term.c:264–369).
    let source = source("diag", DIAGONAL, "B \\h'|0n'C D\n");
    assert_eq!(description_rows(&load(&source)), ["D", "B"], "{source}");
}

#[test]
fn a_real_break_flushes_the_marked_head_before_the_rewind() {
    // roff_term_pre_br calls term_newln. The accepted rows include the
    // separator-only row between the flushed HEAD and the rewound BODY.
    for (style, head) in [("inset", ITALIC), ("diag", DIAGONAL)] {
        let source = source(style, head, &format!(".br\n{REWIND}"));
        assert_eq!(
            description_rows(&load(&source)),
            ["A", "", "B C"],
            "{source}"
        );
    }
}

#[test]
fn unmarked_or_unpadded_head_still_accepts_the_rewind() {
    for (style, head) in [("inset", r#"No "\fIA\p""#), ("diag", r#""A ""#)] {
        let source = source(style, head, REWIND);
        assert_eq!(description_rows(&load(&source)), ["C"], "{source}");
    }
}

#[test]
fn the_marker_still_breaks_before_body_text_without_a_rewind() {
    for (style, head) in [("inset", ITALIC), ("diag", DIAGONAL)] {
        let source = source(style, head, "B C\n");
        assert_eq!(description_rows(&load(&source)), ["A", "B C"], "{source}");
    }
}
