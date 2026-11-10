//! Private formatter coordinates. Source reachability is tested separately.

use super::{NoFillFormatterCell, NoFillInlineState, lower_no_fill_fragment_with_formatter};
use crate::mandoc::formatter::FormatterState;
use crate::mandoc::inline::flow::{AuthorBreakEffect, FieldFlags, InlineBuilder, WordEndBreak};
use crate::mandoc::inline::word_input::append_normalized_word;
use mant_ir::Inline;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Finish {
    Section,
    DetachedHead,
    UnconditionalField,
    RunIn,
}

impl Finish {
    fn effect(self) -> AuthorBreakEffect {
        if self == Self::Section {
            return AuthorBreakEffect::Line;
        }
        AuthorBreakEffect::Field {
            gap_cells: 1,
            body_width_columns: 12,
            field_width_columns: 12,
            flags: if self == Self::RunIn {
                FieldFlags::inset()
            } else {
                FieldFlags::man_head(true)
            },
        }
    }

    fn complete(self, formatter: &mut FormatterState, mut builder: InlineBuilder) {
        match self {
            Self::Section => {
                builder.hard_break();
                formatter.finish_inline_line_with_rows(builder, true);
            }
            Self::DetachedHead => {
                formatter.finish_inline_line_with_rows(builder, true);
            }
            Self::UnconditionalField => {
                formatter.finish_inline_field_flush(builder);
            }
            Self::RunIn => {
                let (_, preserved) = formatter.finish_inline_scope(builder);
                formatter.with_output_builder(&mut Vec::new(), |builder| {
                    builder.inherit_preserved_execution(preserved);
                });
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Successor {
    EmptyBody,
    InvisibleBody,
    VisibleBody,
    Eof,
    Heading,
}

impl Successor {
    fn observe(self, formatter: &mut FormatterState) -> Vec<Inline> {
        let mut output = Vec::new();
        match self {
            Self::EmptyBody => formatter
                .with_output_builder(&mut output, InlineBuilder::execute_native_literal_newline),
            Self::InvisibleBody | Self::VisibleBody => {
                let input = if matches!(self, Self::InvisibleBody) {
                    r"\&"
                } else {
                    "NEW"
                };
                word(formatter, &mut output, input);
                formatter
                    .no_fill_inline
                    .finish_row(&mut formatter.execution, &mut output);
            }
            Self::Eof => formatter
                .no_fill_inline
                .finish_row(&mut formatter.execution, &mut output),
            Self::Heading => {
                let mut builder =
                    formatter.begin_inline_session(true, false, AuthorBreakEffect::Line);
                append_normalized_word(&mut builder, "NEW");
                builder.hard_break();
                output = formatter.finish_inline_line_with_rows(builder, true).output;
            }
        }
        output
    }
}

fn word(formatter: &mut FormatterState, output: &mut Vec<Inline>, input: &str) {
    lower_no_fill_fragment_with_formatter(formatter, output, false, false, |builder| {
        append_normalized_word(builder, input);
    });
}

fn check_coordinate(
    finish: Finish,
    seed: Option<&str>,
    cell: NoFillFormatterCell,
    control: &str,
    successor: Successor,
) {
    let case = format!("{finish:?}, {seed:?}, {control:?}, {successor:?}");
    let mut formatter = FormatterState::default();
    formatter.no_fill = true;
    let mut old_output = Vec::new();
    if let Some(seed) = seed {
        word(&mut formatter, &mut old_output, seed);
    }
    let previous = formatter.no_fill_inline.clone();
    assert_eq!(previous.active, seed.is_some(), "{case}");
    assert_eq!(previous.formatter_cell, cell, "{case}");

    let mut builder = formatter.begin_inline_session(true, false, finish.effect());
    if finish != Finish::Section {
        builder.begin_definition_head_consumption();
    }
    if !control.is_empty() {
        append_normalized_word(&mut builder, control);
    }
    let wrote_input = !builder.execution.word_input().0.cells().is_empty();
    let consumed = wrote_input && finish != Finish::RunIn;
    let continuation = builder.execution.source_row_continues();
    let armed = builder.execution.zero_advance.is_armed();
    let buffered = builder.execution.zero_advance.has_buffered_glyph();
    let word_end_break = builder.execution.word_end_break;
    finish.complete(&mut formatter, builder);

    assert_eq!(
        formatter.no_fill_inline,
        if consumed {
            NoFillInlineState::new()
        } else {
            previous
        },
        "{case}"
    );
    assert_eq!(
        formatter.execution.source_row_continues(),
        continuation,
        "{case}"
    );
    if finish == Finish::RunIn {
        assert_eq!(formatter.execution.zero_advance.is_armed(), armed, "{case}");
        assert_eq!(
            formatter.execution.zero_advance.has_buffered_glyph(),
            buffered,
            "{case}"
        );
        assert_eq!(formatter.execution.word_end_break, word_end_break, "{case}");
        assert_eq!(
            !formatter.execution.word_input().0.cells().is_empty(),
            wrote_input,
            "{case}"
        );
    } else {
        assert!(
            !formatter.execution.zero_advance.has_buffered_glyph(),
            "{case}"
        );
        assert_ne!(
            formatter.execution.word_end_break,
            WordEndBreak::Pending,
            "{case}"
        );
        if !wrote_input && finish != Finish::UnconditionalField {
            assert_eq!(formatter.execution.zero_advance.is_armed(), armed, "{case}");
        }
    }

    // None of the old input reappears in a new owner. An invisible new
    // BODY still owns an empty row; the retired cell cannot eat that row.
    let output = successor.observe(&mut formatter);
    if consumed {
        match successor {
            Successor::EmptyBody | Successor::Eof => {
                assert!(output.is_empty(), "{case}: {output:?}");
            }
            Successor::VisibleBody | Successor::Heading => assert!(
                mant_ir::inline_plain_text(&output)
                    .trim_start_matches(' ')
                    .starts_with("NEW"),
                "{case}: {output:?}"
            ),
            Successor::InvisibleBody => assert_eq!(
                output
                    .iter()
                    .filter(|inline| matches!(inline, Inline::Text { value } if value.is_empty()))
                    .count(),
                1,
                "{case}: {output:?}"
            ),
        }
    }
}

#[test]
fn native_session_retirement_matrix() {
    // An empty term_word marks a fragment as active without writing a cell.
    // term.c::term_newln() guards the buffer flush; term_flushln() clears
    // the backtracking flags unconditionally. Neither resets NONEWLINE.
    // An inset/diag HEAD return does neither (mdoc_term.c::termp_it_post).
    let mut checked = 0;
    for finish in [
        Finish::Section,
        Finish::DetachedHead,
        Finish::UnconditionalField,
        Finish::RunIn,
    ] {
        for (seed, cell) in [
            (None, NoFillFormatterCell::Origin),
            (Some(""), NoFillFormatterCell::Origin),
            (Some(r"\&"), NoFillFormatterCell::Invisible),
            (Some("OLD"), NoFillFormatterCell::Visible),
        ] {
            for control in ["", r"\z", r"\zX", r"\p", r"\c"] {
                for successor in [
                    Successor::EmptyBody,
                    Successor::InvisibleBody,
                    Successor::VisibleBody,
                    Successor::Eof,
                    Successor::Heading,
                ] {
                    check_coordinate(finish, seed, cell, control, successor);
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 400);
}
