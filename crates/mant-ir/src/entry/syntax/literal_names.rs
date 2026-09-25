//! Source-neutral option names in one independently established literal head.
//!
//! The owner and its final visible form are producer evidence. This grammar
//! selects byte ranges within that form; it never creates an owner or divides
//! its display. In particular, punctuation after `=` belongs to an argument
//! unless another complete option starts after an authored separator.

use super::{lexical_option_token, literal_option_aliases, option_prefix};
use std::ops::Range;

type NameRange = (String, Range<usize>);

/// Select bounded option spellings from one complete native declaration head.
/// The caller must bind every returned byte range to its original display.
#[must_use]
pub fn literal_option_names(form: &str) -> Vec<(String, Range<usize>)> {
    // The shared alias grammar admits whitespace-separated short/long names
    // but keeps a slash after whitespace with a path-like operand.
    if let Some(aliases) = literal_option_aliases(form) {
        return aliases;
    }
    declaration_scan(form, &[], &[], StyledBoundaryRule::SingleTextOperand)
        .names(form)
        .0
}

/// A native BI/BR operand boundary is known independently of its final font.
/// A single IP label has no such boundary; text punctuation cannot prove that
/// an already active styled argument has ended.
#[derive(Clone, Copy)]
pub(crate) enum StyledBoundaryRule {
    NativeComponents,
    SingleTextOperand,
}

/// The one text/state pass retains both complete declaration ranges and the
/// part of each range that was still eligible to contain names. In particular,
/// a styled parameter cannot be reparsed later as a fresh text-only head.
#[derive(Debug)]
pub(crate) struct DeclarationScan {
    pub(crate) ranges: Vec<Range<usize>>,
    name_prefixes: Vec<Range<usize>>,
}

impl DeclarationScan {
    fn push(&mut self, start: usize, end: usize, prefix_end: Option<usize>) {
        self.ranges.push(start..end);
        self.name_prefixes
            .push(start..prefix_end.unwrap_or(end).min(end));
    }

    /// Parse only disjoint, already eligible prefixes. The stateful scan has
    /// excluded every argument byte before this small spelling pass begins.
    pub(crate) fn names(&self, form: &str) -> (Vec<(String, Range<usize>)>, bool) {
        let mut names = Vec::new();
        for prefix in &self.name_prefixes {
            let Some(value) = form.get(prefix.clone()) else {
                continue;
            };
            let leading = value.len() - value.trim_start().len();
            let group = value.trim();
            if group.is_empty() {
                continue;
            }
            let offset = prefix.start + leading;
            let selected = if let Some(aliases) = literal_option_aliases(group) {
                aliases
                    .into_iter()
                    .map(|(name, range)| (name, offset + range.start..offset + range.end))
                    .collect()
            } else {
                match slash_names(group, offset) {
                    Err(()) => return (names, true),
                    Ok(Some(slash)) => slash,
                    Ok(None) => {
                        if let Some(pattern) = pattern_names(group, offset) {
                            pattern
                        } else {
                            leading_name(group, offset).into_iter().collect()
                        }
                    }
                }
            };
            if names.len() + selected.len() > 64 {
                return (names, true);
            }
            names.extend(selected);
        }
        (names, false)
    }
}

/// Bounded declaration intervals in one final visible head. A delimiter in
/// a quoted or bracketed argument is not a new declaration. Both native
/// component evidence and source-neutral literal spelling use these ranges.
#[cfg(test)]
pub(crate) fn literal_declaration_ranges(form: &str) -> Vec<Range<usize>> {
    declaration_scan(form, &[], &[], StyledBoundaryRule::SingleTextOperand).ranges
}

/// An independent native/styled declaration may begin directly after a
/// parameter's terminal delimiter. The starts are final displayed byte
/// offsets, in ascending order. An operand boundary is evidence for a new
/// styled declaration only when the surrounding quote/bracket grammar is
/// already closed: `man_term.c::pre_alternate()` changes font but does not
/// close an authored parameter. Keep the text-only contract above for callers
/// with no such evidence.
pub(crate) fn literal_declaration_scan_with_starts(
    form: &str,
    independent_starts: &[usize],
    argument_starts: &[usize],
    boundary_rule: StyledBoundaryRule,
) -> DeclarationScan {
    declaration_scan(form, independent_starts, argument_starts, boundary_rule)
}

