//! Conservative command-head cues, without an executable catalogue or grammar.

use super::{keywords, scan};

pub(super) fn head(value: &str, start: usize, end: usize) -> bool {
    let word = &value[start..end];
    if !word
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
        || keywords::contains(word) && !builtin(word)
    {
        return false;
    }
    let prefix = value[..start].trim_end_matches([' ', '\t']);
    let prompt = prefix.ends_with('$') && scan::line_start(value, prefix.len() - 1);
    if !prompt && !scan::line_start(value, start) {
        return false;
    }
    let tail = value[end..].split('\n').next().unwrap_or_default();
    // Calls, declarations and expressions are not command heads.
    if !tail.is_empty() && !tail.starts_with([' ', '\t', '\r']) {
        return false;
    }
    let tail = tail.trim_start();
    if prompt {
        return true;
    }
    if builtin(word)
        && !tail.is_empty()
        && !tail.starts_with(['(', ')', '{', '}', ';', '=', '<', '>'])
    {
        return true;
    }
    evidence(tail)
}

fn builtin(word: &str) -> bool {
    matches!(
        word,
        "echo"
            | "eval"
            | "exec"
            | "export"
            | "getopts"
            | "local"
            | "printf"
            | "read"
            | "readonly"
            | "set"
            | "shift"
            | "source"
            | "test"
            | "trap"
            | "typeset"
            | "unset"
    )
}

fn evidence(row: &str) -> bool {
    let mut offset = 0;
    while offset < row.len() {
        let rest = &row[offset..];
        let first = rest.chars().next().expect("remaining argument");
        if first.is_whitespace() || matches!(first, '[' | ']' | '|') {
            offset += first.len_utf8();
        } else if matches!(first, '\'' | '"' | '`') {
            // Quoted flags and placeholders are data, not command evidence.
            offset += 1;
            while offset < row.len() {
                let c = row[offset..].chars().next().expect("remaining quote");
                offset += c.len_utf8();
                if c == '\\' {
                    offset += row[offset..].chars().next().map_or(0, char::len_utf8);
                } else if c == first {
                    break;
                }
            }
        } else if scan::option(row, offset).is_some()
            || scan::angle_placeholder(rest).is_some()
            || scan::placeholder(rest).is_some()
        {
            return true;
        } else if matches!(first, '#' | '(' | ')' | '{' | '}' | ';' | '=' | '<' | '>')
            || first == '\\'
            || rest.starts_with("//")
            || rest.starts_with("/*")
        {
            return false;
        } else {
            // Skip opaque argument atoms, including URL fragments and paths.
            offset += rest
                .char_indices()
                .find_map(|(i, c)| {
                    (c.is_whitespace() || matches!(c, '[' | ']' | '|' | '\'' | '"' | '`'))
                        .then_some(i)
                })
                .unwrap_or(rest.len());
        }
    }
    false
}
