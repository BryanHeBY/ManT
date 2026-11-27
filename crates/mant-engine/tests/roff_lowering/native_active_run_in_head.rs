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

fn combined_inset(head: &str, body: &str) -> String {
    source("inset", head, body).replace(".Sh DESCRIPTION\n", ".Sh DESCRIPTION\n.ft BI\n")
}

#[test]
fn a_generated_separator_carries_its_backstep_into_the_live_body() {
    // encode() buffers the ordinary SP without consuming BACKAFTER.
    // BODY pre's generated U+00A0 consumes it in encode1(), leaving
    // BACKBEFORE for B. A later term_word() can still rewind before the
    // marker (term.c:659–660, 678–718, 903–969; mdoc_term.c:757–772).
    let body = "B\n.No \"\\h'|0n'C\" No D\n";
    for (head, first_row) in [
        (r#"No "J\fIA\fP\p\z ""#, "C_DA"),
        (r#"Em "J\fIA\fP\p\z ""#, "DA"),
        (r#"Sy "J\fIA\fP\p\z ""#, "DA"),
    ] {
        let source = combined_inset(head, body);
        assert_eq!(
            description_rows(&load(&source)),
            [first_row, "B"],
            "{source}"
        );
    }
}

#[test]
fn other_body_words_keep_the_separator_backstep_editable() {
    // An empty .No can add an automatic SP and a raw text node can
    // execute HORIZ before BODY post flushes the field (term.c:573–582,
    // 678–718; mdoc_term.c:939–945).
    let body = "B\n.No \"\"\n\\h'|0n'C D\n";
    for (head, row) in [
        (r#"No "J\fIA\fP\p\z ""#, "C B"),
        (r#"Em "J\fIA\fP\p\z ""#, "CAB"),
        (r#"Sy "J\fIA\fP\p\z ""#, "CAB"),
    ] {
        let source = combined_inset(head, body);
        assert_eq!(description_rows(&load(&source)), [row], "{source}");
    }
}

#[test]
fn generated_separator_controls_keep_their_accepted_rows() {
    let without_arm = combined_inset(r#"No "J\fIA\fP\p ""#, "B\n.No \"\\h'|0n'C\" No D\n");
    assert_eq!(
        description_rows(&load(&without_arm)),
        ["C_DA", "B"],
        "{without_arm}"
    );

    // A real .br retires the field before the later rewind.
    let flushed = combined_inset(r#"No "J\fIA\fP\p\z ""#, "B\n.br\n.No \"\\h'|0n'C\" No D\n");
    assert_eq!(
        description_rows(&load(&flushed)),
        ["JA", "B", "C D"],
        "{flushed}"
    );
}

#[test]
fn a_marked_successor_replays_both_live_retreats() {
    // The devices differ on wrapping, but on both the Z consumes the
    // preceding write position and the former J does not survive.
    let head = r#"Em "C\p J" No "\h'-5n'\z""#;
    for marker in [r"\p\z", r"\p\p\z"] {
        let source = source("inset", head, &format!(".No \"{marker}\" Ns No \"\\pZ\"\n"));
        let painted: String = description_rows(&load(&source))
            .iter()
            .flat_map(|row| row.chars())
            .filter(|ch| !ch.is_whitespace())
            .collect();
        assert_eq!(painted, "CZ", "{source}");
    }
}

fn before_definition(control: &str, head: &str, body: &str) -> String {
    source("inset", head, body).replacen(".Bl -inset\n", &format!("{control}.Bl -inset\n"), 1)
}

#[test]
fn inherited_head_font_leaves_a_marker_and_trailing_space_editable() {
    // mandoc receives the decorated A then LF/SP; BODY's absolute HORIZ
    // still addresses those bytes. R and BI previously reached this same
    // native result; B and I must not commit A before the rewind.
    for font in ["R", "B", "I", "BI"] {
        let control = format!(".ft {font}\n");
        let head = r#""A\p ""#;
        let open = before_definition(&control, head, "\\h'|0n'C\n");
        assert_eq!(description_rows(&load(&open)), ["C"], "{open}");
        let closed = before_definition(&control, head, ".br\n\\h'|0n'C\n");
        assert_eq!(description_rows(&load(&closed)), ["A", "", "C"], "{closed}");
    }
}

#[test]
fn generated_ap_preserves_native_decoration_and_the_following_glyph() {
    // A styled apostrophe occupies raw native slots, just like authored
    // glyphs. A rewind into the preceding J must not erase the apostrophe
    // or the following A in JSON or either public reading projection.
    for (control, head, body, expected) in [
        (".ft I\n", "No J Ap No A", "\\h'|0n'C\n", &["C'A"][..]),
        (".ft I\n", "No J Ap No AK", "\\h'|0n'C\n", &["C'AK"]),
        (".ft I\n", "No J Ap No A", ".br\n\\h'|0n'C\n", &["J'A", "C"]),
        (".ft I\n", "No J Op A", "\\h'|0n'C\n", &["[A]"]),
        (".ft I\n", "No J Pq A", "\\h'|0n'C\n", &["(A)"]),
        (
            ".Sm off\n.ft BI\n",
            r#"No J Ap No "A\p ""#,
            "\\h'|0n'C D\n",
            &["C'D"],
        ),
    ] {
        let source = before_definition(control, head, body);
        let content = load(&source);
        assert_eq!(description_rows(&content), expected, "{source}");
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let rows = description_rows(&readback);
        let rows: Vec<_> = rows
            .iter()
            .map(|row| row.strip_prefix("• ").unwrap_or(row))
            .collect();
        assert_eq!(rows, expected, "{source}\n{markdown}");
    }
}
