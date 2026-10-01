//! Escape/device-semantics matrix against pinned CVS mandoc `-Tutf8` output.
//!
//! Native escape regression corpus: native write receipts (`\z` BACKBEFORE retreat
//! over a word separator, term.c:901-908), single-device UTF-8 escape
//! semantics (`\:` buffers `ASCII_NBRZW`, chars.c:53 with term.c:631-632;
//! `\!`/`\?`/`\r` leave no footprint, roff_escape.c:156-160), the `\p`
//! pass rejections (term.c:143-146 with 233-237), and the generated run-in
//! cell overstrike order (term.c:901-908 through encode1(U+00A0)).
//!
//! Each case under `escape_matrix/cases/*.1` records the **row-grouped**
//! output of the pinned reference (`-Tutf8`), produced only by
//! `scripts/roff/fixtures/regen_escape_matrix.sh` — never by hand. Unlike
//! `definition_matrix` (recorded `-Tascii`), this matrix MUST stay UTF-8:
//! its cases sit exactly on the device fork. Row grouping preserves every
//! body row — section heads included — and every blank row between body
//! rows, so a lost or stray paragraph row fails the matrix. Page
//! furniture goes by position window, never by content: row 0 (plus an
//! optional wrapped center line) is the header, the trailing non-blank
//! block is the footer, and the page-edge blanks frame them both — so no
//! body row, however date- or OS-shaped, can be mistaken for either.

use std::fmt::Write as _;

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/roff_lowering/escape_matrix/cases"
);

/// `true` for the `name(section)` tokens that headline page furniture:
/// the reference's `name(1) … name(1)` header and `date … name(1)` footer
/// corner, and this renderer's own `name(1)` label row.
fn is_section_token(token: &str) -> bool {
    if !token.ends_with(')') {
        return false;
    }
    let Some(open) = token.rfind('(') else {
        return false;
    };
    let inner = &token[open + 1..token.len() - 1];
    inner
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_digit())
        && inner.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// The page head both renderers always lay down before any body row —
/// the reference's header (single line, wrapped two-line form, or the
/// metadata-default `UNTITLED LOCAL UNTITLED` / `() ()` degenerate
/// corners) and this renderer's `name(1)` label row. Only a debug
/// tripwire for the head window: recall is positional, never lexical.
fn is_page_head(row: &str) -> bool {
    row.split(' ')
        .any(|token| is_section_token(token) || token == "()")
        || row.starts_with("UNTITLED")
}

