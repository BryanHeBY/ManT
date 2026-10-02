//! Syntax categories of pinned CVS `roff_escape.c::roff_escape_impl()`.
//!
//! Classification determines argument shape for both direct decoding and
//! nested scanning. Display policy stays with each consumer. In particular,
//! IGNORE is known syntax even when it produces no displayed character.

// Argument/category organization adapted from roff_escape.c:
// Copyright (c) 2011, 2012, 2013, 2014, 2015, 2017, 2018, 2020, 2022
//               Ingo Schwarze <schwarze@openbsd.org>
// Copyright (c) 2010, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
//
// Permission to use, copy, modify, and distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EscapeKind {
    Undefined,
    Error,
    Unsupported,
    Ignore,
    Expand,
    Special,
    Font,
    Break,
    NoSpace,
    ZeroAdvance,
    Numbered,
    Horizontal,
    HorizontalLine,
    Overstrike,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ArgumentShape {
    None,
    Standard,
    Counted(usize),
    Bracketed,
    Size,
    Quoted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct EscapeSyntax {
    pub(super) kind: EscapeKind,
    pub(super) argument: ArgumentShape,
}

/// The sole trigger classification. CVS separates syntax categories first,
/// then result types; no delimiter consumer maintains its own known-name list.
pub(super) const fn syntax(trigger: char) -> EscapeSyntax {
    use ArgumentShape::{Bracketed, Counted, None, Quoted, Size, Standard};
    use EscapeKind::{
        Break, Expand, Font, Horizontal, HorizontalLine, Ignore, NoSpace, Numbered, Overstrike,
        Special, Undefined, Unsupported, ZeroAdvance,
    };
    let (kind, argument) = match trigger {
        '!' | '?' | 'r' => (Unsupported, None),
        '%' | '&' | ')' | ',' | '/' | '^' | 'a' | 'd' | 't' | 'u' | '{' | '|' | '}' => {
            (Ignore, None)
        }
        ' ' | '\'' | '-' | '0' | ':' | '_' | '`' | 'e' | '~' => (Special, None),
        'p' => (Break, None),
        'c' => (NoSpace, None),
        'z' => (ZeroAdvance, None),
        '$' | '*' | 'V' | 'g' | 'n' => (Expand, Standard),
        'F' | 'M' | 'O' | 'Y' | 'k' | 'm' => (Ignore, Standard),
        '(' => (Special, Counted(2)),
        '[' => (Special, Bracketed),
        'f' => (Font, Standard),
        'A' | 'B' | 'w' => (Expand, Quoted),
        'D' | 'H' | 'L' | 'R' | 'S' | 'X' | 'Z' | 'b' | 'v' | 'x' => (Ignore, Quoted),
        'C' => (Special, Quoted),
        'N' => (Numbered, Quoted),
        'h' => (Horizontal, Quoted),
        'l' => (HorizontalLine, Quoted),
        'o' => (Overstrike, Quoted),
        's' => (Ignore, Size),
        _ => (Undefined, None),
    };
    EscapeSyntax { kind, argument }
}

/// `mandoc.c::mandoc_font()` accepts only complete known argument spellings.
/// C/V/VB/VI are the existing Pandoc extension from native patch 0013.
/// Display fonts remain the consumer's policy; an invalid spelling is ERROR
/// and must not change either terminal font register.
pub(super) fn valid_font_operand(operand: &[char]) -> bool {
    matches!(
        operand,
        [] | ['C' | 'V' | 'B' | '3' | 'I' | '2' | 'P' | 'R' | '1' | '4']
            | ['V', 'B' | 'I']
            | ['B', 'I']
            | ['C', 'B' | 'I' | 'R' | 'W']
    )
}
