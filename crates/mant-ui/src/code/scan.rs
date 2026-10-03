//! Bounded lexical cues for mixed manual examples, not a language grammar.

use super::{Accent, StyleRole};

pub(super) fn accents(value: &str) -> Vec<Accent> {
    let mut scanner = Scanner {
        value,
        accents: Vec::new(),
    };
    let mut offset = 0;
    let mut brackets = 0_usize;
    let mut command_line = false;
    let mut row_end = 0;
    while offset < value.len() {
        if offset >= row_end {
            // Git synopses can continue an optional group over hard rows.
            command_line &= brackets > 0;
            row_end = value[offset..]
                .find('\n')
                .map_or(value.len(), |end| offset + end + 1);
        }
        let rest = &value[offset..];
        let first = rest.chars().next().expect("remaining text");
        let mut end = offset + first.len_utf8();
        let mut role = None;
        if first == '\\' {
            // Escaped markers and quotes stay literal, without rewriting them.
            end += value[end..].chars().next().map_or(0, char::len_utf8);
        } else if matches!(first, '\'' | '"' | '`') {
            offset = scanner.quoted(offset, first);
            continue;
        } else if rest.starts_with("/*") && boundary(value, offset) {
            end = rest
                .find("*/")
                .map_or(value.len(), |close| offset + close + 2);
            role = Some(StyleRole::CodeComment);
        } else if rest.starts_with("//") && boundary(value, offset)
            || first == ';'
                && line_start(value, offset)
                && !rest[1..]
                    .split('\n')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
        {
            end = offset + rest.find('\n').unwrap_or(rest.len());
            role = Some(StyleRole::CodeComment);
        } else if first == '#' && boundary(value, offset) {
            if line_start(value, offset)
                && let Some(end) = scanner.directive(offset)
            {
                offset = end;
                continue;
            }
            end = offset + rest.find('\n').unwrap_or(rest.len());
            role = Some(StyleRole::CodeComment);
        } else if let Some(length) = placeholder(rest) {
            scanner.placeholder(offset, length, 2);
            offset += length;
            continue;
        } else if first == '<'
            && command_line
            && boundary(value, offset)
            && let Some(length) = angle_placeholder(rest)
        {
            scanner.placeholder(offset, length, 1);
            offset += length;
            continue;
        } else if first == '$'
            && line_start(value, offset)
            && rest[1..].chars().next().is_none_or(char::is_whitespace)
        {
            role = Some(StyleRole::CodePrompt);
        } else if let Some(length) = variable(rest) {
            end = offset + length;
            role = Some(StyleRole::CodeVariable);
        } else if let Some(length) = option(value, offset) {
            end = offset + length;
            role = Some(StyleRole::CodeOption);
            command_line = true;
        } else if first.is_ascii_digit() && boundary(value, offset) {
            end = offset + number(rest);
            role = Some(StyleRole::CodeNumber);
        } else if first.is_alphabetic() || first == '_' {
            (end, role) = scanner.word(offset);
            command_line |= role == Some(StyleRole::CodeCommand);
        } else if matches!(first, '[' | ']') || first == '|' && brackets > 0 {
            brackets = if first == '[' {
                brackets + 1
            } else if first == ']' {
                brackets.saturating_sub(1)
            } else {
                brackets
            };
            role = Some(StyleRole::CodeDelimiter);
        }
        if let Some(role) = role {
            scanner.push(offset..end, role);
        }
        offset = end;
    }
    scanner.accents
}

struct Scanner<'a> {
    value: &'a str,
    accents: Vec<Accent>,
}

impl Scanner<'_> {
    fn word(&self, offset: usize) -> (usize, Option<StyleRole>) {
        let value = self.value;
        let mut end = offset + take_while(&value[offset..], |c| c.is_alphanumeric() || c == '_');
        // Consume paths/URLs/compound names as opaque atoms. A hyphenated
        // command head may still qualify, but paths never supply keywords.
        let compound = value[end..].starts_with(['/', '.', '-'])
            || value[end..].starts_with("://")
            || value[end..].starts_with("::");
        if compound {
            end += take_while(&value[end..], |c| {
                !c.is_whitespace() && !matches!(c, '\'' | '"' | '`' | ';' | ')' | ']' | '}')
            });
        }
        let role = if super::command::head(value, offset, end) {
            Some(StyleRole::CodeCommand)
        } else if !compound
            && boundary(value, offset)
            && super::keywords::contains(&value[offset..end])
        {
            Some(StyleRole::CodeKeyword)
        } else {
            None
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
        // Like tldr presentation, replaceable values are less emphasized than
        // command syntax. Unlike parse_tldr_command(), retain every marker and
        // both alternatives in {{[-c|--create]}}; never rewrite authored text.
        self.push(offset..offset + delimiters, StyleRole::CodeDelimiter);
        let mut start = offset + delimiters;
        let end = offset + length - delimiters;
        if delimiters == 1 {
            // Synopsis choices such as <PATH|-> retain their value text, with
            // only the choice separator taking the bracket/marker accent.
            for (index, _) in self.value[start..end].match_indices('|') {
                let choice = offset + delimiters + index;
                self.push(start..choice, StyleRole::CodePlaceholder);
                self.push(choice..choice + 1, StyleRole::CodeDelimiter);
                start = choice + 1;
            }
        }
        self.push(start..end, StyleRole::CodePlaceholder);
        self.push(
            offset + length - delimiters..offset + length,
            StyleRole::CodeDelimiter,
        );
    }

    fn quoted(&mut self, start: usize, quote: char) -> usize {
        let mut offset = start + 1;
        let mut chunk = start;
        while offset < self.value.len() {
            let rest = &self.value[offset..];
            let first = rest.chars().next().expect("remaining quote text");
            if first == '\\' {
                offset += 1;
                offset += self.value[offset..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8);
            } else if first == quote {
                offset += 1;
                break;
            } else if quote != '\''
                && let Some(length) = variable(rest)
            {
                self.push(chunk..offset, StyleRole::CodeString);
                self.push(offset..offset + length, StyleRole::CodeVariable);
                offset += length;
                chunk = offset;
            } else if let Some(length) = placeholder(rest) {
                self.push(chunk..offset, StyleRole::CodeString);
                self.placeholder(offset, length, 2);
                offset += length;
                chunk = offset;
            } else {
                offset += first.len_utf8();
            }
        }
        self.push(chunk..offset, StyleRole::CodeString);
        offset
    }
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
    if rest.starts_with('(') {
        // Accent the expansion opener, not the entire command/arithmetic body.
        return Some(if rest.starts_with("((") { 3 } else { 2 });
    }
    if let Some(braced) = rest.strip_prefix('{') {
        // Do not repeatedly search an unterminated/nested expansion's tail.
        // Inner expansions can still receive their own independent accent.
        let end = braced.find(['{', '}', '\n', '\r'])? + 1;
        return rest[end..].starts_with('}').then_some(end + 2);
    }
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
