//! Bounded lexical cues for mixed manual examples, not a language grammar.

use super::{Accent, StyleRole};

pub(super) const MAX_CONTEXT_DEPTH: usize = 64;

#[derive(Clone, Copy)]
enum Frame {
    Quote(char),
    Group { close: char, optional: bool },
    Parameter { operand: bool, name_seen: bool },
    Command { head: bool },
    Arithmetic,
}

#[derive(Clone, Copy)]
enum Mode {
    Normal,
    Parameter { operand: bool },
    Command { head: bool },
    Arithmetic,
}

pub(super) fn accents(value: &str) -> Vec<Accent> {
    let mut scanner = Scanner {
        value,
        accents: Vec::new(),
        frames: Vec::new(),
        optional: 0,
        limited: false,
    };
    let mut offset = 0;
    let mut command_line = false;
    let mut row_end = 0;
    while offset < value.len() && !scanner.limited {
        if offset >= row_end {
            // Only synopsis candidates carry command evidence across hard rows.
            command_line &= scanner.optional > 0;
            row_end = value[offset..]
                .find('\n')
                .map_or(value.len(), |end| offset + end + 1);
        }
        offset = scanner
            .protected(offset, command_line)
            .unwrap_or_else(|| scanner.token(offset, &mut command_line));
    }
    if scanner.limited {
        Vec::new()
    } else {
        scanner.accents
    }
}

struct Scanner<'a> {
    value: &'a str,
    accents: Vec<Accent>,
    frames: Vec<Frame>,
    optional: usize,
    limited: bool,
}

