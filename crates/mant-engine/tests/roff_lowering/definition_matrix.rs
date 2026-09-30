//! Definition row-machine matrix against pinned CVS mandoc output.
//!
//! Each case under `definition_matrix/cases/*.1` is an mdoc document
//! exercising the NOBREAK head field row machine (hang/tag heads, `.sp`,
//! `.br`, `.nf`/`.fi`, width sweeps, and `\p` markers). The sibling
//! `.expected` file holds the **row-grouped** output of the fixed CVS
//! reference binary (`target/mandoc-migration/reference/mandoc -Tascii`),
//! recorded once by `scripts/regen_definition_matrix.sh`: which words
//! share a physical row is the row machine's observable decision, so page
//! furniture drops by position window — row 0 (plus an optional wrapped
//! center line) and the trailing footer block — never by content, and
//! all-caps section heads like `OPTIONS` are body rows. Regenerate the
//! expectations only with that script, never by hand.
//!
//! There is deliberately no `\:` case here: `\:` sits exactly on the
//! ascii/UTF-8 device fork (chars.c:53 — `ASCII_BREAK` byte versus `NBRZW`),
//! and this matrix is recorded `-Tascii`. The UTF-8 side of that fork and
//! the rest of the escape semantics live in `escape_matrix`, recorded
//! `-Tutf8` by `scripts/regen_escape_matrix.sh`.

use std::fmt::Write as _;

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/roff_lowering/definition_matrix/cases"
);

/// `true` for the `name(section)` tokens that headline page furniture:
/// the reference's `name(1) … name(1)` header corners and this
/// renderer's own `name(1)` label row.
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

/// The page head both renderers always lay down before any body row (the
/// reference's header or this renderer's `name(1)` label row). Only a
/// debug tripwire for the head window: recall is positional, never
/// lexical.
fn is_page_head(row: &str) -> bool {
    row.split(' ')
        .any(|token| is_section_token(token) || token == "()")
        || row.starts_with("UNTITLED")
}

/// The probe's row-grouping normalization: overstrike projection and
/// inline-whitespace collapse, then the position-window furniture drop —
/// row 0 (plus an optional wrapped center line) is the header, and on a
/// footed page the trailing non-blank block is the footer. Which words
/// share a physical row is the row machine's observable decision; blank
/// rows carry none of it, so unlike `escape_matrix` every blank row
/// drops. Body rows — all-caps section heads like `OPTIONS` included —
/// are content by position and never deleted by shape.
fn normalize(output: &str, has_footer: bool) -> Vec<String> {
    let mut projected = String::with_capacity(output.len());
    for character in output.chars() {
        if character == '\u{8}' {
            projected.pop();
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
    if has_footer {
        while rows.last().is_some_and(|row| !row.is_empty()) {
            rows.pop();
        }
    }
    rows.into_iter().filter(|row| !row.is_empty()).collect()
}

/// This renderer lays out no footer, so only the head window goes; the
/// trailing body block always stays.
fn normalize_mant(output: &str) -> Vec<String> {
    normalize(output, false)
}

#[test]
fn row_grouping_keeps_all_caps_and_adversarial_body_rows() {
    // The reference's own page shape: header, page-edge blank, body,
    // footer block. The all-caps section head is a body row and stays.
    let page = concat!(
        "P(1)                    General Commands Manual                    P(1)\n",
        "\n",
        "OPTIONS\n",
        "\n",
        "after\n",
        "space tail text\n",
        "\n",
        "Linux 6.18.40.1-microsoft-standard-WSL2\n",
        "                                  2026-09-29                          P(1)\n",
    );
    assert_eq!(
        normalize(page, true),
        ["OPTIONS", "after", "space tail text"],
        "the head window and the footer block go by position; every body row stays"
    );
    // Adversarial body shapes are content, not furniture: recall is
    // positional, so `printf(3)`, Linux-command, and date-shaped rows
    // survive verbatim inside the body.
    let extreme = concat!(
        "X(1)                    General Commands Manual                    X(1)\n",
        "\n",
        "OPTIONS\n",
        "Linux commands begin the body here.\n",
        "2026-09-30 printf(3)\n",
        "printf(3)\n",
        "\n",
        "Linux 6.18.40.1-microsoft-standard-WSL2\n",
        "                              September 30, 2026                       X(1)\n",
    );
    assert_eq!(
        normalize(extreme, true),
        [
            "OPTIONS",
            "Linux commands begin the body here.",
            "2026-09-30 printf(3)",
            "printf(3)"
        ]
    );
    // This renderer has no footer: the label row goes, the trailing body
    // block stays.
    assert_eq!(
        normalize_mant("p(1)\n\nOPTIONS\nafter\n"),
        ["OPTIONS", "after"]
    );
}

#[test]
fn definition_matrix_rows_match_the_pinned_reference() {
    let mut failures = Vec::new();
    let mut total = 0;
    let mut entries: Vec<_> = std::fs::read_dir(CASES)
        .expect("definition matrix case directory")
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
        total, 54,
        "case set changed; regen via scripts/regen_definition_matrix.sh"
    );
    assert!(
        failures.is_empty(),
        "row-grouping diffs:\n{}",
        failures.join("")
    );
}
