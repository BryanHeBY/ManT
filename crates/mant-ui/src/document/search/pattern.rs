//! Bounded execution of the regex-automata Thompson NFA.
//!
//! Follow `PikeVM`'s ordered epsilon closures and stop lower-priority paths at
//! a match, retaining higher-priority paths for greedy/leftmost-first matches.
//! Unlike a synchronous find call, both closures and byte transitions can yield.
//! Assertions see the complete logical record, never a truncated scan window.

use mant_protocol::{SearchCase, SearchSyntax};
use regex_automata::{
    nfa::thompson::{NFA, State, WhichCaptures},
    util::{primitives::StateID, syntax},
};
use std::ops::Range;

#[derive(Debug, Clone)]
pub(crate) struct PatternStream {
    nfa: NFA,
    stack: Vec<(StateID, usize)>,
    current: Vec<(StateID, usize)>,
    next: Vec<(StateID, usize)>,
    seen: Vec<u32>,
    generation: u32,
    cursor: usize,
    at: usize,
    candidate: Option<Range<usize>>,
    done: bool,
    literal: Option<ExactLiteral>,
}

impl PatternStream {
    pub(crate) fn compile(
        pattern: &str,
        mode: SearchSyntax,
        case: SearchCase,
    ) -> Result<Self, String> {
        let insensitive = case == SearchCase::Insensitive
            || (case == SearchCase::Smart && !pattern.chars().any(char::is_uppercase));
        let pattern = if mode == SearchSyntax::Literal {
            regex_syntax::escape(pattern)
        } else {
            pattern.to_owned()
        };
        let hir = regex_syntax::ParserBuilder::new()
            .case_insensitive(insensitive)
            .build()
            .parse(&pattern)
            .map_err(|error| error.to_string())?;
        if hir.properties().minimum_len() == Some(0) {
            return Err("Pattern must not match empty text".to_owned());
        }
        let nfa = NFA::compiler()
            .configure(
                NFA::config()
                    .nfa_size_limit(Some(2 * 1024 * 1024))
                    .which_captures(WhichCaptures::None),
            )
            .syntax(syntax::Config::new().case_insensitive(insensitive))
            .build(&pattern)
            .map_err(|error| error.to_string())?;
        let count = nfa.states().len();
        let literal = if let regex_syntax::hir::HirKind::Literal(value) = hir.kind() {
            Some(ExactLiteral::new(value.0.to_vec()))
        } else {
            None
        };
        let mut stream = Self {
            nfa,
            stack: Vec::new(),
            current: Vec::new(),
            next: Vec::new(),
            seen: vec![0; count],
            generation: 0,
            cursor: 0,
            at: 0,
            candidate: None,
            done: false,
            literal,
        };
        stream.begin_position();
        Ok(stream)
    }

    fn begin_position(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.seen.fill(0);
            self.generation = 1;
        }
        self.current.clear();
        self.cursor = 0;
        // New starts have lower priority than existing paths. No new starts
        // after a candidate: higher-priority paths alone can extend/replace it.
        if self.candidate.is_none() {
            self.stack.push((self.nfa.start_anchored(), self.at));
        }
        self.stack.extend(self.next.drain(..).rev());
    }

    pub(crate) fn reset(&mut self) {
        self.stack.clear();
        self.current.clear();
        self.next.clear();
        self.at = 0;
        self.candidate = None;
        self.done = false;
        if let Some(literal) = &mut self.literal {
            literal.offset = 0;
            literal.matched = 0;
        }
        self.begin_position();
    }

    pub(crate) const fn is_done(&self) -> bool {
        self.done
    }

    /// Charge each state visit, not only consumed bytes: a complex regex must
    /// also yield within a large epsilon closure. All text stays caller-owned.
    pub(crate) fn advance(&mut self, text: &str, budget: &mut usize) -> Vec<Range<usize>> {
        let bytes = text.as_bytes();
        if let Some(literal) = &mut self.literal {
            let hits = literal.advance(bytes, budget);
            self.done = literal.offset == bytes.len();
            return hits;
        }
        let mut hits = Vec::new();
        while *budget > 0 && !self.done {
            *budget -= 1;
            if let Some((id, start)) = self.stack.pop() {
                if self.seen[id.as_usize()] == self.generation {
                    continue;
                }
                self.seen[id.as_usize()] = self.generation;
                match self.nfa.state(id) {
                    State::Look { look, next } => {
                        if self.nfa.look_matcher().matches(*look, bytes, self.at) {
                            self.stack.push((*next, start));
                        }
                    }
                    State::Union { alternates } => self
                        .stack
                        .extend(alternates.iter().rev().map(|&id| (id, start))),
                    State::BinaryUnion { alt1, alt2 } => {
                        self.stack.push((*alt2, start));
                        self.stack.push((*alt1, start));
                    }
                    State::Capture { next, .. } => self.stack.push((*next, start)),
                    State::Fail => {}
                    _ => self.current.push((id, start)),
                }
                continue;
            }
            if let Some(&(id, start)) = self.current.get(self.cursor) {
                self.cursor += 1;
                let next = match self.nfa.state(id) {
                    State::ByteRange { trans } => {
                        trans.matches(bytes, self.at).then_some(trans.next)
                    }
                    State::Sparse(trans) => trans.matches(bytes, self.at),
                    State::Dense(trans) => trans.matches(bytes, self.at),
                    State::Match { .. } => {
                        self.candidate = Some(start..self.at);
                        self.cursor = self.current.len();
                        None
                    }
                    _ => None,
                };
                if let Some(next) = next {
                    self.next.push((next, start));
                }
                continue;
            }
            if self.next.is_empty() || self.at == bytes.len() {
                if let Some(range) = self.candidate.take() {
                    self.at = range.end;
                    hits.push(range);
                    self.next.clear();
                    self.begin_position();
                    continue;
                }
                if self.at == bytes.len() {
                    self.done = true;
                    break;
                }
            }
            self.at += 1;
            self.begin_position();
        }
        hits
    }
}

