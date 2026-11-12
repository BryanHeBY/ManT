//! Balanced source-relative tokens, with opaque quoted and URI atoms.

use std::ops::Range;

use super::MAX_DEPTH;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Word,
    Option,
    Name,
    Open(char),
    Close,
    Equals,
    Colon,
    Choice,
    Repeat,
    Opaque,
    Newline,
}

pub(super) struct Token {
    pub(super) bytes: Range<usize>,
    pub(super) kind: Kind,
    pub(super) group: Option<usize>,
}

pub(super) struct Group {
    pub(super) delimiter: char,
    pub(super) parent: Option<usize>,
    pub(super) option: bool,
    pub(super) choice: bool,
    pub(super) attached: bool,
}

pub(super) struct Syntax {
    pub(super) tokens: Vec<Token>,
    pub(super) groups: Vec<Group>,
}

pub(super) fn tokenize(value: &str, names: &[Range<usize>]) -> Option<Syntax> {
    tokenize_with_lookahead(value, names, &mut UriLookahead::default())
}

fn tokenize_with_lookahead(
    value: &str,
    names: &[Range<usize>],
    uri: &mut UriLookahead,
) -> Option<Syntax> {
    let mut syntax = Syntax {
        tokens: Vec::new(),
        groups: Vec::new(),
    };
    let mut stack = Vec::new();
    let mut byte = 0;
    let mut name_index = 0;
    while byte < value.len() {
        while names.get(name_index).is_some_and(|range| range.end <= byte) {
            name_index += 1;
        }
        let first = value[byte..].chars().next()?;
        if first != '\n' && first.is_whitespace() {
            byte += first.len_utf8();
            continue;
        }
        let start = byte;
        let mut group = stack.last().copied();
        let kind;
        if let Some(name) = names.get(name_index).filter(|name| name.start == byte) {
            byte = name.end;
            kind = Kind::Name;
            if value[start..byte].starts_with('-') {
                mark_options(&mut syntax.groups, group);
            }
        } else if matches!(first, '\'' | '"' | '`') {
            byte = quoted_end(value, byte, first)?;
            kind = Kind::Opaque;
        } else if let Some(body) = uri.body_start(value, byte) {
            let close = group.map(|id| closing(syntax.groups[id].delimiter));
            byte = body + uri_body_end(&value[body..], close);
            kind = Kind::Opaque;
        } else if first == '\\' {
            byte += value[byte..]
                .find(char::is_whitespace)
                .unwrap_or(value.len() - byte);
            kind = Kind::Opaque;
        } else if matches!(first, '[' | '<' | '{' | '(') {
            open_group(&mut syntax, &mut stack, value, start, first)?;
            kind = Kind::Open(first);
            byte += first.len_utf8();
        } else if matches!(first, ']' | '>' | '}' | ')') {
            let id = stack.pop()?;
            if closing(syntax.groups[id].delimiter) != first {
                return None;
            }
            group = Some(id);
            kind = Kind::Close;
            byte += first.len_utf8();
        } else if first == '=' {
            kind = Kind::Equals;
            byte += 1;
        } else if first == ':' {
            kind = Kind::Colon;
            byte += 1;
        } else if first == '|' {
            if value[byte..].starts_with("||") || value[byte..].starts_with("|&") {
                return None;
            }
            let mut parent = group;
            while let Some(id) = parent {
                syntax.groups[id].choice = true;
                parent = syntax.groups[id].parent;
            }
            kind = Kind::Choice;
            byte += 1;
        } else if first == '\n' {
            kind = Kind::Newline;
            byte += 1;
        } else if value[byte..].starts_with("...") {
            kind = Kind::Repeat;
            byte += 3;
        } else if matches!(first, '#' | ';' | '&' | '$') {
            // Shell execution and prose punctuation do not establish usage
            // syntax. They remain owned by the ordinary code scanner.
            return None;
        } else if first == ',' {
            kind = Kind::Opaque;
            byte += 1;
        } else {
            byte = word_end(value, byte, names.get(name_index));
            let word = &value[start..byte];
            kind = if option(word) {
                Kind::Option
            } else {
                Kind::Word
            };
            if kind == Kind::Option {
                mark_options(&mut syntax.groups, group);
            }
        }
        syntax.tokens.push(Token {
            bytes: start..byte,
            kind,
            group,
        });
    }
    stack.is_empty().then_some(syntax)
}