impl Scanner<'_> {
    /// Quotes, comments, escapes and explicit markup own their bytes before
    /// ordinary token classification can interpret structural punctuation.
    fn protected(&mut self, offset: usize, command_line: bool) -> Option<usize> {
        if let Some(Frame::Quote(quote)) = self.frames.last().copied() {
            return Some(self.quoted_step(offset, quote));
        }
        let value = self.value;
        let rest = &value[offset..];
        let first = rest.chars().next().expect("remaining text");
        let parameter = matches!(self.mode(), Mode::Parameter { .. });
        let shell = matches!(self.mode(), Mode::Command { .. } | Mode::Arithmetic);
        let mut end = offset + first.len_utf8();
        let mut role = None;
        if first == '\\' {
            // Literal escapes do not become structural punctuation.
            end += value[end..].chars().next().map_or(0, char::len_utf8);
        } else if matches!(first, '\'' | '"' | '`') {
            self.push_frame(Frame::Quote(first));
            role = Some(StyleRole::CodeString);
        } else if !parameter && !shell && rest.starts_with("/*") && boundary(value, offset) {
            end = rest
                .find("*/")
                .map_or(value.len(), |close| offset + close + 2);
            role = Some(StyleRole::CodeComment);
        } else if !parameter
            && !shell
            && (rest.starts_with("//") && boundary(value, offset)
                || first == ';'
                    && line_start(value, offset)
                    && !rest[1..]
                        .split('\n')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .is_empty())
        {
            end = offset + rest.find('\n').unwrap_or(rest.len());
            role = Some(StyleRole::CodeComment);
        } else if !parameter && first == '#' && boundary(value, offset) {
            if !shell
                && line_start(value, offset)
                && let Some(end) = self.directive(offset)
            {
                return Some(end);
            }
            end = offset + rest.find('\n').unwrap_or(rest.len());
            role = Some(StyleRole::CodeComment);
        } else if let Some(length) = placeholder(rest) {
            self.placeholder(offset, length, 2);
            return Some(offset + length);
        } else if first == '<'
            && command_line
            && boundary(value, offset)
            && let Some(length) = angle_placeholder(rest)
        {
            self.placeholder(offset, length, 1);
            return Some(offset + length);
        } else {
            return self.expansion(offset, false);
        }
        if let Some(role) = role {
            self.push(offset..end, role);
        }
        Some(end)
    }

    /// Classify one unprotected token; every branch advances unchanged UTF-8.
    fn token(&mut self, offset: usize, command_line: &mut bool) -> usize {
        let value = self.value;
        let rest = &value[offset..];
        let first = rest.chars().next().expect("remaining text");
        let mode = self.mode();
        let parameter = matches!(mode, Mode::Parameter { .. });
        let mut end = offset + first.len_utf8();
        let mut role = None;
        if first == '$'
            && line_start(value, offset)
            && rest[1..].chars().next().is_none_or(char::is_whitespace)
        {
            role = Some(StyleRole::CodePrompt);
        } else if let Some(length) = variable(rest) {
            end = offset + length;
            role = Some(StyleRole::CodeVariable);
        } else if !parameter && let Some(length) = option(value, offset) {
            end = offset + length;
            role = Some(StyleRole::CodeOption);
            *command_line = true;
        } else if first.is_ascii_digit()
            && matches!(
                self.frames.last(),
                Some(Frame::Parameter {
                    name_seen: false,
                    ..
                })
            )
        {
            end = offset + number(rest);
            role = Some(StyleRole::CodeVariable);
            self.consume_head();
        } else if first.is_ascii_digit()
            && (boundary(value, offset)
                || matches!(mode, Mode::Arithmetic | Mode::Parameter { operand: true }))
        {
            end = offset + number(rest);
            role = Some(StyleRole::CodeNumber);
        } else if first.is_alphabetic() || first == '_' {
            (end, role) = self.word(offset, mode);
            *command_line |= role == Some(StyleRole::CodeCommand);
            self.consume_head();
        } else if matches!(first, '(' | '[' | '{') {
            let optional = first == '['
                && *command_line
                && !rest.starts_with("[[")
                && !value[..offset].ends_with('[')
                && (boundary(value, offset) || self.optional > 0);
            self.push_frame(Frame::Group {
                close: match first {
                    '(' => ')',
                    '[' => ']',
                    _ => '}',
                },
                optional,
            });
            role = Some(StyleRole::CodeDelimiter);
        } else if matches!(first, ')' | ']' | '}') {
            self.close(first);
            role = Some(StyleRole::CodeDelimiter);
        } else if first == '|' && self.optional > 0 && !rest.starts_with("||") {
            role = Some(StyleRole::CodeDelimiter);
        } else if !parameter
            && !matches!(mode, Mode::Arithmetic)
            && let Some(length) = path_atom(rest)
        {
            end = offset + length;
        } else if let Some(length) = operator(value, offset, parameter) {
            end = offset + length;
            role = Some(StyleRole::CodeOperator);
            self.operator(&value[offset..end]);
        }
        if let Some(role) = role {
            self.push(offset..end, role);
        }
        end
    }

    fn mode(&self) -> Mode {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| match frame {
                Frame::Group { .. } => None,
                Frame::Parameter { operand, .. } => Some(Mode::Parameter { operand: *operand }),
                Frame::Command { head } => Some(Mode::Command { head: *head }),
                Frame::Arithmetic => Some(Mode::Arithmetic),
                Frame::Quote(_) => Some(Mode::Normal),
            })
            .unwrap_or(Mode::Normal)
    }

    fn push_frame(&mut self, frame: Frame) {
        if self.frames.len() == MAX_CONTEXT_DEPTH {
            self.limited = true;
            return;
        }
        if matches!(frame, Frame::Group { optional: true, .. }) {
            self.optional += 1;
        }
        self.frames.push(frame);
    }

    fn close(&mut self, close: char) {
        let matches = self.frames.last().is_some_and(|frame| match frame {
            Frame::Group {
                close: expected, ..
            } => *expected == close,
            Frame::Parameter { .. } => close == '}',
            Frame::Command { .. } | Frame::Arithmetic => close == ')',
            Frame::Quote(_) => false,
        });
        if matches && matches!(self.frames.pop(), Some(Frame::Group { optional: true, .. })) {
            self.optional -= 1;
        }
    }

    fn consume_head(&mut self) {
        for frame in self.frames.iter_mut().rev() {
            match frame {
                Frame::Group { .. } => {}
                Frame::Command { head } => {
                    *head = false;
                    break;
                }
                Frame::Parameter { name_seen, .. } => {
                    *name_seen = true;
                    break;
                }
                Frame::Quote(_) | Frame::Arithmetic => break,
            }
        }
    }

    fn operator(&mut self, operator: &str) {
        for frame in self.frames.iter_mut().rev() {
            match frame {
                Frame::Group { .. } => {}
                Frame::Parameter { operand, name_seen } => {
                    if *name_seen {
                        *operand = true;
                    }
                    break;
                }
                Frame::Command { head } => {
                    if matches!(operator, "|" | "|&" | "&&" | "||" | ";") {
                        *head = true;
                    }
                    break;
                }
                Frame::Quote(_) | Frame::Arithmetic => break,
            }
        }
    }

    fn expansion(&mut self, offset: usize, quoted: bool) -> Option<usize> {
        let rest = &self.value[offset..];
        let (marker, open, frame) = if rest.starts_with("${") {
            (
                StyleRole::CodeVariable,
                1,
                Frame::Parameter {
                    operand: false,
                    name_seen: false,
                },
            )
        } else if rest.starts_with("$(") {
            let arithmetic = rest.starts_with("$((");
            (
                StyleRole::CodeVariable,
                if arithmetic { 2 } else { 1 },
                if arithmetic {
                    Frame::Arithmetic
                } else {
                    Frame::Command { head: true }
                },
            )
        } else if !quoted && (rest.starts_with("<(") || rest.starts_with(">(")) {
            (StyleRole::CodeOperator, 1, Frame::Command { head: true })
        } else {
            return None;
        };
        self.push(offset..offset + 1, marker);
        self.push(offset + 1..offset + 1 + open, StyleRole::CodeDelimiter);
        self.push_frame(frame);
        if open == 2 {
            self.push_frame(Frame::Group {
                close: ')',
                optional: false,
            });
        }
        Some(offset + 1 + open)
    }

    fn word(&self, offset: usize, mode: Mode) -> (usize, Option<StyleRole>) {
        let value = self.value;
        let mut end = offset + take_while(&value[offset..], |c| c.is_alphanumeric() || c == '_');
        let url = value[end..].starts_with("://");
        let compound =
            value[end..].starts_with(['/', '.', '-']) || url || value[end..].starts_with("::");
        if compound && !matches!(mode, Mode::Parameter { .. } | Mode::Arithmetic) {
            end += take_while(&value[end..], |c| {
                !c.is_whitespace()
                    && !matches!(c, '\'' | '"' | '`' | ';')
                    && (url
                        || !matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '|' | '&'))
            });
        }
        let role = match mode {
            Mode::Arithmetic | Mode::Parameter { operand: false } => Some(StyleRole::CodeVariable),
            Mode::Parameter { operand: true } => Some(StyleRole::CodeString),
            _ if matches!(mode, Mode::Command { head: true })
                && super::command::expansion_head(&value[offset..end])
                || super::command::head(value, offset, end) =>
            {
                Some(StyleRole::CodeCommand)
            }
            _ if !compound
                && boundary(value, offset)
                && super::keywords::contains(&value[offset..end]) =>
            {
                Some(StyleRole::CodeKeyword)
            }
            _ => None,
        };
        (end, role)
    }

    fn directive(&mut self, start: usize) -> Option<usize> {
        let (length, include) = directive(&self.value[start..])?;
        self.push(start..start + length, StyleRole::CodeKeyword);
        let mut end = start + length;
        let header = self.value[end..].trim_start_matches([' ', '\t']);
        if include
            && header.starts_with('<')
            && let Some(close) = header.split('\n').next().unwrap_or_default().find('>')
        {
            let start = self.value.len() - header.len();
            end = start + close + 1;
            self.push(start..end, StyleRole::CodeString);
        }
        Some(end)
    }

    fn push(&mut self, bytes: std::ops::Range<usize>, role: StyleRole) {
        if bytes.is_empty() {
            return;
        }
        if let Some(last) = self.accents.last_mut()
            && last.role == role
            && last.bytes.end == bytes.start
        {
            last.bytes.end = bytes.end;
        } else {
            self.accents.push(Accent { bytes, role });
        }
    }

    fn placeholder(&mut self, offset: usize, length: usize, delimiters: usize) {
        self.push(offset..offset + delimiters, StyleRole::CodeDelimiter);
        let start = offset + delimiters;
        let end = offset + length - delimiters;
        let choices = delimiters == 2
            && self.value[start..end].starts_with('[')
            && self.value[start..end].ends_with(']')
            && self.value[start..end].contains('|');
        if choices {
            self.push(start..start + 1, StyleRole::CodeDelimiter);
            let mut cursor = start + 1;
            while cursor < end - 1 {
                if let Some(length) = option(self.value, cursor) {
                    self.push(cursor..cursor + length, StyleRole::CodeOption);
                    cursor += length;
                } else {
                    let c = self.value[cursor..]
                        .chars()
                        .next()
                        .expect("choice character");
                    self.push(
                        cursor..cursor + c.len_utf8(),
                        if c == '|' {
                            StyleRole::CodeDelimiter
                        } else {
                            StyleRole::CodePlaceholder
                        },
                    );
                    cursor += c.len_utf8();
                }
            }
            self.push(end - 1..end, StyleRole::CodeDelimiter);
        } else {
            let mut cursor = start;
            for (index, _) in self.value[start..end].match_indices('|') {
                if delimiters == 1 {
                    let choice = start + index;
                    self.push(cursor..choice, StyleRole::CodePlaceholder);
                    self.push(choice..choice + 1, StyleRole::CodeDelimiter);
                    cursor = choice + 1;
                }
            }
            self.push(cursor..end, StyleRole::CodePlaceholder);
        }
        self.push(end..offset + length, StyleRole::CodeDelimiter);
    }

    fn quoted_step(&mut self, offset: usize, quote: char) -> usize {
        let rest = &self.value[offset..];
        let first = rest.chars().next().expect("quoted character");
        let mut end = offset + first.len_utf8();
        if first == '\\' {
            end += self.value[end..].chars().next().map_or(0, char::len_utf8);
        } else if first == quote {
            self.frames.pop();
        } else if quote != '\'' {
            if let Some(end) = self.expansion(offset, true) {
                return end;
            }
            if let Some(length) = variable(rest) {
                self.push(offset..offset + length, StyleRole::CodeVariable);
                return offset + length;
            }
        }
        if let Some(length) = placeholder(rest) {
            self.placeholder(offset, length, 2);
            return offset + length;
        }
        self.push(offset..end, StyleRole::CodeString);
        end
    }
}

