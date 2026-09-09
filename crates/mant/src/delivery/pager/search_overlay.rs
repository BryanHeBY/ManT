//! Search decorates visible ranges without moving any source escape sequence.
use super::sgr::SgrState;
use regex::Regex;

pub(super) fn highlight(line: &str, query: &Regex, escapes: &Regex) -> String {
    let visible = escapes.replace_all(line, "");
    let mut matches = query
        .find_iter(&visible)
        .filter(|m| !m.is_empty())
        .peekable();
    if matches.peek().is_none() {
        return line.to_owned();
    }
    let mut sequences = escapes.find_iter(line).peekable();
    let mut native = SgrState::default();
    let mut result = String::with_capacity(line.len());
    let (mut raw, mut position, mut active) = (0, 0, false);
    while raw < line.len() {
        if sequences.peek().is_some_and(|escape| escape.start() == raw) {
            let escape = sequences.next().expect("peeked escape");
            result.push_str(escape.as_str());
            native.scan(escape.as_str());
            // A source reset inside a match must not cancel the overlay.
            if active && !native.reversed() {
                result.push_str("\x1b[7m");
            }
            raw = escape.end();
            continue;
        }
        if active && matches.peek().is_some_and(|m| m.end() == position) {
            if !native.reversed() {
                result.push_str("\x1b[27m");
            }
            active = false;
            matches.next();
        }
        if matches.peek().is_some_and(|m| m.start() == position) {
            if !native.reversed() {
                result.push_str("\x1b[7m");
            }
            active = true;
        }
        let scalar = line[raw..].chars().next().expect("remaining scalar");
        result.push(scalar);
        raw += scalar.len_utf8();
        position += scalar.len_utf8();
    }
    if active && !native.reversed() {
        result.push_str("\x1b[27m");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn states(text: &str, escapes: &Regex, state: &mut SgrState) -> Vec<(char, String)> {
        let mut offset = 0;
        let mut out = Vec::new();
        for escape in escapes.find_iter(text) {
            for c in text[offset..escape.start()].chars() {
                out.push((c, state.logical_line("").into_owned()));
            }
            state.scan(escape.as_str());
            offset = escape.end();
        }
        for c in text[offset..].chars() {
            out.push((c, state.logical_line("").into_owned()));
        }
        out
    }
    #[test]
    fn role_color_stays_before_unicode_glyphs_and_resets_do_not_end_matches() {
        let escapes = Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap();
        let query = Regex::new("日本z+").unwrap();
        for source in [
            "\x1b[92m日本zz\x1b[0m",
            "\x1b[92m日\x1b[0m本zz",
            "日\x1b[34m本zz",
        ] {
            let result = highlight(source, &query, &escapes);
            assert_eq!(escapes.replace_all(&result, ""), "日本zz");
            let mut original_state = SgrState::default();
            let mut result_state = SgrState::default();
            let before = states(source, &escapes, &mut original_state);
            let after = states(&result, &escapes, &mut result_state);
            for ((a, expected), (b, actual)) in before.iter().zip(after) {
                assert_eq!(*a, b);
                assert!(actual.contains("\x1b[7m"));
                assert_eq!(
                    actual.replace("\x1b[7m", ""),
                    if expected.is_empty() {
                        "\x1b[0m"
                    } else {
                        expected
                    }
                );
            }
        }
    }
}