fn open_group(
    syntax: &mut Syntax,
    stack: &mut Vec<usize>,
    value: &str,
    start: usize,
    delimiter: char,
) -> Option<()> {
    if stack.len() == MAX_DEPTH {
        return None;
    }
    let attached = syntax.tokens.last().is_some_and(|previous| {
        previous.bytes.end == start
            && (previous.kind == Kind::Option
                || previous.kind == Kind::Name && value[previous.bytes.clone()].starts_with('-'))
    });
    let id = syntax.groups.len();
    syntax.groups.push(Group {
        delimiter,
        parent: stack.last().copied(),
        option: false,
        choice: false,
        attached,
    });
    stack.push(id);
    Some(())
}

fn quoted_end(value: &str, start: usize, quote: char) -> Option<usize> {
    let mut byte = start + quote.len_utf8();
    loop {
        let character = value[byte..].chars().next()?;
        byte += character.len_utf8();
        if character == '\\' && quote != '\'' {
            byte += value[byte..].chars().next()?.len_utf8();
        } else if character == quote {
            return Some(byte);
        }
    }
}

fn word_end(value: &str, start: usize, next_name: Option<&Range<usize>>) -> usize {
    let mut byte = start;
    for character in value[start..].chars() {
        if byte > start
            && (character.is_whitespace()
                || "[]<>{}()=:|,'\"`\\#;&$".contains(character)
                || value[byte..].starts_with("...")
                || next_name.is_some_and(|name| name.start == byte))
        {
            break;
        }
        byte += character.len_utf8();
    }
    byte
}

fn mark_options(groups: &mut [Group], mut parent: Option<usize>) {
    while let Some(id) = parent {
        groups[id].option = true;
        parent = groups[id].parent;
    }
}

fn option(word: &str) -> bool {
    let name = word.strip_prefix("--").or_else(|| word.strip_prefix('-'));
    name.is_some_and(|name| {
        name.is_empty()
            || name
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphanumeric())
                && name.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '?')
                })
    })
}

fn closing(open: char) -> char {
    match open {
        '[' => ']',
        '<' => '>',
        '{' => '}',
        _ => ')',
    }
}

/// A URI owns balanced brackets within its atom (notably IPv6 hosts), while
/// an unbalanced closing delimiter still belongs to the surrounding syntax.
pub(super) fn uri_body_end(value: &str, outer_close: Option<char>) -> usize {
    let mut brackets = Vec::new();
    for (byte, character) in value.char_indices() {
        if character.is_whitespace() || brackets.is_empty() && outer_close == Some(character) {
            return byte;
        }
        if matches!(character, '[' | '(' | '{') {
            brackets.push(closing(character));
        } else if brackets.last() == Some(&character) {
            brackets.pop();
        }
    }
    value.len()
}

#[derive(Default)]
pub(super) struct UriLookahead {
    scanned_until: usize,
    #[cfg(test)]
    scheme_bytes_examined: usize,
}

impl UriLookahead {
    /// Only the first byte outside the ASCII scheme run can end a URI
    /// prefix. After a miss, starting later inside that run would find the
    /// same miss. After a match, the lexer consumes at least the prefix.
    /// In both cases the next probe skips all scheme bytes already examined.
    pub(super) fn body_start(&mut self, value: &str, start: usize) -> Option<usize> {
        if start < self.scanned_until {
            return None;
        }
        let bytes = value.as_bytes();
        for (offset, &character) in bytes[start..].iter().enumerate() {
            #[cfg(test)]
            {
                self.scheme_bytes_examined += 1;
            }
            if !character.is_ascii_alphanumeric() && !matches!(character, b'+' | b'-' | b'.') {
                let end = start + offset;
                self.scanned_until = end + 1;
                return (offset != 0 && character == b':' && bytes[end + 1..].starts_with(b"//"))
                    .then_some(end + 3);
            }
        }
        self.scanned_until = value.len();
        None
    }

    #[cfg(test)]
    pub(super) const fn scheme_work(&self) -> usize {
        self.scheme_bytes_examined
    }
}

#[cfg(test)]
mod tests;
