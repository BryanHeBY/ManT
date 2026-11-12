//! Source-marked option references and complete option inventories.
//!
//! The permissive token stream is local to presentation. It does not establish
//! entry identities or option arity, and never converts paragraphs to code.

use std::ops::Range;

use super::{Accent, MAX_BYTES, MAX_DEPTH, Role};

mod scan;
use scan::{Group, Kind, Token};

#[derive(Default)]
struct Work {
    #[cfg(test)]
    steps: usize,
}

impl Work {
    fn charge(&mut self, steps: usize) {
        #[cfg(test)]
        {
            self.steps += steps;
        }
        #[cfg(not(test))]
        let _ = (self, steps);
    }
}

pub(crate) fn accents(
    value: &str,
    marked: &[Range<usize>],
    emphasis: &[Range<usize>],
    protected: &[Range<usize>],
) -> Vec<Accent> {
    checked_accents(value, marked, emphasis, protected).unwrap_or_default()
}

pub(crate) fn checked_accents(
    value: &str,
    marked: &[Range<usize>],
    emphasis: &[Range<usize>],
    protected: &[Range<usize>],
) -> Option<Vec<Accent>> {
    accents_with_work(value, marked, emphasis, protected, &mut Work::default())
}

fn accents_with_work(
    value: &str,
    marked: &[Range<usize>],
    emphasis: &[Range<usize>],
    protected: &[Range<usize>],
    work: &mut Work,
) -> Option<Vec<Accent>> {
    if value.len() > MAX_BYTES
        || !super::valid_ranges(value, marked)
        || !super::valid_ranges(value, emphasis)
        || !super::valid_ranges(value, protected)
    {
        return None;
    }
    let syntax = scan::tokens(value, protected, work)?;
    let inventory = inventory(
        value,
        &syntax.tokens,
        &syntax.groups,
        emphasis,
        protected,
        work,
    );
    let mut marked = Ranges::new(marked);
    let mut emphasis = Ranges::new(emphasis);
    let mut protected = Ranges::new(protected);
    let mut result = Vec::new();
    let mut chain = false;
    let mut previous_end = 0;
    let mut previous_option = false;
    for token in &syntax.tokens {
        let marked = marked.covers(&token.bytes, work);
        let fragments = emphasis.fragments(&token.bytes, work);
        let emphasized = covers_fragments(&fragments, &token.bytes);
        let adjacent = previous_end == token.bytes.start;
        let text = &value[token.bytes.clone()];
        let enabled_option = token.kind == Kind::Option
            && (inventory || marked)
            && option_proven(text, token.bytes.start, &fragments, work);
        let mut accents = Vec::new();
        match token.kind {
            Kind::Option if enabled_option => {
                option_accents(token, &fragments, emphasized, &mut accents);
                chain = true;
            }
            Kind::Equals if adjacent && chain => accents.push(Accent {
                bytes: token.bytes.clone(),
                role: Role::Operator,
            }),
            Kind::Open(_) | Kind::Close | Kind::Choice | Kind::Repeat | Kind::Comma
                if inventory || adjacent && chain =>
            {
                if token.group.is_some_and(|id| syntax.groups[id].closed)
                    || token.kind == Kind::Repeat
                {
                    accents.push(Accent {
                        bytes: token.bytes.clone(),
                        role: Role::Delimiter,
                    });
                }
            }
            Kind::Word => {
                let slot = placeholder(
                    text,
                    token.group.map(|id| &syntax.groups[id]),
                    emphasized,
                    inventory || adjacent && chain,
                );
                if slot
                    && (inventory
                        || adjacent && chain
                        || previous_option && emphasized && super::metavariable(text))
                {
                    accents.push(Accent {
                        bytes: token.bytes.clone(),
                        role: Role::Placeholder,
                    });
                }
                chain &= adjacent;
            }
            Kind::Response
                if inventory
                    || marked
                        && fragments
                            .iter()
                            .any(|range| range.start > token.bytes.start) =>
            {
                accents.push(Accent {
                    bytes: token.bytes.start..token.bytes.start + 1,
                    role: Role::Operator,
                });
                accents.push(Accent {
                    bytes: token.bytes.start + 1..token.bytes.end,
                    role: Role::Placeholder,
                });
                chain = false;
            }
            Kind::Opaque => chain &= adjacent,
            _ => chain = false,
        }
        for accent in accents {
            emit(&mut result, &accent, &mut protected, work);
        }
        previous_end = token.bytes.end;
        previous_option = enabled_option;
    }
    Some(result)
}

fn inventory(
    value: &str,
    tokens: &[Token],
    groups: &[Group],
    emphasis: &[Range<usize>],
    protected: &[Range<usize>],
    work: &mut Work,
) -> bool {
    // An independent POSIX test uses whitespace after '['; '[[' is shell
    // execution syntax. Neither is evidence for a CLI option inventory.
    if tokens.first().is_some_and(|token| {
        token.kind == Kind::Open('[')
            && value[token.bytes.end..]
                .chars()
                .next()
                .is_some_and(|next| next.is_whitespace() || next == '[')
    }) {
        return false;
    }
    let first = tokens
        .iter()
        .find(|token| !matches!(token.kind, Kind::Open('[' | '{')));
    if !first.is_some_and(|token| matches!(token.kind, Kind::Option | Kind::Response)) {
        return false;
    }
    let mut emphasis = Ranges::new(emphasis);
    let mut protected = Ranges::new(protected);
    let mut options = 0;
    for token in tokens {
        work.charge(1);
        if protected.overlaps(&token.bytes, work) {
            return false;
        }
        match token.kind {
            Kind::Option => {
                let text = &value[token.bytes.clone()];
                let fragments = emphasis.fragments(&token.bytes, work);
                options += usize::from(option_proven(text, token.bytes.start, &fragments, work));
            }
            Kind::Word => {
                let text = &value[token.bytes.clone()];
                let choice = token
                    .group
                    .is_some_and(|id| groups[id].closed && groups[id].choice);
                if !emphasis.covers(&token.bytes, work)
                    && !super::metavariable(text)
                    && !super::uppercase(text)
                    && !choice
                    && !text.chars().next().is_some_and(char::is_numeric)
                {
                    return false;
                }
            }
            Kind::Sentence => return false,
            _ => {}
        }
    }
    options >= 2
}

