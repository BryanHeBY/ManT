//! Safety boundaries of the declared `Bl -column` projection.
//!
//! Expectations were recorded from the fixed reference binary
//! (`target/mandoc-migration/reference/mandoc`, `-Tutf8`) before the
//! assertions were written. Upstream anchors: `mdoc_term.c::termp_it_pre`
//! (699-747) accumulates declared widths plus the dcol gap (4/3/1 by declared
//! column count) and gives undeclared columns the default width of 10;
//! `term_ascii.c::ascii_advance()` truncates each single padding advance at
//! 256 columns with the comment that "the input document can trigger [that]
//! by merely providing large input".
use libmandoc_rs::{RenderFormat, Renderer};

const PRE: &str = ".Dd September 8, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\n";

fn lowered_body(source: &str) -> Vec<String> {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower");
    let text = mant_render::render_query_text(&query);
    text.split('\n')
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

/// Word-start columns of each non-empty body line, measured after removing
/// any page heading, so the assertions are independent of outer margins.
fn word_starts(lines: &[String]) -> Vec<Vec<usize>> {
    let body_start = lines
        .iter()
        .position(|line| line.contains("DESCRIPTION"))
        .map_or(0, |index| index + 1);
    lines[body_start..]
        .iter()
        .map(|line| {
            let mut columns = Vec::new();
            let mut in_word = false;
            for (index, ch) in line.char_indices() {
                let word = ch != ' ';
                if word && !in_word {
                    columns.push(index);
                }
                in_word = word;
            }
            columns
        })
        .filter(|columns| !columns.is_empty())
        .collect()
}

#[test]
fn declared_column_geometry_matches_the_reference() {
    // (declaration words, It cells, expected word starts per physical line)
    for (columns, cells, expected, label) in [
        (
            "one two",
            "A Ta B",
            vec![vec![0, 7]],
            "two declared columns",
        ),
        (
            "one two",
            "AAAA B Ta C",
            vec![vec![0, 5, 7]],
            "content ends before the next start",
        ),
        (
            "one two",
            "AAAA BB Ta C",
            vec![vec![0, 5], vec![7]],
            "content ends exactly at the next start",
        ),
        (
            "one two",
            "AAAA BBX Ta C",
            vec![vec![0, 5], vec![7]],
            "content ends past the next start",
        ),
        (
            "one two three",
            "AAAA BB Ta C Ta D",
            vec![vec![0, 5], vec![7, 14]],
            "first column full, three columns",
        ),
        (
            "one two three",
            "A Ta CCCCCCCCCCCC Ta D",
            vec![vec![0, 7], vec![14]],
            "middle column full",
        ),
        (
            "one two three",
            "A Ta C Ta DDDDDDDDDDDDDDDDDDDDDDDD",
            vec![vec![0, 7, 14]],
            "last column overruns untruncated",
        ),
        ("o o", "AAAA Ta B", vec![vec![0, 5]], "width-one columns"),
        (
            "\u{4e2d}\u{4e2d} one",
            "A Ta B",
            vec![vec![0, 8]],
            "device-measured CJK width 4",
        ),
        (
            "\\(lq\\(rq one",
            "A Ta B",
            vec![vec![0, 6]],
            "escape glyphs measure one column each",
        ),
    ] {
        let source = format!("{PRE}.Bl -column {columns}\n.It {cells}\n.El\n");
        assert_eq!(
            word_starts(&lowered_body(&source)),
            expected,
            "{label}:\n{source}"
        );
    }
}

#[test]
fn column_advances_truncate_at_256_columns_like_ascii_advance() {
    // Reference: a declared width of 252 puts B at start 256 (advance 255,
    // unclamped); widths of 256 and beyond clamp the advance to 256 columns,
    // and so does the 65531-column CW01 seed.
    for (width, b_start) in [(250_usize, 254), (252, 256), (256, 257), (300, 257)] {
        let source = format!(
            "{PRE}.Bl -column {} one\n.It A Ta B\n.El\n",
            "x".repeat(width)
        );
        assert_eq!(
            word_starts(&lowered_body(&source)),
            vec![vec![0, b_start]],
            "width {width}"
        );
    }
}

#[test]
fn absurd_column_declarations_stay_bounded_without_panicking() {
    // External audit CW01 seeds: 65531/65532/65535/65536-character width
    // strings used to overflow the u16 offset arithmetic (debug panic,
    // silent column collapse in release) and explode padding. Every boundary
    // now advances one truncated 256-column step and the content survives.
    for width in [256_usize, 300, 65531, 65532, 65535, 65536, 70_000] {
        let declared = "x".repeat(width);
        let source = format!("{PRE}.Bl -column {declared} {declared}\n.It A Ta B\n.El\n");
        let body = lowered_body(&source);
        assert_eq!(
            word_starts(&body),
            vec![vec![0, 257]],
            "width {width}: {body:?}"
        );
        for line in &body {
            assert!(
                line.chars().count() < 1024,
                "width {width}: {}",
                line.chars().count()
            );
        }
    }
    // Multi-column accumulation stays stepwise-bounded.
    let declared = "x".repeat(65535);
    let source =
        format!("{PRE}.Bl -column {declared} {declared} {declared}\n.It A Ta B Ta C\n.El\n");
    assert_eq!(word_starts(&lowered_body(&source)), vec![vec![0, 257, 514]]);
}

#[test]
fn undeclared_columns_start_after_all_declared_fields() {
    // termp_it_pre's preceding-BODY loop is capped at ncols: the third
    // cell starts after BOTH measured declarations plus their gaps. Its
    // own default width ten is not another preceding-column stride.
    let source = format!("{PRE}.Bl -column one two\n.It A Ta B Ta C\n.El\n");
    assert_eq!(word_starts(&lowered_body(&source)), vec![vec![0, 7, 14]]);
}

#[test]
fn native_and_lowered_column_geometry_agree() {
    // Cross-check the same table through the vendored UTF-8 terminal render
    // at width 78; the overstrike projection removes page furniture biases.
    let native = |source: &str| -> Vec<String> {
        let raw = Renderer::new(RenderFormat::Utf8)
            .with_width(78)
            .render_bytes("column-safety.1", source.as_bytes())
            .expect("native render")
            .output;
        let mut projected = String::new();
        for c in raw.chars() {
            if c == '\u{8}' {
                projected.pop();
            } else {
                projected.push(c);
            }
        }
        projected
            .split('\n')
            .filter(|line| !line.trim().is_empty())
            .map(str::to_owned)
            .collect()
    };
    for (columns, cells) in [
        ("one two", "A Ta B"),
        ("one two three", "AAAA BB Ta C Ta D"),
        ("one two three", "A Ta CCCCCCCCCCCC Ta D"),
        ("one two", "AAAA BB Ta C"),
        ("one two", "AAAA B Ta C"),
        ("o o", "AAAA Ta B"),
    ] {
        let source = format!("{PRE}.Bl -column {columns}\n.It {cells}\n.El\n");
        let native_lines = native(&source);
        let start = native_lines
            .iter()
            .position(|line| line.contains("DESCRIPTION"))
            .expect("description heading");
        let native_table = &native_lines[start + 1..];
        // The footer follows the table; stop at the system attribution line.
        let native_table: &[String] = native_table
            .split_at(
                native_table
                    .iter()
                    .position(|line| line.contains("Linux"))
                    .unwrap_or(native_table.len()),
            )
            .0;
        let lowered = lowered_body(&source);
        let lowered_table = {
            let body_start = lowered
                .iter()
                .position(|line| line.contains("DESCRIPTION"))
                .map_or(0, |index| index + 1);
            &lowered[body_start..]
        };
        let starts = |lines: &[String]| -> Vec<Vec<usize>> {
            lines
                .iter()
                .map(|line| {
                    let mut columns = Vec::new();
                    let mut in_word = false;
                    for (index, ch) in line.char_indices() {
                        let word = ch != ' ';
                        if word && !in_word {
                            columns.push(index);
                        }
                        in_word = word;
                    }
                    columns
                })
                .filter(|columns| !columns.is_empty())
                .collect()
        };
        let native_starts = starts(native_table);
        let lowered_starts = starts(lowered_table);
        // The native render indents the table by the `.Bl` margin; compare
        // the table-relative starts.
        let origin = native_starts.iter().flatten().copied().min().unwrap_or(0);
        let relative: Vec<Vec<usize>> = native_starts
            .into_iter()
            .map(|line| line.into_iter().map(|c| c - origin).collect())
            .collect();
        assert_eq!(
            relative, lowered_starts,
            "{columns} / {cells}:\nnative:  {native_table:?}\nlowered: {lowered_table:?}"
        );
    }
}

#[test]
fn declarations_use_term_strlen_semantics_instead_of_visible_word_projection() {
    // term.c::term_strlen: z skips the next measured glyph across font and
    // invisible controls; motion has no width; overstrike ignores escapes
    // and takes the widest remaining literal character. Exact source/header
    // oracle runs precede this assertion (column-safety-exact-new-cases).
    for (sample, second_start) in [
        ("\\zX", 4),
        ("A\\zX", 5),
        ("\\z\\fBX\\fP", 4),
        ("\\z\\&X", 4),
        ("A\\h'1n'B", 6),
        ("\\o'中A'", 5),
        ("8n", 6),
        ("\\&", 4),
        ("\\~", 5),
        ("\\[vc]", 4),
        ("\\[e aa]", 4),
        ("\\[u0065_0301]", 4),
        ("\\z\\[e aa]X", 4),
        ("\\z\\[unknown]X", 4),
    ] {
        let source = format!("{PRE}.Bl -column \"{sample}\" \"b\"\n.It A Ta SECOND\n.El\n");
        assert_eq!(
            word_starts(&lowered_body(&source)),
            vec![vec![0, second_start]],
            "{sample}"
        );
    }
}

#[test]
fn buffered_column_padding_does_not_close_a_row_before_the_next_field() {
    // term_fill records the last graph before ordinary trailing spaces;
    // term_field postpones printing those spaces. Fixed blanks remain graph
    // cells. Exact pristine ASCII/UTF-8 inputs preceded these assertions.
    for (operand, expected) in [
        ("AAAA B", "AAAA B C"),
        ("AAAA BB", "AAAA BB\n       C"),
        ("AAAA B\\~", "AAAA B\u{a0}\n       C"),
        ("AAAA B\\0", "AAAA B\u{a0}\n       C"),
        ("Em \"AAAA B\"", "AAAA B C"),
        ("Sy \"AAAA B\"", "AAAA B C"),
        ("中中 B", "中中 B C"),
        ("中中 BB", "中中 BB\n       C"),
        ("😀😀 B", "😀😀 B C"),
        ("😀😀 BB", "😀😀 BB\n       C"),
    ] {
        let source = format!("{PRE}.Bl -column AAA BBB -compact\n.It {operand} Ta C\n.El\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&query, false).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let decoded: mant_ir::ResolvedContent = decoded.into();
        let plain = mant_render::render_query_man(&decoded);
        let body = plain.split_once("DESCRIPTION\n").unwrap().1;
        assert_eq!(body.trim_matches('\n'), expected, "{operand}");
        let decorated = mant_render::render_query_text_with(&decoded, |_, text| {
            format!("\u{1b}[1m{text}\u{1b}[0m")
        });
        let stripped = decorated.replace("\u{1b}[1m", "").replace("\u{1b}[0m", "");
        let body = stripped.split_once("DESCRIPTION\n").unwrap().1;
        assert_eq!(body.trim_matches('\n'), expected, "{operand}: ANSI");
    }
}

#[test]
fn lists_without_items_do_not_claim_an_it_vertical_pre_request() {
    // mdoc_term.c::termp_bl_pre/post only run term_newln; print_bvspace
    // belongs to It. Pristine exact sources were run for all three styles.
    let pre = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n";
    for style in ["-item", "-tag -width 4n", "-column aa b"] {
        let source = format!("{pre}.No BEFORE\n.Bl {style}\n.El\n.No AFTER\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let text = mant_render::render_query_man(&query);
        let body = text.split_once("DESCRIPTION\n").unwrap().1;
        assert_eq!(body.trim_matches('\n'), "BEFORE\nAFTER", "{style}");
    }
}
