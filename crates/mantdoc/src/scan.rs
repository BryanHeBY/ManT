//! Byte-oriented physical-line scanning and roff argument tokenization.

use crate::Limits;

/// One physical source line classified without decoding its bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScannedLine<'a> {
    /// A source line whose byte length exceeds the configured scanner bound.
    TooLong {
        /// Zero-based byte offset of the first source byte in the line.
        start: u32,
        /// Zero-based exclusive byte offset after the physical line content.
        end: u32,
    },
    /// A non-control input line.
    Text {
        /// Zero-based byte offset of the first source byte in the line.
        start: u32,
        /// Zero-based exclusive byte offset after the physical line content.
        end: u32,
        /// Bytes excluding the terminating LF, when present.
        bytes: &'a [u8],
    },
    /// A roff request or macro invocation.
    Control {
        /// Zero-based byte offset of the first physical source byte in line.
        start: u32,
        /// Zero-based byte offset of the first request or macro byte.
        control_start: u32,
        /// Zero-based exclusive byte offset after the physical line content.
        end: u32,
        /// Whether the no-break control character introduced this line.
        no_break: bool,
        /// Raw request or macro bytes without the control character.
        name: &'a [u8],
        /// Raw horizontally trimmed bytes after the request or macro name.
        arguments: &'a [u8],
        /// Raw bytes after the request or macro name, including horizontal
        /// whitespace before and after the retained arguments.
        raw_arguments: &'a [u8],
        /// Zero-based byte offset of the first retained argument byte.
        argument_start: u32,
    },
    /// A complete control-line comment retained without interpretation.
    Comment {
        /// Zero-based byte offset of the first source byte in the line.
        start: u32,
        /// Zero-based exclusive byte offset after the physical line content.
        end: u32,
        /// Comment bytes after the control/comment spelling.
        bytes: &'a [u8],
    },
}

/// One physical line consumed while roff is in copy mode.
///
/// Copy-mode input deliberately bypasses control-character and escape-character
/// interpretation. The executor decides whether the retained bytes terminate
/// the definition or become part of its delayed body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RawLine<'a> {
    /// Zero-based byte offset of the first source byte in the line.
    pub(crate) start: u32,
    /// Zero-based exclusive byte offset after the physical line content.
    pub(crate) end: u32,
    /// Bytes excluding the terminating LF, when present.
    pub(crate) bytes: &'a [u8],
    /// Whether retaining this physical line would exceed scanner limits.
    pub(crate) too_long: bool,
}

/// Stateful byte scanner for one source.
///
/// Only the roff control/no-break/escape characters affect this stage. Macro
/// meaning, expansion, and structural parsing stay in later stages.
pub(crate) struct Scanner<'a> {
    source: &'a [u8],
    cursor: usize,
    pending_line: Option<ScannedLine<'a>>,
    control: u8,
    no_break_control: u8,
    escape: u8,
    max_line_bytes: usize,
}

impl<'a> Scanner<'a> {
    /// Construct a scanner with ordinary roff control characters.
    pub(crate) const fn new(source: &'a [u8], limits: &Limits) -> Self {
        Self {
            source,
            cursor: 0,
            pending_line: None,
            control: b'.',
            no_break_control: b'\'',
            escape: b'\\',
            max_line_bytes: limits.max_line_bytes,
        }
    }

    /// Return the current escape character after any prior `.ec` request.
    pub(crate) const fn escape_character(&self) -> u8 {
        self.escape
    }

    /// Return the current ordinary control character after prior `.cc` input.
    pub(crate) const fn control_character(&self) -> u8 {
        self.control
    }

    /// Snapshot the mutable character spellings without changing input cursor.
    pub(crate) const fn character_state(&self) -> (u8, u8, u8) {
        (self.control, self.no_break_control, self.escape)
    }

    /// Restore a prior control, no-break-control, and escape spelling snapshot.
    pub(crate) fn restore_character_state(&mut self, state: (u8, u8, u8)) {
        (self.control, self.no_break_control, self.escape) = state;
    }

