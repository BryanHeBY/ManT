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
    // The shared alias grammar admits whitespace-separated short/long names
    // but keeps a slash after whitespace with a path-like operand.
    if let Some(aliases) = literal_option_aliases(form) {
        return aliases;
    }
    let ranges = literal_declaration_ranges(form);
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

/// Bounded declaration intervals in one final visible head. A delimiter in
/// a quoted or bracketed argument is not a new declaration. Both native
/// component evidence and source-neutral literal spelling use these ranges.
pub(crate) fn literal_declaration_ranges(form: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut phase = Phase::Name;
    let mut name_end = declaration_name_end(form, start);
    let mut after_name_space = false;
    let mut closers = Vec::new();
    let mut quote = None;
    let mut uncertain = false;
    for (offset, character) in form.char_indices() {
        // A complete name followed by a separated non-option begins an
        // ordinary parameter. Flow's declaration state makes the same
        // transition after a literal name; punctuation inside that parameter
        // cannot create another name. A later separator followed by a fresh
        // option can still begin an independent declaration.
        if phase == Phase::Name && name_end.is_some_and(|end| offset >= end) {
            if character.is_whitespace() {
                after_name_space = true;
            } else if after_name_space {
                if declaration_name_end(form, offset).is_none() {
                    phase = Phase::Argument;
                }
                after_name_space = false;
            }
        }
        if let Some(close) = quote {
            if character == close {
                quote = None;
            }
            continue;
        }
        let previous = form[..offset].chars().next_back();
        if let Some(close) = match character {
            '"' => Some('"'),
            '\'' if previous.is_none_or(|before| {
                before.is_whitespace() || matches!(before, '=' | ',' | '|' | '[' | '{' | '(')
            }) =>
            {
                Some('\'')
            }
            '“' => Some('”'),
            '‘' => Some('’'),
            _ => None,
        } {
            quote = Some(close);
            phase = Phase::Argument;
            continue;
        }
        if matches!(character, ',' | '|') && !uncertain && closers.is_empty() {
            // Looking past a separator may scan whitespace. Do this only at
            // a separator, never for every scalar in a long literal head.
            let remainder = &form[offset + character.len_utf8()..];
            let next_name_end = declaration_name_end(form, offset + character.len_utf8());
            let fresh_option =
                remainder.starts_with(char::is_whitespace) && next_name_end.is_some();
            if phase == Phase::Name && name_end.is_some() || fresh_option {
                ranges.push(start..offset);
                start = offset + character.len_utf8();
                phase = if next_name_end.is_some() {
                    Phase::Name
                } else {
                    Phase::Argument
                };
                name_end = next_name_end;
                after_name_space = false;
                continue;
            }
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
    ranges
}

/// End of the leading option spelling, before any attached value or visible
/// declaration separator. This is a syntax boundary, not a source coordinate
/// or proof that the native owner represents an option.
fn declaration_name_end(form: &str, start: usize) -> Option<usize> {
    let remainder = form.get(start..)?;
    let leading = remainder.len() - remainder.trim_start().len();
    let head = remainder.trim_start();
    let token_end = head
        .find(|character: char| character.is_whitespace() || matches!(character, ',' | '|'))
        .unwrap_or(head.len());
    leading_name(&head[..token_end], start + leading).map(|(_, range)| range.end)
}

/// Require an inferred PP/RS head to be complete declaration syntax, not
/// merely to start with an option-looking word. The native continuation
/// proves presentation ownership; this source-neutral rule prevents ordinary
/// prose in that same paragraph from becoming an entry. It follows the
/// bounded bare-argument rule of Flow's `is_option_head` without treating
/// visual adjacency as an alias or reconstructing roff markup.
#[must_use]
pub fn is_complete_hanging_option_head(form: &str) -> bool {
    let names = literal_option_names(form);
    let Some((_, first)) = names.first() else {
        return false;
    };
    if !form[..first.start]
        .chars()
        .all(|character| character.is_whitespace() || matches!(character, '[' | '{' | '('))
    {
        return false;
    }
    for pair in names.windows(2) {
        let [(previous, previous_range), (next, next_range)] = pair else {
            unreachable!()
        };
        let Some(gap) = form.get(previous_range.end..next_range.start) else {
            return false;
        };
        let gap = gap.trim();
        if gap.is_empty() || gap == "or" {
            if previous.len() != 2 || !previous.starts_with('-') || !next.starts_with("--") {
                return false;
            }
        } else if let Some(prefix) = gap.strip_suffix([',', '|', '/']) {
            if !hanging_argument_tail(prefix) {
                return false;
            }
        } else {
            return false;
        }
    }
    names
        .last()
        .and_then(|(_, range)| form.get(range.end..))
        .is_some_and(hanging_argument_tail)
}

fn hanging_argument_tail(value: &str) -> bool {
    let mut tail = value.trim();
    if !tail.is_empty()
        && tail
            .chars()
            .all(|character| matches!(character, ']' | '}' | ')'))
    {
        return true;
    }
    if tail.starts_with([',', '|', '/']) {
        return false;
    }
    if let Some(attached) = tail.strip_prefix('=') {
        let Some(token) = attached.split_whitespace().next() else {
            return false;
        };
        if token.is_empty() || token.chars().any(char::is_control) {
            return false;
        }
        tail = attached[token.len()..].trim_start();
    }
    let mut bare = 0usize;
    let mut closers = Vec::new();
    for token in tail.split_whitespace() {
        let inside = !closers.is_empty();
        for character in token.chars() {
            if let Some(closer) = match character {
                '[' => Some(']'),
                '{' => Some('}'),
                '<' => Some('>'),
                '(' => Some(')'),
                _ => None,
            } {
                if closers.len() == 64 {
                    return false;
                }
                closers.push(closer);
            } else if matches!(character, ']' | '}' | '>' | ')') && closers.pop() != Some(character)
            {
                return false;
            }
        }
        if inside || token.starts_with(['[', '{', '<', '(']) || token == "..." {
            continue;
        }
        if token.starts_with('/')
            || token
                .chars()
                .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-'))
                && !token.starts_with('-')
        {
            bare += 1;
        } else {
            return false;
        }
    }
    closers.is_empty() && bare <= 1
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
    use super::{
        is_complete_hanging_option_head, literal_declaration_ranges, literal_option_names,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn visible_quoted_argument_does_not_restart_a_declaration() {
        // Both exact .IP inputs ran pinned CVS -Tutf8 first. man_term.c::
        // pre_IP prints the sole label operand, and term.c::term_word emits
        // \(dq as visible quotes around one parameter containing commas.
        let argument = "--pattern \"one,--fake,two\"";
        let ranges = literal_declaration_ranges(argument);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], 0..argument.len());
        assert_eq!(literal_option_names(argument), [("--pattern".into(), 0..9)]);

        let followed = "--pattern \"one,--fake,two\", --all";
        let start = followed.find("--all").unwrap();
        assert_eq!(literal_declaration_ranges(followed).len(), 2);
        assert_eq!(
            literal_option_names(followed),
            [
                ("--pattern".into(), 0..9),
                ("--all".into(), start..start + 5)
            ]
        );

        // term.c::term_word prints plain apostrophes literally. Only an
        // apostrophe beginning an argument opens a quoted interval; an
        // apostrophe inside a word does not consume later declarations.
        let single = "--pattern 'one,--fake,two', --all";
        let start = single.find("--all").unwrap();
        assert_eq!(literal_declaration_ranges(single).len(), 2);
        assert_eq!(
            literal_option_names(single),
            [
                ("--pattern".into(), 0..9),
                ("--all".into(), start..start + 5)
            ]
        );

        let adjacent = "--pattern,'one,--fake,two'";
        assert_eq!(literal_option_names(adjacent), [("--pattern".into(), 0..9)]);
    }

    #[test]
    fn ordinary_argument_punctuation_does_not_restart_a_declaration() {
        // These exact .IP labels first ran pinned CVS -Tutf8. man_term.c::
        // pre_IP emits one label operand; term.c::term_word keeps the plain
        // parameter and its punctuation after the bold --list spelling.
        for form in [
            "--list first,--fake,last",
            "--list, first,--fake,last",
            "--list first|--fake|last",
        ] {
            assert_eq!(
                literal_option_names(form),
                [("--list".into(), 0..6)],
                "{form}"
            );
        }
        let followed = "--list first,--fake,last, --all";
        let start = followed.find("--all").unwrap();
        assert_eq!(
            literal_option_names(followed),
            [("--list".into(), 0..6), ("--all".into(), start..start + 5)]
        );
    }

    #[test]
    fn negative_number_parameter_does_not_start_a_declaration() {
        // All three exact .IP labels ran pinned CVS -Tutf8 first. Under
        // man_term.c::pre_IP the visible -10 is still part of one HEAD; it
        // does not license a name inside its comma-separated parameter.
        for form in ["--number -10,--fake,20", "--number, -10,--fake,20"] {
            assert_eq!(literal_option_names(form), [("--number".into(), 0..8)]);
        }
        let followed = "--number -10,--fake,20, --all";
        let start = followed.find("--all").unwrap();
        assert_eq!(
            literal_option_names(followed),
            [
                ("--number".into(), 0..8),
                ("--all".into(), start..start + 5)
            ]
        );
    }

    #[test]
    fn long_plain_argument_gap_does_not_rescan_suffix_per_scalar() {
        // The exact .IP head with 8192 spaces ran pinned CVS -Tutf8 first.
        // man_term.c::pre_IP prints that one HEAD; the syntax scan must not
        // repeatedly inspect its remaining whitespace between separators.
        let form = format!("--list {}first,--fake,last", " ".repeat(8192));
        let started = Instant::now();
        assert_eq!(literal_option_names(&form), [("--list".into(), 0..6)]);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "long literal head was rescanned per scalar"
        );
    }

    #[test]
    fn hanging_heads_require_complete_declaration_syntax() {
        // Each spelling was first executed in a minimal PP/B/RS input with
        // pinned CVS -Tutf8. The formatter keeps the whole PP presentation
        // head; this source-neutral rule alone decides whether it is a name.
        for accepted in [
            "--git-dir",
            "--output FILE",
            "--git-dir=path",
            "-a, --all",
            "--foo [=FILE]",
        ] {
            assert!(is_complete_hanging_option_head(accepted), "{accepted}");
        }
        for rejected in [
            "--git-dir intervening text",
            "--foo --bar",
            "-a / --all",
            "ordinary prose",
        ] {
            assert!(!is_complete_hanging_option_head(rejected), "{rejected}");
        }
    }

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