fn path_atom(value: &str) -> Option<usize> {
    (value.starts_with("./")
        || value.starts_with("//")
        || value.starts_with("../")
        || value.starts_with('/')
            && value[1..].starts_with(|c: char| c.is_alphanumeric() || c == '_'))
    .then(|| {
        take_while(value, |c| {
            !c.is_whitespace()
                && !matches!(
                    c,
                    '\'' | '"'
                        | '`'
                        | '('
                        | ')'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '<'
                        | '>'
                        | '|'
                        | '&'
                        | ';'
                )
        })
    })
}

fn operator(value: &str, offset: usize, parameter: bool) -> Option<usize> {
    let rest = &value[offset..];
    let first = rest.chars().next()?;
    if first == '-'
        && boundary(value, offset)
        && rest[1..].starts_with(|c: char| c.is_ascii_digit())
    {
        return None;
    }
    if !(matches!(
        first,
        '|' | '&'
            | '<'
            | '>'
            | '+'
            | '-'
            | '*'
            | '/'
            | '%'
            | '='
            | '!'
            | '^'
            | '~'
            | ':'
            | ','
            | ';'
            | '?'
    ) || parameter && first == '#')
    {
        return None;
    }
    Some(
        [
            "<<<", "<<-", "&&", "||", "|&", ">>", "<<", "<>", ">|", ">&", "<&", "==", "!=", "<=",
            ">=", "+=", "-=", "*=", "/=", "%=", ":-", ":=", ":?", ":+",
        ]
        .into_iter()
        .find(|operator| rest.starts_with(operator))
        .map_or(1, str::len),
    )
}