    /// Consume one physical line without interpreting control or escape state.
    ///
    /// This is used only for `.de`/`.am` copy mode. In particular, a literal
    /// `.cc` retained in a macro body must not change the caller's scanner
    /// before that macro is invoked.
    pub(crate) fn next_raw_line(&mut self) -> Option<RawLine<'a>> {
        if self.cursor >= self.source.len() {
            return None;
        }
        let start = self.cursor;
        let newline = self.source[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(self.source.len(), |offset| start + offset);
        self.cursor = newline.saturating_add(1).min(self.source.len());
        let bytes = &self.source[start..newline];
        let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
        Some(RawLine {
            start: u32::try_from(start).expect("parser checks public span offsets first"),
            end: u32::try_from(start.saturating_add(bytes.len()))
                .expect("parser checks public span offsets first"),
            bytes,
            too_long: bytes.len() > self.max_line_bytes,
        })
    }

    /// Scan the next physical input line.
    pub(crate) fn next_line(&mut self) -> Option<ScannedLine<'a>> {
        if self.pending_line.is_some() {
            return self.pending_line.take();
        }
        if self.cursor >= self.source.len() {
            return None;
        }
        let start = self.cursor;
        let newline = self.source[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(self.source.len(), |offset| start + offset);
        self.cursor = newline.saturating_add(1).min(self.source.len());
        let bytes = &self.source[start..newline];
        let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
        let start = u32::try_from(start).expect("parser checks public span offsets first");
        let end = start
            .checked_add(u32::try_from(bytes.len()).expect("line length fits u32"))
            .expect("parser checks public span offsets first");
        if bytes.len() > self.max_line_bytes {
            return Some(ScannedLine::TooLong { start, end });
        }
        let Some(introducer) = bytes.first().copied() else {
            return Some(ScannedLine::Text { start, end, bytes });
        };
        let no_break = introducer == self.no_break_control;
        if introducer != self.control && !no_break {
            return Some(ScannedLine::Text { start, end, bytes });
        }
        let control_remainder = &bytes[1..];
        let remainder = trim_horizontal_space(control_remainder);
        let leading_control_space = control_remainder.len() - remainder.len();
        let control_start = start
            .checked_add(1)
            .and_then(|offset| {
                offset.checked_add(
                    u32::try_from(leading_control_space).expect("line length fits u32"),
                )
            })
            .expect("parser checks public span offsets first");
        // A roff comment begins immediately with either `\"` or `"`.
        // Generic control-name scanning deliberately permits the first
        // escape byte, so it would otherwise absorb `.\"attached comment`
        // as a request name rather than stopping at the comment marker.
        let comment_marker_length = if remainder.starts_with(&[self.escape, b'"']) {
            Some(2_usize)
        } else if remainder.starts_with(b"\"") {
            Some(1_usize)
        } else {
            None
        };
        if let Some(comment_marker_length) = comment_marker_length {
            return Some(ScannedLine::Comment {
                start: control_start
                    .checked_add(
                        u32::try_from(comment_marker_length - 1)
                            .expect("comment marker length fits public spans"),
                    )
                    .expect("parser checks public span offsets first"),
                end,
                bytes: &remainder[comment_marker_length..],
            });
        }
        let name_end = remainder
            .iter()
            .enumerate()
            .position(|(index, byte)| {
                byte.is_ascii_whitespace() || (index > 0 && *byte == self.escape)
            })
            .unwrap_or(remainder.len());
        let name = &remainder[..name_end];
        // `\\"` begins a roff input comment even after a macro argument.
        // It is processed before the request-specific argument grammar, so
        // formatter annotations such as `.IR troff s, \\" DWB` never become
        // visible macro arguments. Keep preceding horizontal space: callers
        // that distinguish spaces from tabs still receive the exact retained
        // argument prefix.
        let raw_arguments = strip_inline_comment(&remainder[name_end..], self.escape);
        let arguments = trim_horizontal_space(raw_arguments);
        let argument_start = control_start
            .checked_add(u32::try_from(name_end).expect("line length fits u32"))
            .and_then(|offset| {
                offset.checked_add(
                    u32::try_from(raw_arguments.len() - arguments.len())
                        .expect("line length fits u32"),
                )
            })
            .expect("parser checks public span offsets first");
        if is_comment_name(name, self.escape) {
            return Some(ScannedLine::Comment {
                // mandoc points a comment node at the final character of its
                // control spelling (`."` -> the quote), while retaining the
                // following horizontal space as comment text.
                start: control_start
                    .checked_add(u32::try_from(name_end - 1).expect("comment name is nonempty"))
                    .expect("parser checks public span offsets first"),
                end,
                bytes: &remainder[name_end..],
            });
        }
        self.apply_character_request(name, arguments);
        Some(ScannedLine::Control {
            start,
            control_start,
            end,
            no_break,
            name,
            arguments,
            raw_arguments,
            argument_start,
        })
    }

