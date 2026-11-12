//! Conservative command-usage accents shared by displays and validated forms.
//!
//! These are presentation cues, not option arity or executable syntax. All
//! positions refer to unchanged UTF-8 source; no viewport or IR state is kept.

use std::ops::Range;

mod lex;
mod prose;
use lex::{Kind, Syntax, Token, tokenize};
pub(crate) use prose::accents as prose_accents;
pub(crate) use prose::checked_accents as checked_prose_accents;

const MAX_BYTES: usize = 64 * 1024;
const MAX_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Context {
    Inferred,
    Synopsis,
    Entry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Command,
    Option,
    Placeholder,
    Delimiter,
    Operator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Accent {
    pub(crate) bytes: Range<usize>,
    pub(crate) role: Role,
}

pub(crate) fn overlay(base: &mut Vec<Accent>, patches: Vec<Accent>) {
    crate::highlight::ranges::overlay(
        base,
        patches,
        |accent| accent.bytes.clone(),
        |accent, bytes| Accent {
            bytes,
            role: accent.role,
        },
    );
}

/// Scan one complete content owner. Names are exact protected spellings;
/// emphasis supplies evidence only after a command/form has been recognized.
pub(crate) fn accents(
    value: &str,
    context: Context,
    emphasis: &[Range<usize>],
    names: &[Range<usize>],
) -> Vec<Accent> {
    if value.len() > MAX_BYTES || !valid_ranges(value, emphasis) || !valid_ranges(value, names) {
        return Vec::new();
    }
    let Some(syntax) = tokenize(value, names) else {
        return Vec::new();
    };
    let mut accepted = vec![context == Context::Entry; syntax.tokens.len()];
    let mut commands = vec![false; syntax.tokens.len()];
    if context != Context::Entry {
        recognize(
            value,
            context,
            emphasis,
            &syntax,
            &mut accepted,
            &mut commands,
        );
    }
    let mut result: Vec<Accent> = Vec::new();
    for (index, token) in syntax.tokens.iter().enumerate() {
        if !accepted[index] || token.kind == Kind::Name {
            continue;
        }
        let text = &value[token.bytes.clone()];
        let group = token.group.map(|group| &syntax.groups[group]);
        let angle = group.is_some_and(|group| group.delimiter == '<');
        let choice = group.is_some_and(|group| group.choice);
        let emphasized = covered(emphasis, &token.bytes);
        let role = match token.kind {
            Kind::Word if commands[index] => Some(Role::Command),
            Kind::Word | Kind::Option if angle => Some(Role::Placeholder),
            Kind::Option => Some(Role::Option),
            Kind::Open(_) | Kind::Close | Kind::Choice | Kind::Repeat => Some(Role::Delimiter),
            Kind::Equals => Some(Role::Operator),
            Kind::Colon
                if syntax
                    .tokens
                    .get(index + 1)
                    .is_some_and(|next| next.kind == Kind::Equals) =>
            {
                Some(Role::Operator)
            }
            Kind::Word if text.chars().next().is_some_and(char::is_numeric) => None,
            Kind::Word => {
                // ip's -V[ersion] is a spelling abbreviation, not an argument.
                // Lowercase attached groups stay conservative even when the
                // author uses italics for that optional spelling suffix.
                let suffix =
                    group.is_some_and(|group| group.attached && !group.option && !uppercase(text));
                let argument = angle
                    || !suffix
                        && (emphasized
                            || !choice && uppercase(text)
                            || ((if choice {
                                choice_metavariable(text)
                            } else {
                                metavariable(text)
                            }) && (group.is_some()
                                || assignment(&syntax.tokens, index)
                                || context == Context::Synopsis))
                            || group.is_some_and(|group| group.option && !group.choice));
                argument.then_some(Role::Placeholder)
            }
            _ => None,
        };
        if let Some(role) = role {
            if let Some(previous) = result.last_mut()
                && previous.role == role
                && previous.bytes.end == token.bytes.start
            {
                previous.bytes.end = token.bytes.end;
            } else {
                result.push(Accent {
                    bytes: token.bytes.clone(),
                    role,
                });
            }
        }
    }
    result
}

fn valid_ranges(value: &str, ranges: &[Range<usize>]) -> bool {
    ranges.iter().all(|range| {
        range.start < range.end
            && range.end <= value.len()
            && value.is_char_boundary(range.start)
            && value.is_char_boundary(range.end)
    }) && ranges.windows(2).all(|pair| pair[0].end <= pair[1].start)
}

fn covered(ranges: &[Range<usize>], token: &Range<usize>) -> bool {
    let mut index = ranges.partition_point(|range| range.end <= token.start);
    let mut byte = token.start;
    while let Some(range) = ranges.get(index) {
        if range.start > byte {
            return false;
        }
        if range.end >= token.end {
            return true;
        }
        byte = range.end;
        index += 1;
    }
    false
}

fn recognize(
    value: &str,
    context: Context,
    emphasis: &[Range<usize>],
    syntax: &Syntax,
    accepted: &mut [bool],
    commands: &mut [bool],
) {
    let mut start = 0;
    let mut active = false;
    let mut base_indent = "";
    for end in (0..syntax.tokens.len())
        .filter(|index| syntax.tokens[*index].kind == Kind::Newline)
        .chain(std::iter::once(syntax.tokens.len()))
    {
        let row = &syntax.tokens[start..end];
        if row.is_empty() {
            active = false;
            start = end + 1;
            continue;
        }
        let first = &row[0];
        let head = &value[first.bytes.clone()];
        let line_start = value[..first.bytes.start]
            .rfind('\n')
            .map_or(0, |byte| byte + 1);
        let indent = &value[line_start..first.bytes.start];
        let invalid = row.iter().any(|token| {
            matches!(token.kind, Kind::Open('('))
                || value[token.bytes.clone()].contains([';', '#', '$', '&'])
                    && token.kind != Kind::Opaque
        });
        let strict_group = row.iter().enumerate().any(|(index, token)| {
            token.kind == Kind::Open('[')
                && value[token.bytes.end..].starts_with('-')
                && row.get(index + 1).is_some_and(|other| {
                    other.bytes.start == token.bytes.end && other.kind == Kind::Option
                })
        });
        let syntax_evidence = row
            .iter()
            .any(|token| matches!(token.kind, Kind::Open('[' | '<' | '{') | Kind::Option));
        let definition = context == Context::Synopsis
            && row.len() >= 4
            && first.kind == Kind::Word
            && row[1].kind == Kind::Colon
            && row[2].kind == Kind::Equals
            && matches!(row[3].kind, Kind::Open('{' | '['));
        let command = first.kind == Kind::Word
            && command(head)
            && row.len() > 1
            && signature_tail(value, row, emphasis)
            && if context == Context::Inferred {
                strict_group
            } else {
                syntax_evidence
            };
        let continuation = active
            && (context == Context::Synopsis
                || first.group.is_some()
                || indent.len() > base_indent.len() && indent.starts_with(base_indent))
            && (first.group.is_some()
                || matches!(first.kind, Kind::Open('[' | '<' | '{') | Kind::Choice));
        let standalone = matches!(first.kind, Kind::Open('[')) && strict_group;
        let valid = !invalid && (definition || command || continuation || standalone);
        accepted[start..end].fill(valid);
        if valid && command && !definition {
            commands[start] = true;
            base_indent = indent;
        }
        active = valid;
        start = end + 1;
    }
}

fn signature_tail(value: &str, row: &[Token], emphasis: &[Range<usize>]) -> bool {
    let mut structure = false;
    for token in &row[1..] {
        if matches!(token.kind, Kind::Open(_) | Kind::Option | Kind::Equals) {
            structure = true;
        }
        if structure && token.group.is_none() && token.kind == Kind::Word {
            let word = &value[token.bytes.clone()];
            if !uppercase(word) && !covered(emphasis, &token.bytes) && !metavariable(word) {
                return false;
            }
        }
    }
    true
}

fn assignment(tokens: &[Token], index: usize) -> bool {
    index > 0 && tokens[index - 1].kind == Kind::Equals
}

fn command(word: &str) -> bool {
    !word.is_empty()
        && word.chars().next().is_some_and(char::is_lowercase)
        && word
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-' | '.'))
        && !matches!(
            word,
            "struct"
                | "enum"
                | "union"
                | "typedef"
                | "template"
                | "namespace"
                | "class"
                | "const"
                | "static"
                | "extern"
                | "unsigned"
                | "signed"
                | "void"
                | "int"
                | "char"
                | "float"
                | "double"
                | "return"
                | "if"
                | "then"
                | "else"
                | "for"
                | "while"
                | "case"
                | "function"
        )
}

fn uppercase(word: &str) -> bool {
    word.chars().any(char::is_uppercase) && !word.chars().any(char::is_lowercase)
}

fn metavariable(word: &str) -> bool {
    let word = word.trim_end_matches('@').to_ascii_lowercase();
    word.contains('_')
        || common_metavariable(&word)
        || word.starts_with("script-")
        || word.starts_with("path/")
}

fn common_metavariable(word: &str) -> bool {
    matches!(
        word,
        "arg"
            | "args"
            | "argument"
            | "arguments"
            | "option"
            | "options"
            | "operand"
            | "operands"
            | "file"
            | "files"
            | "filename"
            | "filenames"
            | "path"
            | "paths"
            | "directory"
            | "dir"
            | "script"
            | "command"
            | "parameters"
            | "parameter"
            | "name"
            | "value"
            | "format"
            | "pattern"
            | "expression"
            | "target"
            | "source"
            | "destination"
            | "host"
            | "hostname"
            | "user"
            | "port"
            | "count"
            | "size"
            | "length"
            | "number"
            | "string"
            | "url"
            | "urls"
            | "repo"
            | "repository"
            | "device"
            | "mountpoint"
            | "level"
            | "standard"
    )
}

fn choice_metavariable(word: &str) -> bool {
    let word = word.to_ascii_lowercase();
    // Bash uses command_string alongside file. A composed conventional name
    // still identifies a slot, while CUSTOM_KIND and GET remain fixed choices.
    common_metavariable(&word) || word.contains('_') && word.split('_').all(common_metavariable)
}

#[cfg(test)]
mod tests;