fn boundary(value: &str, offset: usize) -> bool {
    value[..offset]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric() && !matches!(c, '_' | '.' | '/' | '-' | ':' | '$'))
}

pub(super) fn line_start(value: &str, offset: usize) -> bool {
    value[..offset]
        .chars()
        .rev()
        .take_while(|c| *c != '\n')
        .all(char::is_whitespace)
}

fn take_while(value: &str, predicate: impl Fn(char) -> bool) -> usize {
    value
        .char_indices()
        .find_map(|(offset, c)| (!predicate(c)).then_some(offset))
        .unwrap_or(value.len())
}

fn directive(value: &str) -> Option<(usize, bool)> {
    let rest = value[1..].trim_start_matches([' ', '\t']);
    let length = take_while(rest, char::is_alphabetic);
    let word = &rest[..length];
    matches!(
        word,
        "include"
            | "define"
            | "if"
            | "ifdef"
            | "ifndef"
            | "else"
            | "elif"
            | "endif"
            | "pragma"
            | "error"
            | "warning"
            | "line"
            | "undef"
    )
    .then_some((value.len() - rest.len() + length, word == "include"))
}

pub(super) fn placeholder(value: &str) -> Option<usize> {
    let rest = value.strip_prefix("{{")?;
    let close = rest.find(['{', '}', '\n', '\r'])?;
    (rest[close..].starts_with("}}") && rest[..close].chars().any(char::is_alphabetic))
        .then_some(2 + close + 2)
}