    /// Return one already-scanned physical line to the normal input stream.
    /// This is used by logical-line recovery when a possible continuation is
    /// followed by a non-text line; preserving it is safer than consuming an
    /// unrelated request merely to inspect its kind.
    pub(crate) fn unread_line(&mut self, line: ScannedLine<'a>) {
        debug_assert!(self.pending_line.is_none());
        self.pending_line = Some(line);
    }

    /// Apply one already-recognized control-character request.
    ///
    /// Macro execution uses this after delayed copy-mode bytes become active,
    /// so that character changes affect subsequent physical source lines but
    /// never the definition body that contained them.
    pub(crate) fn apply_character_request(&mut self, name: &[u8], arguments: &[u8]) {
        let Some(argument) = first_argument(arguments) else {
            match name {
                b"cc" => self.control = b'.',
                b"c2" => self.no_break_control = b'\'',
                b"ec" => self.escape = b'\\',
                _ => {}
            }
            return;
        };
        let Some(character) = argument.first().copied() else {
            return;
        };
        match name {
            b"cc" => self.control = character,
            b"c2" => self.no_break_control = character,
            b"ec" => self.escape = character,
            _ => {}
        }
    }
}

/// One argument token copied from a control-line argument list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Argument {
    /// Byte offset relative to the beginning of the raw argument slice.
    pub(crate) offset: usize,
    /// Whether the raw token started with an outer double quote.
    pub(crate) quoted: bool,
    /// The immediate horizontal-space byte after this token, if one ended
    /// the token.  Most request parsers need only normalized tokens, but a
    /// small number of roff requests distinguish a literal space from a tab.
    pub(crate) separator_after: Option<u8>,
    /// Whether the complete horizontal-whitespace run after this token
    /// contains a literal tab.  The first byte alone is not sufficient for
    /// mdoc column phrases such as `word <space><tab> next`.
    pub(crate) separator_contains_tab: bool,
    /// Literal tab bytes contained within this token, including a quoted
    /// phrase.  mdoc column parsing treats each as a cell boundary.
    pub(crate) embedded_tab_count: usize,
    /// Number of horizontal-whitespace bytes after this token.  This remains
    /// scanner-private metadata: mdoc's one-line display macros interpret a
    /// doubled separator as an `ARGS_PHRASE` boundary.
    pub(crate) separator_width: usize,
    /// Raw token bytes excluding outer quotes, with escapes preserved.
    pub(crate) bytes: Vec<u8>,
}

/// Recoverable argument-lexing issue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArgumentIssue {
    /// A quoted token reached end of line before its closing quote.
    UnterminatedQuote,
    /// Argument count or retained argument bytes exceeded a parser limit.
    Limit,
}

/// Lex byte arguments with roff-style quoted and escaped whitespace forms.
pub(crate) fn lex_arguments(
    bytes: &[u8],
    escape: u8,
    limits: &Limits,
) -> Result<Vec<Argument>, ArgumentIssue> {
    lex_arguments_inner(bytes, escape, limits, true)
}

/// Lex a user-macro invocation argument list.
///
/// This explicit entry point documents the delayed `\$` substitution call
/// site; it shares the same roff quote grammar as ordinary controls.
pub(crate) fn lex_user_macro_arguments(
    bytes: &[u8],
    escape: u8,
    limits: &Limits,
) -> Result<Vec<Argument>, ArgumentIssue> {
    lex_arguments_inner(bytes, escape, limits, true)
}

