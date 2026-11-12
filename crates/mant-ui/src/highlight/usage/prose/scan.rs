//! One forward token walk; URI lookahead retains the shared monotone cache.

use std::ops::Range;

use super::{MAX_DEPTH, Work};
use crate::highlight::usage::lex::{UriLookahead, uri_body_end};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Option,
    Word,
    Response,
    Equals,
    Open(char),
    Close,
    Choice,
    Repeat,
    Comma,
    Opaque,
    Protected,
    Other,
    Sentence,
}

pub(super) struct Token {
    pub(super) bytes: Range<usize>,
    pub(super) kind: Kind,
    pub(super) group: Option<usize>,
}
pub(super) struct Group {
    pub(super) open: char,
    pub(super) closed: bool,
    pub(super) choice: bool,
    pub(super) spelling_suffix: bool,
}
pub(super) struct Syntax {
    pub(super) tokens: Vec<Token>,
    pub(super) groups: Vec<Group>,
}

pub(super) fn tokens(value: &str, protected: &[Range<usize>], work: &mut Work) -> Option<Syntax> {
    let mut result = Syntax {
        tokens: Vec::new(),
        groups: Vec::new(),
    };
    let mut stack: Vec<usize> = Vec::new();
    let mut uri = UriLookahead::default();
    let mut byte = 0;
    let mut protected = Protected {
        ranges: protected,
        index: 0,
    };
    while byte < value.len() {
        let start = byte;
        if let Some(end) = protected.end_at(byte, work) {
            byte = end;
            result.tokens.push(Token {
                bytes: start..byte,
                kind: Kind::Protected,
                group: None,
            });
            work.charge(1);
            continue;
        }
        let limit = protected.limit(value.len());
        let remaining = &value[byte..limit];
        let character = remaining.chars().next()?;
        if character.is_whitespace() {
            byte += character.len_utf8();
            work.charge(byte - start);
            continue;
        }
        let mut group = stack.last().copied();
        let kind;
        if matches!(character, '\'' | '"' | '`') {
            byte = quoted_end(&value[..limit], byte, character);
            kind = Kind::Opaque;
        } else if let Some(body) = uri.body_start(&value[..limit], byte) {
            byte = body
                + uri_body_end(
                    &value[body..limit],
                    group.map(|id| close(result.groups[id].open)),
                );
            kind = Kind::Opaque;
        } else if matches!(character, '[' | '<' | '{' | '(') {
            let id = open_group(&mut result, &mut stack, byte, character)?;
            group = Some(id);
            kind = Kind::Open(character);
            byte += character.len_utf8();
        } else if matches!(character, ']' | '>' | '}' | ')') {
            if let Some(&id) = stack.last()
                && close(result.groups[id].open) == character
            {
                result.groups[id].closed = true;
                group = stack.pop();
                kind = Kind::Close;
            } else {
                kind = Kind::Other;
            }
            byte += character.len_utf8();
        } else if character == '=' {
            for &id in &stack {
                work.charge(1);
                result.groups[id].spelling_suffix = false;
            }
            kind = Kind::Equals;
            byte += 1;
        } else if character == '|' {
            for &id in &stack {
                work.charge(1);
                result.groups[id].choice = true;
            }
            kind = Kind::Choice;
            byte += 1;
        } else if character == ',' {
            kind = Kind::Comma;
            byte += 1;
        } else if remaining.starts_with("...") {
            kind = Kind::Repeat;
            byte += 3;
        } else if matches!(character, '.' | ';') {
            kind = Kind::Sentence;
            byte += 1;
        } else {
            byte = atom_end(&value[..limit], byte);
            kind = atom_kind(&value[start..byte]);
        }
        work.charge(byte - start + 1);
        result.tokens.push(Token {
            bytes: start..byte,
            kind,
            group,
        });
    }
    #[cfg(test)]
    work.charge(uri.scheme_work());
    Some(result)
}

struct Protected<'a> {
    ranges: &'a [Range<usize>],
    index: usize,
}

impl Protected<'_> {
    fn end_at(&mut self, byte: usize, work: &mut Work) -> Option<usize> {
        while self
            .ranges
            .get(self.index)
            .is_some_and(|range| range.end <= byte)
        {
            self.index += 1;
            work.charge(1);
        }
        self.ranges
            .get(self.index)
            .filter(|range| range.start == byte)
            .map(|range| range.end)
    }

    fn limit(&self, length: usize) -> usize {
        self.ranges
            .get(self.index)
            .map_or(length, |range| range.start)
    }
}

fn atom_kind(text: &str) -> Kind {
    if option(text) {
        Kind::Option
    } else if text.starts_with('@') && text.len() > 1 {
        Kind::Response
    } else if text
        .chars()
        .next()
        .is_some_and(|first| first.is_alphanumeric() || first == '_')
    {
        Kind::Word
    } else {
        Kind::Other
    }
}

fn open_group(
    result: &mut Syntax,
    stack: &mut Vec<usize>,
    byte: usize,
    open: char,
) -> Option<usize> {
    if stack.len() == MAX_DEPTH {
        return None;
    }
    let attached = result
        .tokens
        .last()
        .is_some_and(|token| token.kind == Kind::Option && token.bytes.end == byte);
    let id = result.groups.len();
    result.groups.push(Group {
        open,
        closed: false,
        choice: false,
        spelling_suffix: attached,
    });
    stack.push(id);
    Some(id)
}

fn close(open: char) -> char {
    match open {
        '[' => ']',
        '<' => '>',
        '{' => '}',
        _ => ')',
    }
}

fn quoted_end(value: &str, start: usize, quote: char) -> usize {
    let mut escaped = false;
    for (byte, character) in value[start + quote.len_utf8()..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote != '\'' {
            escaped = true;
        } else if character == quote {
            return start + quote.len_utf8() + byte + character.len_utf8();
        }
    }
    value.len()
}

fn atom_end(value: &str, start: usize) -> usize {
    let mut byte = start;
    let mut escaped = false;
    for character in value[start..].chars() {
        if escaped {
            byte += character.len_utf8();
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            byte += 1;
            continue;
        }
        if byte > start
            && (character.is_whitespace()
                || "[]<>{}()=|,;\"`".contains(character)
                || value[byte..].starts_with("..."))
        {
            break;
        }
        // A terminal sentence full stop is outside an option atom; dotted
        // option names, paths and compound identifiers remain whole.
        if byte > start
            && character == '.'
            && value[byte + 1..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace)
        {
            break;
        }
        byte += character.len_utf8();
    }
    byte
}

fn option(word: &str) -> bool {
    if word == "--" {
        return true;
    }
    let name = word.strip_prefix("--").or_else(|| word.strip_prefix('-'));
    name.is_some_and(|name| {
        !name.is_empty()
            && name
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || matches!(first, '#' | '?'))
            && name.chars().all(|character| {
                !character.is_ascii()
                    || character.is_ascii_alphanumeric()
                    || "_-?#+.".contains(character)
            })
    })
}
