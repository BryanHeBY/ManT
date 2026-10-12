//! Finite synopsis evidence, scoped to indented signature-only continuations.

use std::ops::Range;

use super::{command, scan};

/// A closed, unquoted optional group with an immediately authored option head.
/// POSIX `[ -x file ]`, `[[...]]`, expressions and command substitutions are
/// not evidence for this standalone synopsis rule.
pub(super) fn candidate(value: &str) -> bool {
    value.starts_with('[')
        && !value.starts_with("[[")
        && scan::option(value, 1).is_some()
        && group(value).is_some()
}

pub(super) fn rows(value: &str) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut active_indent = None;
    let mut byte = 0;
    for line in value.split_inclusive('\n') {
        let row = line.trim_end_matches(['\n', '\r']);
        let content = row.trim_start_matches([' ', '\t']);
        let indent = row.len() - content.len();
        let indentation = &row[..indent];
        let continuation = active_indent.is_some_and(|base: &str| {
            indentation.len() > base.len() && indentation.starts_with(base)
        }) && content.starts_with(['[', '<'])
            && signature(content).is_some();
        if continuation {
            rows.push(byte..byte + line.len());
        } else {
            active_indent = None;
            let command = content
                .strip_prefix("$ ")
                .or_else(|| content.strip_prefix("$\t"))
                .map_or(content, |rest| rest.trim_start_matches([' ', '\t']));
            if let Some(end) = command.find([' ', '\t']) {
                let start = byte + row.len() - command.len();
                if command::head(value, start, start + end)
                    && signature(&command[end..]) == Some(true)
                {
                    active_indent = Some(indentation);
                    rows.push(byte..byte + line.len());
                }
            }
        }
        byte += line.len();
    }
    rows
}

/// Accept only synopsis atoms, never executable shell/programming syntax.
/// The result records actual option-group evidence, not just any bracket pair.
fn signature(value: &str) -> Option<bool> {
    let mut option_group = false;
    let mut byte = 0;
    while byte < value.len() {
        let rest = &value[byte..];
        let character = rest.chars().next()?;
        if character.is_whitespace() {
            byte += character.len_utf8();
        } else if character == '[' {
            let length = group(rest)?;
            option_group |= candidate(rest);
            byte += length;
        } else if character == '<' {
            byte += scan::angle_placeholder(rest)?;
        } else if character == '=' && value[..byte].ends_with('>') && rest[1..].starts_with('<') {
            byte += 1;
        } else {
            let length = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let atom = &rest[..length];
            if !atom.chars().all(|character| {
                character.is_alphanumeric() || matches!(character, '_' | '-' | '.')
            }) {
                return None;
            }
            byte += length;
        }
    }
    Some(option_group)
}

/// Iterative and depth-bounded, shared by row evidence and group recognition.
fn group(value: &str) -> Option<usize> {
    if !value.starts_with('[') || value.starts_with("[[") {
        return None;
    }
    let mut depth = 0;
    let mut byte = 0;
    while byte < value.len() {
        let rest = &value[byte..];
        let character = rest.chars().next()?;
        match character {
            '[' => {
                depth += 1;
                if depth > scan::MAX_CONTEXT_DEPTH {
                    return None;
                }
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(byte + 1);
                }
            }
            '<' => {
                byte += scan::angle_placeholder(rest)?;
                continue;
            }
            '\n' | '\r' | '\'' | '"' | '`' | '\\' | ';' | '#' | '(' | ')' | '{' | '}' | '$'
            | '>' | '&' => return None,
            '|' if rest.starts_with("||") || rest.starts_with("|&") => return None,
            _ => {}
        }
        byte += character.len_utf8();
    }
    None
}