fn lex_arguments_inner(
    bytes: &[u8],
    escape: u8,
    limits: &Limits,
    doubled_quotes_are_literal: bool,
) -> Result<Vec<Argument>, ArgumentIssue> {
    let mut arguments = Vec::new();
    let mut cursor = 0;
    let mut retained_bytes = 0_usize;
    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        if arguments.len() >= limits.max_arguments {
            return Err(ArgumentIssue::Limit);
        }
        let offset = cursor;
        let quoted = bytes[cursor] == b'"';
        if quoted {
            cursor += 1;
        }
        let mut token = Vec::new();
        let mut closed = !quoted;
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            if byte == escape && cursor + 1 < bytes.len() {
                token.push(byte);
                token.push(bytes[cursor + 1]);
                cursor += 2;
                continue;
            }
            if quoted {
                if byte == b'"' {
                    // A doubled delimiter inside a quoted roff argument is
                    // one literal quote, not two adjacent argument bounds.
                    // This is especially important for later `\$` replay:
                    // `"one""one"` is one argument containing `one"one`.
                    if doubled_quotes_are_literal && bytes.get(cursor + 1) == Some(&b'"') {
                        token.push(b'"');
                        cursor += 2;
                        continue;
                    }
                    cursor += 1;
                    closed = true;
                    break;
                }
            } else if byte.is_ascii_whitespace() {
                break;
            }
            token.push(byte);
            cursor += 1;
        }
        if !closed {
            return Err(ArgumentIssue::UnterminatedQuote);
        }
        let separator_after = bytes.get(cursor).copied().filter(u8::is_ascii_whitespace);
        let separator_width = bytes[cursor..]
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        let separator_contains_tab = bytes[cursor..cursor + separator_width].contains(&b'\t');
        retained_bytes = retained_bytes.saturating_add(token.len());
        if retained_bytes > limits.max_argument_bytes {
            return Err(ArgumentIssue::Limit);
        }
        arguments.push(Argument {
            offset,
            quoted,
            separator_after,
            separator_contains_tab,
            embedded_tab_count: memchr::memchr_iter(b'\t', &token).count(),
            separator_width,
            bytes: token,
        });
    }
    Ok(arguments)
}

fn trim_horizontal_space(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !matches!(*byte, b' ' | b'\t'))
        .unwrap_or(bytes.len());
    &bytes[start..]
}

/// Remove a roff input comment from one request's argument tail.
///
/// The active escape followed by a double quote starts a comment before the
/// normal request argument parser sees quotes or whitespace.  Other escapes
/// are skipped as pairs so an escaped escape does not hide a following active
/// comment marker.
pub(crate) fn strip_inline_comment(bytes: &[u8], escape: u8) -> &[u8] {
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != escape {
            cursor += 1;
            continue;
        }
        if bytes.get(cursor + 1) == Some(&b'"') {
            return &bytes[..cursor];
        }
        cursor = cursor.saturating_add(2);
    }
    bytes
}

fn first_argument(bytes: &[u8]) -> Option<&[u8]> {
    let bytes = trim_horizontal_space(bytes);
    let end = bytes
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(bytes.len());
    (!bytes.is_empty()).then_some(&bytes[..end])
}

fn is_comment_name(name: &[u8], escape: u8) -> bool {
    name == b"\"" || name == [escape, b'"']
}

#[cfg(test)]
mod tests {
    use crate::Limits;

    use super::{
        ArgumentIssue, ScannedLine, Scanner, lex_arguments, lex_user_macro_arguments,
        strip_inline_comment,
    };

    #[test]
    fn scanner_tracks_dynamic_control_and_escape_characters() {
        let input = b".cc !\n!TH TITLE 1\n!ec @\ntext @\\ @\" quote\n";
        let mut scanner = Scanner::new(input, &Limits::default());
        assert!(matches!(
            scanner.next_line(),
            Some(ScannedLine::Control { name: b"cc", .. })
        ));
        assert!(matches!(
            scanner.next_line(),
            Some(ScannedLine::Control { name: b"TH", .. })
        ));
        assert!(matches!(
            scanner.next_line(),
            Some(ScannedLine::Control { name: b"ec", .. })
        ));
        assert!(matches!(
            scanner.next_line(),
            Some(ScannedLine::Text { .. })
        ));
        assert_eq!(scanner.escape_character(), b'@');
    }