pub(super) fn angle_placeholder(value: &str) -> Option<usize> {
    let rest = value.strip_prefix('<')?;
    let length = take_while(rest, |c| {
        c.is_alphanumeric() || matches!(c, '_' | '-' | '/' | '.' | '|')
    });
    (length > 0 && rest[length..].starts_with('>')).then_some(length + 2)
}

fn variable(value: &str) -> Option<usize> {
    let rest = value.strip_prefix('$')?;
    let first = rest.chars().next()?;
    if first.is_ascii_alphabetic() || first == '_' {
        Some(1 + take_while(rest, |c| c.is_ascii_alphanumeric() || c == '_'))
    } else {
        (first.is_ascii_digit() || matches!(first, '?' | '#' | '@' | '*' | '$' | '!' | '-'))
            .then_some(2)
    }
}

pub(super) fn option(value: &str, offset: usize) -> Option<usize> {
    if value[..offset]
        .chars()
        .next_back()
        .is_some_and(|c| !c.is_whitespace() && !matches!(c, '[' | '|'))
    {
        return None;
    }
    let rest = value[offset..].strip_prefix('-')?;
    let name = rest.strip_prefix('-').unwrap_or(rest);
    if name.is_empty() || name.starts_with(char::is_whitespace) {
        return rest.starts_with('-').then_some(2);
    }
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let length = take_while(name, |c| {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-')
    });
    if name[length..].starts_with(['(', ')', ';', '.', '/']) {
        return None;
    }
    Some(1 + rest.len() - name.len() + length)
}

fn number(value: &str) -> usize {
    // Ordinary decimal/hexadecimal literals, not a numeric-language grammar.
    let hex = value.starts_with("0x") || value.starts_with("0X");
    let prefix = if hex { 2 } else { 0 };
    prefix
        + take_while(&value[prefix..], |c| {
            if hex {
                c.is_ascii_hexdigit() || c == '_'
            } else {
                c.is_ascii_digit() || matches!(c, '.' | '_')
            }
        })
}
