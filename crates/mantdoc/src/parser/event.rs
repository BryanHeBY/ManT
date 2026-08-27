use std::borrow::Cow;

use super::{MacroSet, PackageToken, ScannedLine, Scanner, strip_inline_comment};

pub(super) enum SourceEvent<'source> {
    TooLong {
        start: u32,
        end: u32,
    },
    Text {
        start: u32,
        end: u32,
        bytes: Cow<'source, [u8]>,
        terminal_inline_conditional: bool,
        suppress_filled_text_tabs: bool,
    },
    Comment {
        start: u32,
        end: u32,
        bytes: Cow<'source, [u8]>,
    },
    Control(ControlEvent<'source>),
}

impl SourceEvent<'_> {
    pub(super) const fn range(&self) -> (u32, u32) {
        match self {
            Self::TooLong { start, end }
            | Self::Text { start, end, .. }
            | Self::Comment { start, end, .. } => (*start, *end),
            Self::Control(control) => (control.start, control.end),
        }
    }

    pub(super) fn is_else_request(&self) -> bool {
        matches!(
            self,
            Self::Control(control) if control.request == RequestKind::Else
        )
    }
}

impl<'source> SourceEvent<'source> {
    pub(super) fn from_scanned(line: ScannedLine<'source>, macro_set: MacroSet) -> Self {
        match line {
            ScannedLine::TooLong { start, end } => Self::TooLong { start, end },
            ScannedLine::Text { start, end, bytes } => Self::Text {
                start,
                end,
                bytes: Cow::Borrowed(bytes),
                terminal_inline_conditional: false,
                suppress_filled_text_tabs: false,
            },
            ScannedLine::Comment { start, end, bytes } => Self::Comment {
                start,
                end,
                bytes: Cow::Borrowed(bytes),
            },
            ScannedLine::Control {
                start,
                control_start,
                end,
                no_break: _,
                name,
                arguments,
                raw_arguments,
                argument_start,
            } => {
                let package = PackageToken::classify(macro_set, name);
                debug_assert!(package.name().is_none_or(|known| known == name));
                Self::Control(ControlEvent {
                    start,
                    control_start,
                    end,
                    name: Cow::Borrowed(name),
                    request: RequestKind::classify(name),
                    package,
                    arguments: Cow::Borrowed(arguments),
                    raw_arguments: Cow::Borrowed(raw_arguments),
                    argument_start,
                    generated: false,
                })
            }
        }
    }

    /// Reclassify an expanded same-line body as ordinary parser input.
    ///
    /// This is the owned counterpart of [`Self::from_scanned`].  Keeping it
    /// in the event layer lets conditional reruns use the same request
    /// dispatcher as physical source without copying ordinary source lines.
    pub(super) fn from_generated(
        bytes: Vec<u8>,
        start: u32,
        end: u32,
        macro_set: MacroSet,
        scanner: &mut Scanner<'source>,
        suppress_filled_text_tabs: bool,
    ) -> Self {
        let Some(introducer) = bytes.first().copied() else {
            return Self::Text {
                start,
                end,
                bytes: Cow::Owned(bytes),
                terminal_inline_conditional: true,
                suppress_filled_text_tabs,
            };
        };
        let no_break = introducer == scanner.no_break_control_character();
        if introducer != scanner.control_character() && !no_break {
            return Self::Text {
                start,
                end,
                bytes: Cow::Owned(bytes),
                terminal_inline_conditional: true,
                suppress_filled_text_tabs,
            };
        }
        let control_remainder = &bytes[1..];
        let remainder = trim_horizontal_space(control_remainder);
        let leading_control_space = control_remainder.len() - remainder.len();
        let control_start = start
            .saturating_add(1)
            .saturating_add(u32::try_from(leading_control_space).unwrap_or(u32::MAX));
        let escape = scanner.escape_character();
        let comment_marker_length = if remainder.starts_with(&[escape, b'"']) {
            Some(2_usize)
        } else if remainder.starts_with(b"\"") {
            Some(1_usize)
        } else {
            None
        };
        if let Some(comment_marker_length) = comment_marker_length {
            return Self::Comment {
                start: control_start.saturating_add(
                    u32::try_from(comment_marker_length.saturating_sub(1)).unwrap_or(u32::MAX),
                ),
                end,
                bytes: Cow::Owned(remainder[comment_marker_length..].to_vec()),
            };
        }
        let name_end = remainder
            .iter()
            .enumerate()
            .position(|(index, byte)| byte.is_ascii_whitespace() || (index > 0 && *byte == escape))
            .unwrap_or(remainder.len());
        let name = &remainder[..name_end];
        let raw_arguments = strip_inline_comment(&remainder[name_end..], escape);
        let arguments = trim_horizontal_space(raw_arguments);
        let argument_start = control_start
            .saturating_add(u32::try_from(name_end).unwrap_or(u32::MAX))
            .saturating_add(
                u32::try_from(raw_arguments.len() - arguments.len()).unwrap_or(u32::MAX),
            );
        if name == b"\"" || name == [escape, b'"'] {
            return Self::Comment {
                start: control_start
                    .saturating_add(u32::try_from(name_end.saturating_sub(1)).unwrap_or(u32::MAX)),
                end,
                bytes: Cow::Owned(remainder[name_end..].to_vec()),
            };
        }
        let name = name.to_vec();
        let arguments = arguments.to_vec();
        let raw_arguments = raw_arguments.to_vec();
        scanner.apply_character_request(&name, &arguments);
        let package = PackageToken::classify(macro_set, &name);
        let request = RequestKind::classify(&name);
        Self::Control(ControlEvent {
            start,
            control_start,
            end,
            name: Cow::Owned(name),
            request,
            package,
            arguments: Cow::Owned(arguments),
            raw_arguments: Cow::Owned(raw_arguments),
            argument_start,
            generated: true,
        })
    }
}