#[cfg(test)]
pub(crate) fn literal_declaration_ranges_with_starts(
    form: &str,
    independent_starts: &[usize],
    argument_starts: &[usize],
) -> Vec<Range<usize>> {
    declaration_scan(
        form,
        independent_starts,
        argument_starts,
        StyledBoundaryRule::NativeComponents,
    )
    .ranges
}

#[expect(
    clippy::too_many_lines,
    reason = "keep the bounded, single-pass declaration state transitions together"
)]
fn declaration_scan(
    form: &str,
    independent_starts: &[usize],
    argument_starts: &[usize],
    boundary_rule: StyledBoundaryRule,
) -> DeclarationScan {
    let mut scan = DeclarationScan {
        ranges: Vec::new(),
        name_prefixes: Vec::new(),
    };
    let mut start = 0;
    let mut prefix_end = None;
    let mut phase = Phase::Name;
    let mut name_end = declaration_name_end(form, start);
    let mut after_name_space = false;
    let mut closers = Vec::new();
    let mut quote = None;
    let mut uncertain = false;
    let mut independent = 0usize;
    let mut argument = 0usize;
    for (offset, character) in form.char_indices() {
        while argument_starts
            .get(argument)
            .is_some_and(|&evidence| evidence < offset)
        {
            argument += 1;
        }
        if argument_starts.get(argument) == Some(&offset) {
            // A nonempty final underlined operand is already an argument.
            // Without this transition, a glued `-Lfirst` looks like one
            // option token and its following comma could manufacture names
            // before the style check has a chance to reject them.
            if phase == Phase::Name {
                prefix_end.get_or_insert(offset);
            }
            phase = Phase::StyledArgument;
            name_end = None;
            after_name_space = false;
        }
        // A complete name followed by a separated non-option begins an
        // ordinary parameter. Flow's declaration state makes the same
        // transition after a literal name; punctuation inside that parameter
        // cannot create another name. A later separator followed by a fresh
        // option can still begin an independent declaration.
        if phase == Phase::Name && name_end.is_some_and(|end| offset >= end) {
            if character.is_whitespace() {
                after_name_space = true;
            } else if after_name_space {
                if declaration_name_end(form, offset).is_none()
                    && !is_short_long_connector(form, start, offset)
                {
                    prefix_end.get_or_insert(offset);
                    phase = Phase::Argument;
                }
                after_name_space = false;
            }
        }
        // An independent native/styled declaration can restart only after
        // the complete authored quote/bracket scope has ended. Alternating
        // font operands are not parameter boundaries in pinned CVS.
        let mut proved_start = None;
        if matches!(character, ',' | '|') {
            let next_offset = offset + character.len_utf8();
            let remainder = &form[next_offset..];
            let candidate_start = next_offset + remainder.len() - remainder.trim_start().len();
            while independent_starts
                .get(independent)
                .is_some_and(|&evidence| evidence < candidate_start)
            {
                independent += 1;
            }
            let state_closed = quote.is_none() && closers.is_empty() && !uncertain;
            if matches!(boundary_rule, StyledBoundaryRule::NativeComponents)
                && independent_starts.get(independent) == Some(&candidate_start)
                && state_closed
            {
                proved_start = declaration_name_end(form, next_offset);
            }
        }
        if let Some(next_name_end) = proved_start {
            scan.push(start, offset, prefix_end);
            start = offset + character.len_utf8();
            prefix_end = None;
            phase = Phase::Name;
            name_end = Some(next_name_end);
            after_name_space = false;
            continue;
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
            if phase != Phase::StyledArgument {
                if phase == Phase::Name {
                    prefix_end.get_or_insert(offset);
                }
                phase = Phase::Argument;
            }
            continue;
        }
        if matches!(character, ',' | '|') && !uncertain && closers.is_empty() {
            // Looking past a separator may scan whitespace. Do this only at
            // a separator, never for every scalar in a long literal head.
            let next_offset = offset + character.len_utf8();
            let remainder = &form[next_offset..];
            let next_name_end = declaration_name_end(form, next_offset);
            let fresh_option =
                remainder.starts_with(char::is_whitespace) && next_name_end.is_some();
            // A styled argument stays opaque even if its internal punctuation
            // is followed by whitespace and a bold run. A plain text head
            // still permits comma+space declaration syntax before entering a
            // styled parameter; only native components can restart afterward.
            if phase == Phase::Name && name_end.is_some()
                || phase != Phase::StyledArgument && fresh_option
            {
                scan.push(start, offset, prefix_end);
                start = next_offset;
                prefix_end = None;
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
            '=' if phase != Phase::StyledArgument => {
                if phase == Phase::Name {
                    prefix_end.get_or_insert(offset);
                }
                phase = Phase::Argument;
            }
            '[' | '{' | '(' | '<' => {
                if phase != Phase::StyledArgument {
                    // An opening enclosure before a visible option (for
                    // example `[-n/--number]`) belongs to the declaration
                    // prefix. A bracket after the name begins its value.
                    if phase == Phase::Name && name_end.is_some_and(|end| offset >= end) {
                        prefix_end.get_or_insert(offset);
                    }
                    phase = Phase::Argument;
                }
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
    scan.push(start, form.len(), prefix_end);
    scan
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
    let token = &head[..token_end];
    if pattern_start(token) {
        // A pattern proves a provisional declaration boundary, not a name.
        // Subsequent ordinary operands must still enter Argument phase so a
        // comma inside one cannot restart at a fake option.
        return Some(start + leading + token.len());
    }
    leading_name(token, start + leading).map(|(_, range)| range.end)
}

/// The shared alias grammar admits exactly `-q or --quiet`; `or` is not an
/// ordinary argument when it bridges one short option and one long option.
/// Look ahead only at this one candidate connector, never at every glyph.
fn is_short_long_connector(form: &str, segment_start: usize, offset: usize) -> bool {
    let Some(before) = form.get(segment_start..offset) else {
        return false;
    };
    let first = before.trim();
    if first.len() != 2 || !first.starts_with('-') || !lexical_option_token(first) {
        return false;
    }
    let Some(after_or) = form
        .get(offset..)
        .and_then(|value| value.strip_prefix("or"))
    else {
        return false;
    };
    let skipped = after_or.len() - after_or.trim_start().len();
    if skipped == 0 {
        return false;
    }
    let next = offset + 2 + skipped;
    declaration_name_end(form, next)
        .and_then(|end| form.get(next..end))
        .is_some_and(|candidate| candidate.starts_with("--"))
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
    StyledArgument,
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

fn slash_names(group: &str, offset: usize) -> Result<Option<Vec<NameRange>>, ()> {
    let Some(token) = group.split_whitespace().next() else {
        return Ok(None);
    };
    let token = token.trim_matches(['[', ']', '(', ')', '{', '}', '“', '”', '‘', '’']);
    if !token.contains('/') {
        return Ok(None);
    }
    // Keep the temporary split bounded by the same 64-name ceiling as the
    // final result. An overlong candidate cannot become a checked name.
    let parts = token.split('/').take(66).collect::<Vec<_>>();
    if parts.len() > 65 {
        return Err(());
    }
    if parts.len() < 2
        || !parts[..parts.len() - 1]
            .iter()
            .all(|part| lexical_option_token(part))
    {
        return Ok(None);
    }
    let mut result = Vec::with_capacity(parts.len());
    let mut position = offset + token.as_ptr() as usize - group.as_ptr() as usize;
    for (index, part) in parts.iter().enumerate() {
        let Some(name) = option_prefix(part) else {
            return Ok(None);
        };
        if !lexical_option_token(name)
            || index + 1 != parts.len() && name != *part
            || index + 1 == parts.len() && name != *part && !part[name.len()..].starts_with('=')
        {
            return Ok(None);
        }
        result.push((name.to_owned(), position..position + name.len()));
        position += part.len() + 1;
    }
    if result.len() > 64 {
        return Err(());
    }
    Ok(Some(result))
}

fn pattern_names(group: &str, offset: usize) -> Option<Vec<(String, Range<usize>)>> {
    let mut tokens = group.split_whitespace();
    if !pattern_start(tokens.next()?) {
        return None;
    }
    let mut names = Vec::new();
    for token in tokens {
        let start = offset + token.as_ptr() as usize - group.as_ptr() as usize;
        let Some((name, range)) = leading_name(token, start) else {
            break;
        };
        if !name.starts_with("--") {
            break;
        }
        let attached = range.end < start + token.len();
        names.push((name, range));
        // An assignment or bracketed value ends the provisional name group.
        // Later option-looking words need their own proved declaration edge.
        if attached || names.len() == 64 {
            break;
        }
    }
    (!names.is_empty()).then_some(names)
}

fn pattern_start(token: &str) -> bool {
    token.starts_with('-')
        && token.contains('#')
        && token
            .chars()
            .all(|character| matches!(character, '-' | '#'))
}

#[cfg(test)]
mod tests {
    use super::{
        StyledBoundaryRule::{NativeComponents, SingleTextOperand},
        is_complete_hanging_option_head, literal_declaration_ranges,
        literal_declaration_ranges_with_starts, literal_declaration_scan_with_starts,
        literal_option_names,
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
    fn native_parameter_interval_blocks_internal_font_changed_option_spelling() {
        // This exact TP/BI head with `first,\fB--fake\fI,last,` ran pinned
        // CVS -Thtml first. man_term.c::pre_alternate keeps one italic operand
        // even when term.c::term_word changes fonts inside it; only the later
        // bold operand is an independent declaration candidate.
        let form = "-Lfirst,--fake,last,--all FILE";
        let all = form.find("--all").unwrap();
        assert_eq!(
            literal_declaration_ranges_with_starts(form, &[all], &[2]),
            [0..all - 1, all..form.len()]
        );
        assert_eq!(
            literal_declaration_scan_with_starts(form, &[all], &[2], NativeComponents)
                .names(form)
                .0,
            [("-L".into(), 0..2), ("--all".into(), all..all + 5)]
        );
        // Whitespace inside the same underlined native operand is not a
        // fresh declaration either; an independent later bold operand is.
        let spaced = "-Lfirst, --fake, last,--all FILE";
        let all = spaced.find("--all").unwrap();
        assert_eq!(
            literal_declaration_ranges_with_starts(spaced, &[all], &[2]),
            [0..all - 1, all..spaced.len()]
        );
        // A BI operand switches font, not authored quote/bracket scope. Each
        // exact TP/BI input ran pinned CVS -Tutf8 before this assertion.
        for argument in ["(first,--fake,", "\"first,--fake,"] {
            let form = format!("-L{argument}--all FILE");
            let all = form.find("--all").unwrap();
            assert_eq!(
                literal_declaration_ranges_with_starts(&form, &[all], &[2]),
                std::iter::once(0..form.len()).collect::<Vec<_>>()
            );
            assert_eq!(
                literal_declaration_ranges_with_starts(&form, &[], &[2]),
                std::iter::once(0..form.len()).collect::<Vec<_>>(),
                "no operand may reset {argument}"
            );
            assert_eq!(
                literal_declaration_scan_with_starts(&form, &[all], &[2], NativeComponents)
                    .names(&form)
                    .0,
                [("-L".into(), 0..2)]
            );
        }
        // Quote/parenthesis closure in a later operand, however, permits
        // the next proved declaration after its terminal comma.
        for argument in ["(first,--fake,last),", "\"first,--fake,last\","] {
            let form = format!("-L{argument}--all FILE");
            let all = form.find("--all").unwrap();
            assert_eq!(
                literal_declaration_scan_with_starts(&form, &[all], &[2], NativeComponents)
                    .names(&form)
                    .0,
                [("-L".into(), 0..2), ("--all".into(), all..all + 5)]
            );
        }
        // The real cross-operand quote case includes a bold --fake operand
        // inside the same quoted argument; only the post-quote --all is a
        // declaration. Pinned CVS man_term.c::pre_alternate retains the
        // operand font switch without ending the quote.
        let form = "--pattern \"first,--fake,last\",--all FILE";
        let fake = form.find("--fake").unwrap();
        let all = form.find("--all").unwrap();
        assert_eq!(
            literal_declaration_scan_with_starts(form, &[fake, all], &[10], NativeComponents)
                .names(form)
                .0,
            [("--pattern".into(), 0..9), ("--all".into(), all..all + 5)]
        );
        for argument in ["first|--fake|last,", "-10,--fake,20,", "first/--fake/last,"] {
            let form = format!("-L{argument}--all FILE");
            let all = form.find("--all").unwrap();
            assert_eq!(
                literal_declaration_scan_with_starts(&form, &[all], &[2], NativeComponents)
                    .names(&form)
                    .0,
                [("-L".into(), 0..2), ("--all".into(), all..all + 5)],
                "{argument}"
            );
        }
    }

    #[test]
    fn checked_scan_keeps_the_short_or_long_connector() {
        // Both exact `.B -q or --quiet` and styled `.IP` counterparts ran
        // pinned CVS -Tutf8 first. man_term.c::pre_B/pre_IP emit the complete
        // visible head; `or` connects two names rather than beginning an
        // ordinary argument in the shared declaration grammar.
        for form in ["-q or --quiet", "-a or --all"] {
            let scan = literal_declaration_scan_with_starts(form, &[], &[], SingleTextOperand);
            assert_eq!(scan.names(form).0, literal_option_names(form), "{form}");
            assert_eq!(scan.names(form).0.len(), 2, "{form}");
        }
    }

    #[test]
    fn single_ip_operand_font_switch_is_not_an_independent_boundary() {
        // Both exact `.IP` inputs ran pinned CVS -Tutf8 first. Its HEAD has
        // one text operand (man_term.c::pre_IP); term.c::term_word applies
        // inline font escapes without making another native component.
        let inline_font = "-L first,--fake,last,";
        assert_eq!(
            literal_declaration_scan_with_starts(inline_font, &[], &[3], SingleTextOperand)
                .names(inline_font)
                .0,
            [("-L".into(), 0..2)]
        );
        // Even comma+space plus a later bold run cannot establish an
        // independent declaration after the styled middle parameter. The
        // same first operand can contain `first, \fB--fake`, so omit `--all`
        // conservatively rather than promoting a false name.
        let spaced = "-a, --operand, --all";
        assert_eq!(
            literal_declaration_scan_with_starts(spaced, &[], &[4], SingleTextOperand)
                .names(spaced)
                .0,
            [("-a".into(), 0..2)]
        );
        let plain = "-a, --all";
        assert_eq!(
            literal_declaration_scan_with_starts(plain, &[], &[], SingleTextOperand)
                .names(plain)
                .0,
            [("-a".into(), 0..2), ("--all".into(), 4..9)]
        );
    }

    #[test]
    fn provisional_pattern_still_bounds_ordinary_and_styled_arguments() {
        // Each exact TP/B or TP/BI input first ran pinned CVS -Tutf8.
        // man_macro.c::blk_imp retains one HEAD, man_term.c::pre_alternate
        // joins BI operands, and term.c::term_word prints punctuation inside
        // a following parameter without manufacturing declaration nodes.
        for form in [
            "-### --long first,--fake,last",
            "-### --long first|--fake|last",
            "-### --long -10,--fake,20",
            "-### --long=FILE",
        ] {
            assert_eq!(
                literal_option_names(form),
                [("--long".into(), 5..11)],
                "{form}"
            );
        }
        let followed = "-### --long first,--fake,last, --all";
        let start = followed.find("--all").unwrap();
        assert_eq!(
            literal_option_names(followed),
            [("--long".into(), 5..11), ("--all".into(), start..start + 5)]
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