/// The probe's row-grouping normalization: overstrike projection and
/// inline-whitespace collapse, then the position-window furniture drop.
/// The head window takes row 0 — both renderers always lay the header or
/// label down there — plus an optional non-blank row 1, the wrapped
/// center line of a two-line reference header, and the page-edge blanks
/// that follow. The tail window adapts per device: the reference always
/// foots the page, so a `has_footer` page loses its trailing non-blank
/// block (the 1–3 row footer) and the blanks framing it, while this
/// footerless renderer only trims trailing blanks. Body rows and blank
/// rows between body rows are paragraph structure and stay pinned:
/// losing a body row, losing or adding a blank row, or rendering a
/// body-bearing page empty all fail.
fn normalize(output: &str, has_footer: bool) -> Vec<String> {
    let mut projected = String::with_capacity(output.len());
    for character in output.chars() {
        if character == '\u{8}' {
            projected.pop();
        } else if character == '\u{a0}' {
            // The generated run-in cell is NBSP on this device; row
            // grouping treats it as the blank it occupies.
            projected.push(' ');
        } else if character == '\u{2013}' || character == '\u{2014}' {
            // The en/em dash glyph is a tracked presentation deviation
            // (G7); row grouping normalizes it on both sides.
            projected.push('-');
        } else {
            projected.push(character);
        }
    }
    let mut rows: Vec<String> = projected
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    // Row 0 is furniture on the faith of the layout invariant; the debug
    // tripwire keeps that faith observable.
    if let Some(head) = rows.first() {
        debug_assert!(is_page_head(head), "row 0 is not page furniture: {head:?}");
        rows.remove(0);
    }
    // A non-blank row 1 without a section token is the wrapped center of
    // a two-line header; this renderer's row 1 is always the title-gap
    // blank, so the branch stays dormant on that side.
    if rows
        .first()
        .is_some_and(|row| !row.is_empty() && !row.split(' ').any(is_section_token))
    {
        rows.remove(0);
    }
    while rows.first().is_some_and(String::is_empty) {
        rows.remove(0);
    }
    if has_footer {
        while rows.last().is_some_and(|row| !row.is_empty()) {
            rows.pop();
        }
    }
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

/// This renderer lays out no footer, so only the head window and the
/// page-edge blanks go; the trailing body block always stays.
fn normalize_mant(output: &str) -> Vec<String> {
    normalize(output, false)
}

#[test]
fn row_grouping_keeps_body_and_blank_rows_and_drops_only_furniture() {
    let page = concat!(
        "T(1)                General Commands Manual                T(1)\n",
        "\n",
        "NAME\n",
        "     t - probe\n",
        "\n",
        "DESCRIPTION\n",
        "     A\n",
        "\n",
        "Linux 6.18.40.1-microsoft-standard-WSL2\n",
        "                              September 30, 2026                       T(1)\n",
    );
    assert_eq!(
        normalize(page, true),
        ["NAME", "t - probe", "", "DESCRIPTION", "A"],
        "body rows, section heads, and the paragraph blank stay; the head window, the page-edge blanks, and the footer block go"
    );
    // A long name wraps the header into a corner line plus a center
    // line; the head window takes both.
    let long = "t".repeat(60);
    let wrapped =
        format!("{long}(1)\n             General Commands Manual\n\nBODY ROW\n\n{long}(1)\n");
    assert_eq!(normalize(&wrapped, true), ["BODY ROW"]);
    // Metadata defaults are still furniture by position: the `UNTITLED`
    // header and the dateless `()` footer corner.
    assert_eq!(
        normalize("UNTITLED  LOCAL  UNTITLED\n\nBODY\n\n()\n", true),
        ["BODY"]
    );
    // An all-furniture page (empty body) groups to nothing.
    assert_eq!(
        normalize(
            concat!(
                "E(1)                    General Commands Manual                    E(1)\n",
                "\n",
                "Linux 6.18.40.1-microsoft-standard-WSL2\n",
                "                                             E(1)\n",
            ),
            true
        ),
        Vec::<String>::new()
    );
    // Adversarial body rows are content, never furniture: recall is
    // positional, so date-shaped, OS-shaped, and `printf(3)` rows inside
    // the body survive verbatim instead of matching a furniture glossary.
    let extreme = concat!(
        "X(1)                    General Commands Manual                    X(1)\n",
        "\n",
        "Linux commands begin the body here.\n",
        "\n",
        "Mid body references printf(3) and a Linux command.\n",
        "\n",
        "A date-shaped body row: 2026-09-30 2026-09-30 printf(3)\n",
        "\n",
        "GNU ends near the tail.\n",
        "\n",
        "printf(3)\n",
        "\n",
        "Linux 6.18.40.1-microsoft-standard-WSL2\n",
        "                              September 30, 2026                       X(1)\n",
    );
    assert_eq!(
        normalize(extreme, true),
        [
            "Linux commands begin the body here.",
            "",
            "Mid body references printf(3) and a Linux command.",
            "",
            "A date-shaped body row: 2026-09-30 2026-09-30 printf(3)",
            "",
            "GNU ends near the tail.",
            "",
            "printf(3)",
        ],
        "the head window and the footer block go; every body row and paragraph blank stays"
    );
    // This renderer's own shape: the label row and its title-gap blanks
    // are page-edge layout, and with no footer the trailing body block
    // stays put.
    assert_eq!(normalize_mant("x(1)\n\n\nS\nX"), ["S", "X"]);
    assert_eq!(
        normalize_mant("x(1)\n\nLinux command row\n\n2026-09-30 x(1)\n\nprintf(3)\n"),
        ["Linux command row", "", "2026-09-30 x(1)", "", "printf(3)"]
    );
    // Whatever row 0 carries goes with the head window — even a date
    // corner — while the same text inside the body is content.
    assert_eq!(normalize_mant("2026-09-30  x(1)"), Vec::<String>::new());
    assert_eq!(
        normalize(
            "D(1)  General Commands Manual  D(1)\n\n2026-09-30 x(1)\n\n()\n",
            true
        ),
        ["2026-09-30 x(1)"]
    );
    // Losing or adding a paragraph blank changes the pinned rows.
    assert_ne!(
        normalize_mant("x(1)\n\nS\n\nX"),
        normalize_mant("x(1)\n\nS\nX")
    );
}

#[test]
fn escape_matrix_rows_match_the_pinned_utf8_reference() {
    let mut failures = Vec::new();
    let mut total = 0;
    let mut entries: Vec<_> = std::fs::read_dir(CASES)
        .expect("escape matrix case directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "1"))
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "no matrix cases under {CASES}");
    for source_path in entries {
        let name = source_path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let expected_path = source_path.with_extension("expected");
        let expected = std::fs::read_to_string(&expected_path).unwrap_or_else(|error| {
            panic!("missing snapshot {}: {error}", expected_path.display())
        });
        let expected_rows: Vec<String> = expected.lines().map(str::to_owned).collect();
        let source = std::fs::read_to_string(&source_path).expect("read case source");
        let query = mant_loader::load_roff_bytes(source.as_bytes())
            .unwrap_or_else(|error| panic!("{name}: lower case: {error}"));
        let rendered = mant_render::render_query_man(&query);
        let actual = normalize_mant(&rendered);
        total += 1;
        if actual != expected_rows {
            let mut report = format!("{name}\n");
            for (index, (actual_row, expected_row)) in
                actual.iter().zip(expected_rows.iter()).enumerate()
            {
                if actual_row != expected_row {
                    let _ = writeln!(
                        report,
                        "  row {index}: ManT {actual_row:?} vs reference {expected_row:?}"
                    );
                }
            }
            if actual.len() != expected_rows.len() {
                let _ = writeln!(
                    report,
                    "  row count: ManT {} vs reference {}",
                    actual.len(),
                    expected_rows.len()
                );
            }
            failures.push(report);
        }
    }
    assert_eq!(
        total, 80,
        "case set changed; regen via scripts/roff/fixtures/regen_escape_matrix.sh"
    );
    assert!(
        failures.is_empty(),
        "row-grouping diffs:\n{}",
        failures.join("")
    );
}