fn trim_horizontal_space(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !matches!(*byte, b' ' | b'\t'))
        .unwrap_or(bytes.len());
    &bytes[start..]
}

pub(super) struct ControlEvent<'source> {
    pub(super) start: u32,
    pub(super) control_start: u32,
    pub(super) end: u32,
    pub(super) name: Cow<'source, [u8]>,
    pub(super) request: RequestKind,
    pub(super) package: PackageToken,
    pub(super) arguments: Cow<'source, [u8]>,
    pub(super) raw_arguments: Cow<'source, [u8]>,
    pub(super) argument_start: u32,
    /// Whether this event was produced by same-line roff re-entry.
    pub(super) generated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RequestKind {
    While,
    If,
    Ie,
    Else,
    ControlCharacter,
    NoBreakControlCharacter,
    EscapeCharacter,
    OperatingSystem,
    Definition,
    Environment(EnvironmentRequest),
    Transparent(TransparentRequest),
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EnvironmentRequest {
    DefineString,
    AppendString,
    DefineRegister,
    RemoveRegister,
    Remove,
    Rename,
    Alias,
    FormatterState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TransparentRequest {
    Translation,
    Character,
    InputTrap,
}

impl EnvironmentRequest {
    pub(super) const fn name(self) -> &'static [u8] {
        match self {
            Self::DefineString => b"ds",
            Self::AppendString => b"as",
            Self::DefineRegister => b"nr",
            Self::RemoveRegister => b"rr",
            Self::Remove => b"rm",
            Self::Rename => b"rn",
            Self::Alias => b"als",
            // These requests are formatter-side no-ops in the parser. Their
            // exact spelling is immaterial to environment mutation.
            Self::FormatterState => b"",
        }
    }

    pub(super) const fn appends_string(self) -> bool {
        matches!(self, Self::AppendString)
    }
}

impl RequestKind {
    pub(super) fn classify(name: &[u8]) -> Self {
        match name {
            b"while" => Self::While,
            b"if" => Self::If,
            b"ie" => Self::Ie,
            b"el" => Self::Else,
            b"cc" => Self::ControlCharacter,
            b"c2" => Self::NoBreakControlCharacter,
            b"ec" => Self::EscapeCharacter,
            b"Os" => Self::OperatingSystem,
            b"de" | b"de1" | b"am" | b"dei" | b"ami" => Self::Definition,
            b"ds" => Self::Environment(EnvironmentRequest::DefineString),
            b"as" => Self::Environment(EnvironmentRequest::AppendString),
            b"nr" => Self::Environment(EnvironmentRequest::DefineRegister),
            b"rr" => Self::Environment(EnvironmentRequest::RemoveRegister),
            b"rm" => Self::Environment(EnvironmentRequest::Remove),
            b"rn" => Self::Environment(EnvironmentRequest::Rename),
            b"als" => Self::Environment(EnvironmentRequest::Alias),
            b"ftr" | b"na" | b"pl" | b"ps" => Self::Environment(EnvironmentRequest::FormatterState),
            b"tr" => Self::Transparent(TransparentRequest::Translation),
            b"char" => Self::Transparent(TransparentRequest::Character),
            b"it" => Self::Transparent(TransparentRequest::InputTrap),
            _ => Self::Other,
        }
    }

    pub(super) const fn is_definition(self) -> bool {
        matches!(self, Self::Definition)
    }

    pub(super) const fn owns_scope_continuation(self) -> bool {
        matches!(
            self,
            Self::While
                | Self::If
                | Self::Ie
                | Self::Else
                | Self::ControlCharacter
                | Self::NoBreakControlCharacter
                | Self::EscapeCharacter
        )
    }
}