/// Plain patterns use KMP without visiting an NFA state per query character.
#[derive(Debug, Clone)]
struct ExactLiteral {
    needle: Vec<u8>,
    prefixes: Vec<usize>,
    matched: usize,
    offset: usize,
}

impl ExactLiteral {
    fn new(needle: Vec<u8>) -> Self {
        let mut prefixes = vec![0; needle.len()];
        for index in 1..needle.len() {
            let mut length = prefixes[index - 1];
            while length > 0 && needle[index] != needle[length] {
                length = prefixes[length - 1];
            }
            if needle[index] == needle[length] {
                length += 1;
            }
            prefixes[index] = length;
        }
        Self {
            needle,
            prefixes,
            matched: 0,
            offset: 0,
        }
    }
    fn advance(&mut self, text: &[u8], budget: &mut usize) -> Vec<Range<usize>> {
        let mut hits = Vec::new();
        while *budget > 0 && self.offset < text.len() {
            let byte = text[self.offset];
            self.offset += 1;
            *budget -= 1;
            while self.matched > 0 && byte != self.needle[self.matched] {
                self.matched = self.prefixes[self.matched - 1];
            }
            if byte == self.needle[self.matched] {
                self.matched += 1;
            }
            if self.matched == self.needle.len() {
                hits.push(self.offset - self.needle.len()..self.offset);
                self.matched = 0;
            }
        }
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex_automata::meta::Regex;

    #[test]
    fn every_budget_matches_upstream_leftmost_first_ranges() {
        for (pattern, text) in [
            ("a+", "aaa aa"),
            ("a+?", "aaa aa"),
            ("ab|a", "ababa"),
            ("a.*z|b", "abbb abzz b"),
            ("foo.*bar", "foo bar bar foo bar"),
            (r"\b\w+\b", "é界 foo ΟΣ"),
            ("(?i)foo", "FOO foo Foo"),
            ("(?m)^--[a-z]+$", "--foo\n--bar\ntext"),
            ("界+|é", "界界 é"),
            ("(ab){2,3}", "ababab abab"),
        ] {
            let expected: Vec<_> = Regex::new(pattern)
                .unwrap()
                .find_iter(text)
                .map(|hit| hit.range())
                .collect();
            for size in [1, 2, 7, 64, 8192] {
                let mut stream =
                    PatternStream::compile(pattern, SearchSyntax::Regex, SearchCase::Sensitive)
                        .unwrap();
                let mut actual = Vec::new();
                while !stream.is_done() {
                    actual.extend(stream.advance(text, &mut size.clone()));
                }
                assert_eq!(actual, expected, "{pattern}/{text}, budget {size}");
            }
        }
    }

    #[test]
    fn syntax_cases_errors_and_cancellation_are_explicit() {
        assert!(PatternStream::compile("[", SearchSyntax::Regex, SearchCase::Sensitive).is_err());
        assert!(PatternStream::compile("a*", SearchSyntax::Regex, SearchCase::Sensitive).is_err());
        for (mode, case, query, count) in [
            (SearchSyntax::Literal, SearchCase::Sensitive, "[Foo]", 1),
            (SearchSyntax::Literal, SearchCase::Insensitive, "[Foo]", 2),
            (SearchSyntax::Literal, SearchCase::Smart, "[Foo]", 1),
            (SearchSyntax::Literal, SearchCase::Smart, "[foo]", 2),
        ] {
            let mut stream = PatternStream::compile(query, mode, case).unwrap();
            let mut hits = Vec::new();
            while !stream.is_done() {
                hits.extend(stream.advance("[Foo] [foo]", &mut 1024));
            }
            assert_eq!(hits.len(), count);
        }
        let text = format!("a{}z", "x".repeat(100_000));
        let mut stream =
            PatternStream::compile("a.*z", SearchSyntax::Regex, SearchCase::Sensitive).unwrap();
        assert_eq!(stream.advance(&text, &mut 8192), Vec::<Range<usize>>::new());
        assert!(!stream.is_done());
        stream.reset();
        let mut hits = Vec::new();
        while !stream.is_done() {
            hits.extend(stream.advance("az", &mut 1));
        }
        assert_eq!(hits, vec![0..2]);
    }

    #[test]
    fn resumable_execution_matches_upstream_across_branch_and_anchor_combinations() {
        for pattern in [
            "a|ab",
            "ab|a",
            "a+?a",
            "(a|b)+c",
            "(?:ab|a)b?",
            "(?:a.*z|b)+",
            r"\b[a-z]+\b",
            "(?i)k|σ",
            "(?m)^a+$",
            "a$|b",
            "(?s)a.*?b",
        ] {
            let reference = Regex::new(pattern).unwrap();
            for length in 0..=5 {
                for encoded in 0..3usize.pow(length) {
                    let mut value = encoded;
                    let mut text = String::new();
                    for _ in 0..length {
                        text.push(['a', 'b', 'c'][value % 3]);
                        value /= 3;
                    }
                    text.push_str("\n\u{212A} σ ς abazz");
                    let expected: Vec<_> =
                        reference.find_iter(&text).map(|hit| hit.range()).collect();
                    let mut stream =
                        PatternStream::compile(pattern, SearchSyntax::Regex, SearchCase::Sensitive)
                            .unwrap();
                    let mut actual = Vec::new();
                    while !stream.is_done() {
                        actual.extend(stream.advance(&text, &mut 7));
                    }
                    assert_eq!(actual, expected, "{pattern}/{text:?}");
                }
            }
        }
    }
}
