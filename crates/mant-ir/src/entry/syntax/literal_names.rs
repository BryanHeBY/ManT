//! Source-neutral option names in one independently established literal head.
//!
//! The owner and its final visible form are producer evidence. This grammar
//! selects byte ranges within that form; it never creates an owner or divides
//! its display. In particular, punctuation after `=` belongs to an argument
//! unless another complete option starts after an authored separator.

use super::{lexical_option_token, literal_option_aliases, option_prefix};
use std::ops::Range;

/// Select bounded option spellings from one complete native declaration head.
/// The caller must bind every returned byte range to its original display.
#[must_use]
pub fn literal_option_names(form: &str) -> Vec<(String, Range<usize>)> {
    // Flow treats a slash as alias punctuation only inside one invocation
    // token. A slash following whitespace begins an operand/path instead.
    if !form
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0].is_ascii_whitespace() && pair[1] == b'/')
        && let Some(aliases) = literal_option_aliases(form)
    {
        return aliases;
    }
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut phase = Phase::Name;
    let mut closers = Vec::new();
    let mut uncertain = false;
    for (offset, character) in form.char_indices() {
        if matches!(character, ',' | '|')
            && !uncertain
            && closers.is_empty()
            && (phase == Phase::Name
                || form[offset + character.len_utf8()..]
                    .strip_prefix(char::is_whitespace)
                    .is_some_and(|tail| tail.trim_start().starts_with('-')))
        {
            ranges.push(start..offset);
            start = offset + character.len_utf8();
            phase = Phase::Name;
            continue;
        }
        match character {
            '=' => phase = Phase::Argument,
            '[' | '{' | '(' | '<' => {
                phase = Phase::Argument;
                if closers.len() == 64 {
                    uncertain = true;
                } else {
                    closers.push(match character {
                        '[' => ']',
                        '{' => '}',
                        '(' => ')',
                        '<' => '>',
                        _ => unreachable!(),
                    });
                }
            }
            ']' | '}' | ')' | '>' => {
                if closers.last() == Some(&character) {
                    closers.pop();
                } else {
                    uncertain = true;
                }
            }
            _ => {}
        }
    }
    ranges.push(start..form.len());
    let mut names = Vec::new();
    for range in ranges {
        let Some(group) = form.get(range.clone()) else {
            continue;
        };
        let leading = group.len() - group.trim_start().len();
        let group = group.trim();
        if group.is_empty() {
            continue;
        }
        let offset = range.start + leading;
        let before = names.len();
        if let Some(slash) = slash_names(group, offset) {
            names.extend(slash);
        } else if let Some(pattern) = pattern_names(group, offset) {
            names.extend(pattern);
        } else if let Some((name, found)) = leading_name(group, offset) {
            names.push((name, found));
        }
        if names.len() > 64 {
            names.truncate(before);
            break;
        }
    }
    names
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Name,
    Argument,
}

fn leading_name(group: &str, offset: usize) -> Option<(String, Range<usize>)> {
    let token = group.split_whitespace().next()?;
    let token = token.trim_matches(['[', ']', '(', ')', '{', '}', '“', '”', '‘', '’']);
    let start = offset + token.as_ptr() as usize - group.as_ptr() as usize;
    let name = option_prefix(token)?;
    if !lexical_option_token(name) {
        return None;
    }
    let suffix = &token[name.len()..];
    if !suffix.is_empty() && !suffix.starts_with(['=', '[', '{', '<', '(', '/']) {
        return None;
    }
    Some((name.to_owned(), start..start + name.len()))
}

fn slash_names(group: &str, offset: usize) -> Option<Vec<(String, Range<usize>)>> {
    let token = group
        .split_whitespace()
        .next()?
        .trim_matches(['[', ']', '(', ')', '{', '}', '“', '”', '‘', '’']);
    if !token.contains('/') {
        return None;
    }
    let parts = token.split('/').collect::<Vec<_>>();
    if parts.len() < 2
        || !parts[..parts.len() - 1]
            .iter()
            .all(|part| lexical_option_token(part))
    {
        return None;
    }
    let mut result = Vec::with_capacity(parts.len());
    let mut position = offset + token.as_ptr() as usize - group.as_ptr() as usize;
    for (index, part) in parts.iter().enumerate() {
        let name = option_prefix(part)?;
        if !lexical_option_token(name)
            || index + 1 != parts.len() && name != *part
            || index + 1 == parts.len() && name != *part && !part[name.len()..].starts_with('=')
        {
            return None;
        }
        result.push((name.to_owned(), position..position + name.len()));
        position += part.len() + 1;
    }
    Some(result)
}

fn pattern_names(group: &str, offset: usize) -> Option<Vec<(String, Range<usize>)>> {
    let tokens = group.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 2
        || !tokens[0].starts_with('-')
        || !tokens[0].contains('#')
        || !tokens[0]
            .chars()
            .all(|character| matches!(character, '-' | '#'))
        || !tokens[1..]
            .iter()
            .all(|token| token.starts_with("--") && lexical_option_token(token))
    {
        return None;
    }
    Some(
        tokens[1..]
            .iter()
            .map(|token| {
                let start = offset + token.as_ptr() as usize - group.as_ptr() as usize;
                ((*token).to_owned(), start..start + token.len())
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::literal_option_names;

    #[test]
    fn names_remain_separate_from_attached_values_and_unproved_aliases() {
        // Exact TP heads first ran pinned CVS -Tutf8. man_macro.c::blk_imp
        // retains one HEAD; man_term.c::pre_TP prints its label before BODY.
        assert_eq!(
            literal_option_names("--width=NUMBER"),
            [("--width".into(), 0..7)]
        );
        assert_eq!(
            literal_option_names("--output=FILE"),
            [("--output".into(), 0..8)]
        );
        assert_eq!(
            literal_option_names("--set=KEY,VALUE"),
            [("--set".into(), 0..5)]
        );
        assert_eq!(
            literal_option_names("-f, --file=ARCHIVE"),
            [("-f".into(), 0..2), ("--file".into(), 4..10)]
        );
        assert_eq!(literal_option_names("-a, text"), [("-a".into(), 0..2)]);
        assert_eq!(
            literal_option_names("--foo --bar"),
            [("--foo".into(), 0..5)]
        );
        assert_eq!(
            literal_option_names("-o/path/--help"),
            [("-o".into(), 0..2)]
        );
        assert_eq!(literal_option_names("-o /-NUM"), [("-o".into(), 0..2)]);
        assert_eq!(literal_option_names("-a / --all"), [("-a".into(), 0..2)]);
        assert_eq!(
            literal_option_names("[-n/--number]"),
            [("-n".into(), 1..3), ("--number".into(), 4..12)]
        );
        assert_eq!(
            literal_option_names("-a, -a"),
            [("-a".into(), 0..2), ("-a".into(), 4..6)]
        );
        assert!(literal_option_names("-1").is_empty());
        assert!(literal_option_names("FILE").is_empty());
    }
}
