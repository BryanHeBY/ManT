use super::ScannedLine;

pub(super) enum SourceEvent<'source> {
    TooLong {
        start: u32,
        end: u32,
    },
    Text {
        start: u32,
        end: u32,
        bytes: &'source [u8],
    },
    Comment {
        start: u32,
        end: u32,
        bytes: &'source [u8],
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

impl<'source> From<ScannedLine<'source>> for SourceEvent<'source> {
    fn from(line: ScannedLine<'source>) -> Self {
        match line {
            ScannedLine::TooLong { start, end } => Self::TooLong { start, end },
            ScannedLine::Text { start, end, bytes } => Self::Text { start, end, bytes },
            ScannedLine::Comment { start, end, bytes } => Self::Comment { start, end, bytes },
            ScannedLine::Control {
                start,
                control_start,
                end,
                no_break: _,
                name,
                arguments,
                raw_arguments,
                argument_start,
            } => Self::Control(ControlEvent {
                start,
                control_start,
                end,
                name,
                request: RequestKind::classify(name),
                arguments,
                raw_arguments,
                argument_start,
            }),
        }
    }
}

pub(super) struct ControlEvent<'source> {
    pub(super) start: u32,
    pub(super) control_start: u32,
    pub(super) end: u32,
    pub(super) name: &'source [u8],
    pub(super) request: RequestKind,
    pub(super) arguments: &'source [u8],
    pub(super) raw_arguments: &'source [u8],
    pub(super) argument_start: u32,
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
    Other,
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