fn option_accents(
    token: &Token,
    fragments: &[Range<usize>],
    emphasized: bool,
    result: &mut Vec<Accent>,
) {
    let mut start = token.bytes.start;
    if !emphasized {
        for fragment in fragments
            .iter()
            .filter(|range| range.start > token.bytes.start + 1)
        {
            result.push(Accent {
                bytes: start..fragment.start,
                role: Role::Option,
            });
            result.push(Accent {
                bytes: fragment.clone(),
                role: Role::Placeholder,
            });
            start = fragment.end;
        }
    }
    if start < token.bytes.end {
        result.push(Accent {
            bytes: start..token.bytes.end,
            role: Role::Option,
        });
    }
}

fn placeholder(text: &str, group: Option<&Group>, emphasized: bool, inferred: bool) -> bool {
    if group.is_some_and(|group| group.open == '<' && group.closed) {
        return true;
    }
    (!group.is_some_and(|group| group.spelling_suffix) || super::uppercase(text))
        && !text.chars().next().is_some_and(char::is_numeric)
        && (emphasized
            || inferred
                && if group.is_some_and(|group| group.choice) {
                    super::choice_metavariable(text)
                } else {
                    super::metavariable(text) || super::uppercase(text)
                })
}

fn option_proven(text: &str, start: usize, emphasis: &[Range<usize>], work: &mut Work) -> bool {
    let mut index = 0;
    for (byte, character) in text.char_indices() {
        work.charge(1);
        if character.is_ascii() {
            continue;
        }
        let byte = start + byte;
        while emphasis.get(index).is_some_and(|range| range.end <= byte) {
            index += 1;
            work.charge(1);
        }
        if !emphasis
            .get(index)
            .is_some_and(|range| range.contains(&byte))
        {
            return false;
        }
    }
    true
}

fn covers_fragments(fragments: &[Range<usize>], token: &Range<usize>) -> bool {
    fragments
        .first()
        .is_some_and(|range| range.start == token.start)
        && fragments.last().is_some_and(|range| range.end == token.end)
        && fragments
            .windows(2)
            .all(|pair| pair[0].end == pair[1].start)
}

struct Ranges<'a> {
    values: &'a [Range<usize>],
    cursor: usize,
}

impl<'a> Ranges<'a> {
    const fn new(values: &'a [Range<usize>]) -> Self {
        Self { values, cursor: 0 }
    }

    fn advance(&mut self, start: usize, work: &mut Work) {
        while self
            .values
            .get(self.cursor)
            .is_some_and(|range| range.end <= start)
        {
            work.charge(1);
            self.cursor += 1;
        }
    }

    fn covers(&mut self, token: &Range<usize>, work: &mut Work) -> bool {
        self.advance(token.start, work);
        work.charge(1);
        self.values
            .get(self.cursor)
            .is_some_and(|range| range.start <= token.start && range.end >= token.end)
    }

    fn overlaps(&mut self, token: &Range<usize>, work: &mut Work) -> bool {
        self.advance(token.start, work);
        work.charge(1);
        self.values
            .get(self.cursor)
            .is_some_and(|range| range.start < token.end)
    }

    fn fragments(&mut self, token: &Range<usize>, work: &mut Work) -> Vec<Range<usize>> {
        self.advance(token.start, work);
        let mut result = Vec::new();
        while let Some(range) = self.values.get(self.cursor) {
            work.charge(1);
            if range.start >= token.end {
                break;
            }
            result.push(range.start.max(token.start)..range.end.min(token.end));
            if range.end > token.end {
                break;
            }
            self.cursor += 1;
        }
        result
    }
}

fn emit(result: &mut Vec<Accent>, accent: &Accent, protected: &mut Ranges<'_>, work: &mut Work) {
    protected.advance(accent.bytes.start, work);
    let mut start = accent.bytes.start;
    for range in &protected.values[protected.cursor..] {
        work.charge(1);
        if range.start >= accent.bytes.end {
            break;
        }
        if start < range.start {
            push(result, start..range.start, accent.role);
        }
        start = start.max(range.end);
    }
    if start < accent.bytes.end {
        push(result, start..accent.bytes.end, accent.role);
    }
}

fn push(result: &mut Vec<Accent>, bytes: Range<usize>, role: Role) {
    if bytes.is_empty() {
        return;
    }
    if let Some(last) = result.last_mut()
        && last.role == role
        && last.bytes.end == bytes.start
    {
        last.bytes.end = bytes.end;
    } else {
        result.push(Accent { bytes, role });
    }
}

#[cfg(test)]
mod tests;
