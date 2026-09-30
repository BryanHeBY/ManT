//! Escape/device-semantics matrix against pinned CVS mandoc `-Tutf8` output.
//!
//! Wave-1 regression corpus: native write receipts (`\z` BACKBEFORE retreat
//! over a word separator, term.c:901-908), single-device UTF-8 escape
//! semantics (`\:` buffers ASCII_NBRZW, chars.c:53 with term.c:631-632;
//! `\!`/`\?`/`\r` leave no footprint, roff_escape.c:156-160), the `\p`
//! pass rejections (term.c:143-146 with 233-237), and the generated run-in
//! cell overstrike order (term.c:901-908 through encode1(U+00A0)).
//!
//! Each case under `escape_matrix/cases/*.1` records the **row-grouped**
//! output of the pinned reference (`-Tutf8`), produced only by
//! `scripts/regen_escape_matrix.sh` — never by hand. Unlike
//! `definition_matrix` (recorded `-Tascii`), this matrix MUST stay UTF-8:
//! its cases sit exactly on the device fork. Row grouping preserves every
//! body row — section heads included — and every blank row between body
//! rows, so a lost or stray paragraph row fails the matrix. Only
//! `mandoc`'s page furniture goes: the `name(1)` title/header rows, the
//! footer OS and date rows, and the page-edge blank rows framing them.

use std::fmt::Write as _;

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/roff_lowering/escape_matrix/cases"
);

/// The footer's left-corner operating-system words (`mandoc` prints the
/// `.Os` default from `uname` when the macro is bare).
const FURNITURE_OS_WORDS: [&str; 11] = [
    "Linux", "macOS", "Darwin", "Apple", "Ubuntu", "Debian", "NetBSD", "FreeBSD",
    "OpenBSD", "AT&T", "GNU",
];

/// Month names `mandoc`'s footer date line carries for `%B`-style `.Dd`
/// dates (ISO dates are matched structurally instead).
const FURNITURE_MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];

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

/// The reference's header row (`name(1) … name(1)`) and this renderer's
/// bare `name(1)` label row.
fn is_title_row(row: &str) -> bool {
    let mut tokens = row.split(' ');
    match tokens.next() {
        Some(first) if is_section_token(first) => {
            tokens.next_back().is_none_or(is_section_token)
        }
        _ => false,
    }
}

/// The footer's operating-system corner row.
fn is_os_row(row: &str) -> bool {
    row.split(' ')
        .next()
        .is_some_and(|first| FURNITURE_OS_WORDS.contains(&first))
}

/// The footer's date corner: `September 30, 2026 name(1)` or
/// `2026-09-30 name(1)` for ISO `.Dd` dates.
fn is_date_row(row: &str) -> bool {
    let tokens: Vec<&str> = row.split(' ').collect();
    let [.., last] = tokens.as_slice() else {
        return false;
    };
    if !is_section_token(last) {
        return false;
    }
    let body = &tokens[..tokens.len() - 1];
    body.iter().any(|token| {
        let bytes = token.as_bytes();
        matches!(bytes, [y0, y1, y2, y3, b'-', m0, m1, b'-', d0, d1] if [
            *y0, *y1, *y2, *y3, *m0, *m1, *d0, *d1,
        ]
        .iter()
        .all(|byte| byte.is_ascii_digit()))
    }) || (body.iter().any(|token| FURNITURE_MONTHS.contains(token))
        && body
            .iter()
            .any(|token| token.len() == 4 && token.bytes().all(|byte| byte.is_ascii_digit())))
}

/// The probe's row-grouping normalization: overstrike projection and
/// inline-whitespace collapse, then drop only the page furniture — the
/// `name(1)` title/header rows, the footer OS and date rows, and the
/// page-edge blank rows that frame them. Body rows and blank rows between
/// body rows are paragraph structure and stay pinned: losing a body row,
/// losing or adding a blank row, or rendering a body-bearing page empty
/// all fail. The page-edge trim is symmetric because the header/footer
/// `term_vspace` blanks belong to the furniture, not the body, and this
/// single-device renderer lays out its own title-to-body gap.
fn normalize_mant(output: &str) -> Vec<String> {
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
        .filter(|row| !is_title_row(row) && !is_os_row(row) && !is_date_row(row))
        .collect();
    while rows.first().is_some_and(|row| row.is_empty()) {
        rows.remove(0);
    }
    while rows.last().is_some_and(|row| row.is_empty()) {
        rows.pop();
    }
    rows
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
        normalize_mant(page),
        ["NAME", "t - probe", "", "DESCRIPTION", "A"],
        "body rows, section heads, and the paragraph blank stay; the header, its blank, the footer blank, the OS row, and the date row go"
    );
    // ISO `.Dd` dates are furniture too.
    assert_eq!(normalize_mant("2026-09-30  x(1)"), Vec::<String>::new());
    // The renderer's own `name(1)` label row strips like the header, and
    // its title-gap blanks are page-edge layout, not paragraph structure.
    assert_eq!(normalize_mant("x(1)\n\n\nS\nX"), ["S", "X"]);
    // A body row that merely ends in a section token is content.
    assert_eq!(normalize_mant("see x(1)"), ["see x(1)"]);
    // Losing or adding a paragraph blank changes the pinned rows.
    assert_ne!(normalize_mant("S\n\nX"), normalize_mant("S\nX"));
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
        total, 65,
        "case set changed; regen via scripts/regen_escape_matrix.sh"
    );
    assert!(
        failures.is_empty(),
        "row-grouping diffs:\n{}",
        failures.join("")
    );
}
