// Copyright (c) 2010-2022, 2025, 2026 Ingo Schwarze <schwarze@openbsd.org>
// Copyright (c) 2008, 2009, 2010, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
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

//! Native cell writes and post-write ownership facts.

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mandoc) enum FieldCell {
    /// One printable graph with its terminal width.
    Graph { text: char, width: usize },
    /// Native `ASCII_HYPH`: its first real scan uses it as a width-break
    /// candidate and immediately converts it to '-' (term.c:307-324),
    /// including lookahead not accepted by that pass.
    Hyphen,
    /// An ordinary breakable blank (`bufferc(' ')`, term.c:576 and 574-576
    /// for empty operands).
    BreakableBlank,
    /// A non-breaking blank (`ASCII_NBRSP`: `\~`, `\0`, KEPT separators).
    /// Its first scan counts its width and normalizes `ASCII_NBRSP` to an
    /// ordinary blank (term.c:340-347); a later pass can break on that byte.
    NonBreakingBlank,
    /// A `\p` break marker (`bufferc('\n')`, term.c:657-658). A pass only
    /// arms its LOCAL `breakline` from it (304-306); `term_field` skips it.
    BreakMarker,
    /// A literal tab in the word (`bufferc('\t')` through `encode()`,
    /// term.c:946-958). `term_fill` advances to the next configured tab stop
    /// and counts it as a graph (term.c:337); the `term_flushln()` tail
    /// scan skips it while `TERMP_BRTRSP` moves `vbr` to the next stop
    /// (term.c:179-182). The periodic stops come from
    /// `term_tab_set(p, "T"); term_tab_set(p, ".5i")` (mdoc_term.c:257-259,
    /// man_term.c:160-162): 120 basic units, 24 per character cell.
    Tab,
    /// Source-row reference inserted by `term_tab_ref()` (term.c:873-878).
    /// It changes the local tab origin, not text or graph occupancy.
    TabReference,
    /// `ASCII_NBRZW`: a native buffer cell and graph with zero width.
    ZeroWidthGraph,
    /// A zero-width breakpoint `\:` on the ascii device (`ASCII_BREAK`,
    /// term.c:287-300). Shares the breakable-blank arm with no width of
    /// its own: a pass may break at it, records it as the resume candidate
    /// after a graph, and never prints it (term.c:396-398). Unproduced
    /// while mant is single-device UTF-8 (`\:` buffers `ASCII_NBRZW`
    /// there, chars.c:53); preserved as the -Tascii implementation point.
    #[expect(dead_code)]
    Breakpoint,
    /// The `'\b'` `encode1()` buffers when a BACKBEFORE retreat meets a
    /// non-blank predecessor (term.c:906): fill subtracts the width of the
    /// cell before it (283-286), then the following graph adds its own.
    Backline,
}

/// Writes produced by the text executor before semantic projection.
#[derive(Clone, Debug)]
pub(in crate::mandoc) enum FieldWrite {
    Cell(FieldCell),
    /// A generated Unicode escaped space whose semantic column may already
    /// belong to the detached HEAD. It always executes encode1(U+00A0).
    OwnedBlank {
        projected: bool,
    },
    /// Unknown SPECIAL/invalid NUMBERED buffers `ASCII_NBRZW` directly.
    /// Recovery spelling owns output scalars but is never native width or
    /// an encode1 glyph (term.c:610-638).
    RecoveryGlyph {
        projected_scalars: usize,
    },
    /// A breakable source blank consumed by a marker's pass boundary. Its
    /// cell participates in acceptance, but has no authored output scalar.
    UnprojectedBlank,
    ArmBackafter,
    CancelBackafter,
}

impl FieldWrite {
    pub(in crate::mandoc) fn literal(value: &str) -> Vec<Self> {
        value.chars().map(Self::literal_cell).collect()
    }

    pub(in crate::mandoc) fn append_literal(writes: &mut Vec<Self>, value: &str) {
        writes.extend(value.chars().map(Self::literal_cell));
    }

    pub(in crate::mandoc) fn append_character(writes: &mut Vec<Self>, character: char) {
        writes.push(Self::literal_cell(character));
    }

    fn literal_cell(character: char) -> Self {
        Self::Cell(match character {
            ' ' => FieldCell::BreakableBlank,
            '\n' => FieldCell::BreakMarker,
            '\t' => FieldCell::Tab,
            // The frozen Unicode reader executes escaped spaces through
            // ESCAPE_SPECIAL -> encode1(U+00A0), not bufferc(ASCII_NBRSP).
            // KEEP's automatic separators remain direct buffered cells.
            '\u{8}' => FieldCell::Backline,
            // Printable ASCII occupies exactly one cell on the frozen
            // Unicode reader. Control sentinels and Unicode retain their
            // separate paths; no text-width scan is needed for this scalar.
            '!'..='~' => FieldCell::Graph {
                text: character,
                width: 1,
            },
            _ => {
                let mut utf8 = [0u8; 4];
                FieldCell::Graph {
                    text: character,
                    width: mant_ir::geometry::text_width(character.encode_utf8(&mut utf8)),
                }
            }
        })
    }
}
/// Post-execution ownership interval of one word's writes: the landing
/// facts the recorder must register its anchor from, never a prediction
/// made before execution (a BACKBEFORE retreat can pop the separator
/// blank or a previous word's trailing blank, term.c:901-908).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::mandoc::inline::flow) struct WordWriteReceipt {
    /// Index of the first cell this word's writes produced that survives
    /// execution with projection ownership. A leading `Backline` is a
    /// zero-width pairing cell, so the paired graph owns the content.
    /// Equal to `end_cell` when the writes produced no cell.
    pub(in crate::mandoc::inline::flow) first_content_cell: usize,
    /// `cells.len()` after the writes executed.
    pub(in crate::mandoc::inline::flow) end_cell: usize,
    /// `term_field()` writes deferred vbl only at a Graph or backspace.
    /// NBRZW contributes graph acceptance, but never prints this padding.
    pub(in crate::mandoc::inline::flow) prints_padding: bool,
}