    #[test]
    fn scanner_discards_only_the_carriage_return_in_crlf_input() {
        let limits = Limits::default();
        let mut scanner = Scanner::new(b".TH CRLF 1\r\nvisible\r\n", &limits);
        assert!(matches!(
            scanner.next_line(),
            Some(ScannedLine::Control {
                name: b"TH",
                arguments: b"CRLF 1",
                end: 10,
                ..
            })
        ));
        assert!(matches!(
            scanner.next_line(),
            Some(ScannedLine::Text {
                bytes: b"visible",
                ..
            })
        ));
    }

    #[test]
    fn scanner_accepts_horizontal_space_between_control_and_request_name() {
        let mut scanner = Scanner::new(b".  nr count -1\n", &Limits::default());
        assert!(matches!(
            scanner.next_line(),
            Some(ScannedLine::Control {
                name: b"nr",
                arguments: b"count -1",
                ..
            })
        ));
    }

    #[test]
    fn scanner_stops_a_control_name_before_an_adjacent_escape() {
        let mut scanner = Scanner::new(b".el\\{dummy\n", &Limits::default());
        assert!(matches!(
            scanner.next_line(),
            Some(ScannedLine::Control {
                name: b"el",
                arguments: b"\\{dummy",
                ..
            })
        ));
    }

    #[test]
    fn scanner_discards_input_comments_after_control_arguments() {
        let mut scanner = Scanner::new(b".IR troff s, \\\" DWB, Plan 9\n", &Limits::default());
        assert!(matches!(
            scanner.next_line(),
            Some(ScannedLine::Control {
                name: b"IR",
                arguments: b"troff s, ",
                raw_arguments: b" troff s, ",
                ..
            })
        ));
        assert_eq!(strip_inline_comment(b"one @\" ignored", b'@'), b"one ");
    }

    #[test]
    fn scanner_retains_control_comments_and_rejects_long_lines_without_copying() {
        let limits = Limits {
            max_line_bytes: 32,
            ..Limits::default()
        };
        let mut scanner = Scanner::new(
            b".\\\"attached comment\nlonger-than-the-configured-line-limit\n",
            &limits,
        );
        assert!(matches!(
            scanner.next_line(),
            Some(ScannedLine::Comment { .. })
        ));
        assert!(matches!(
            scanner.next_line(),
            Some(ScannedLine::TooLong { .. })
        ));
    }

    #[test]
    fn argument_lexer_preserves_escapes_and_quote_boundaries() {
        let arguments = lex_arguments(b"one \"two words\" three\\ four", b'\\', &Limits::default())
            .expect("bounded valid arguments");
        assert_eq!(arguments.len(), 3);
        assert_eq!(arguments[1].bytes, b"two words");
        assert_eq!(arguments[2].bytes, b"three\\ four");
        assert_eq!(arguments[0].separator_after, Some(b' '));
        assert_eq!(
            lex_arguments(b"name\tvalue", b'\\', &Limits::default()).unwrap()[0].separator_after,
            Some(b'\t')
        );
        assert!(
            lex_arguments(b"name \tvalue", b'\\', &Limits::default()).unwrap()[0]
                .separator_contains_tab
        );
        let arguments =
            lex_user_macro_arguments(b"\"one\"\"one\" \"\"\"two\"\"\"", b'\\', &Limits::default())
                .expect("doubled quotes stay within their outer argument");
        assert_eq!(arguments.len(), 2);
        assert_eq!(arguments[0].bytes, b"one\"one");
        assert_eq!(arguments[1].bytes, b"\"two\"");
    }

    #[test]
    fn argument_lexer_reports_unterminated_quotes() {
        assert_eq!(
            lex_arguments(b"\"unterminated", b'\\', &Limits::default()),
            Err(ArgumentIssue::UnterminatedQuote)
        );
    }
}
