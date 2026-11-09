// Copyright (c) 2011 Kristaps Dzonsons <kristaps@bsd.lv>
// Copyright (c) 2014, 2015, 2017 Ingo Schwarze <schwarze@openbsd.org>
//
// Permission to use, copy, modify, and distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

//! Native equation words execute in the surrounding formatter stream.
use libmandoc_rs::{EquationBox, EquationFont, EquationKind, EquationPosition};

use super::ownership::Annotation;
use super::{Font, InlineBuilder, word_input};
use libmandoc_rs::Node;
use mant_ir::Inline;
use std::cell::Cell;

pub(super) fn append_owned(builder: &mut InlineBuilder, node: &Node) {
    let Some(equation) = &node.equation else {
        return;
    };
    let pool = builder
        .zero_advance
        .annotations
        .get_or_insert_with(Default::default)
        .clone();
    let expression = crate::mandoc::equations::expression_from_ast(equation);
    let registered = pool.register_equation(expression, crate::mandoc::source_span(node));
    let incoming_backtracking = builder.zero_advance.is_armed();
    let receipt = Cell::new(WordExecution {
        words: 0,
        complete: true,
    });
    let (_, membership) = builder.append_semantic_word_scope(
        registered.key,
        |builder| {
            receipt.set(append_words(builder, equation));
        },
        |children| {
            pool.record_equation_projection(registered.key, &children, receipt.get().complete);
            vec![Inline::Link {
                target: registered.target.clone(),
                title: None,
                children,
            }]
        },
    );
    if receipt.get().words == 0 {
        // Empty mathematical structures retain their source identity after
        // acceptance. A configuration-only root never enters this scope.
        pool.record_equation_projection(registered.key, &[], receipt.get().complete);
        builder.append(vec![Inline::Link {
            target: registered.target.clone(),
            title: None,
            children: Vec::new(),
        }]);
    }
    let pending = builder.zero_advance.deferred_member_projection(membership);
    pool.record_equation_deferred(registered.key, &pending);
    // A single buffered atom can still be accepted intact at its next real
    // boundary. A backtracking entry that consumed internal word separation
    // or already overwrote its own glyph requires the accepted text view.
    if incoming_backtracking && (receipt.get().words != 1 || pending.is_empty()) {
        pool.mark_equation_incomplete(registered.key);
    }
    builder.zero_advance.bind_deferred(
        membership,
        &Annotation::Link {
            target: registered.target,
            title: None,
        },
        false,
    );
}

#[derive(Clone, Copy)]
pub(super) struct WordExecution {
    pub(super) words: usize,
    pub(super) complete: bool,
}

pub(super) fn append_words(builder: &mut InlineBuilder, equation: &EquationBox) -> WordExecution {
    let mut receipt = WordExecution {
        words: 0,
        complete: true,
    };
    append_box(builder, equation, None, None, None, &mut receipt);
    receipt
}

fn append_word(builder: &mut InlineBuilder, source: &str, receipt: &mut WordExecution) {
    receipt.words += 1;
    receipt.complete &= word_input::append_equation_word(builder, source);
}

fn append_box(
    builder: &mut InlineBuilder,
    equation: &EquationBox,
    parent: Option<&EquationBox>,
    previous: Option<&EquationBox>,
    next: Option<&EquationBox>,
    receipt: &mut WordExecution,
) {
    let fenced = needs_fences(equation, parent, previous, next);
    if fenced {
        if parent.is_some_and(|parent| parent.kind == EquationKind::Subexpression)
            && previous.is_some()
            || equation.kind == EquationKind::List
                && equation.children.first().is_some_and(|first| {
                    !matches!(first.kind, EquationKind::Pile | EquationKind::Matrix)
                })
                && previous.is_some_and(|previous| {
                    previous.kind == EquationKind::List
                        || previous.kind == EquationKind::Text
                            && previous.text.as_deref().is_some_and(|text| {
                                text.starts_with('\\')
                                    || text.starts_with(|ch: char| ch.is_ascii_alphabetic())
                            })
                })
        {
            builder.tighten_next_boundary();
        }
        append_word(builder, equation.left.as_deref().unwrap_or("("), receipt);
        builder.tighten_next_boundary();
    }
    let font = match equation.font {
        EquationFont::None => None,
        EquationFont::Roman => Some(Font::Regular),
        EquationFont::Bold | EquationFont::Fat => Some(Font::Strong),
        EquationFont::Italic => Some(Font::Emphasis),
    };
    let saved = font.map(|font| builder.font.push_scope(font));
    if let Some(text) = &equation.text {
        // strchr also matches the terminating NUL of an empty atom.
        if text
            .chars()
            .next()
            .is_none_or(|ch| "!\"'),.:;?]}".contains(ch))
        {
            builder.tighten_next_boundary();
        }
        append_word(builder, text, receipt);
        if text.ends_with(['\"', '\'', '(', '[', '{'])
            || previous.is_none() && (text.ends_with('-') || text.ends_with("\\[mi]"))
        {
            builder.tighten_next_boundary();
        }
    }
    append_operands(builder, equation, receipt);
    if let Some(saved) = saved {
        builder.font.pop_scope(saved);
    }
    if let Some(top) = &equation.top {
        builder.tighten_next_boundary();
        append_word(builder, top, receipt);
    }
    if equation.bottom.is_some() {
        builder.tighten_next_boundary();
        append_word(builder, "_", receipt);
    }
    if fenced {
        builder.tighten_next_boundary();
        append_word(builder, equation.right.as_deref().unwrap_or(")"), receipt);
        if parent.is_some_and(|parent| parent.kind == EquationKind::Subexpression) && next.is_some()
        {
            builder.tighten_next_boundary();
        }
    }
}

fn append_operands(
    builder: &mut InlineBuilder,
    equation: &EquationBox,
    receipt: &mut WordExecution,
) {
    if equation.position == EquationPosition::Sqrt {
        append_word(builder, "\\(sr", receipt);
        if !equation.children.is_empty() {
            builder.tighten_next_boundary();
            append_child(builder, equation, 0, receipt);
        }
    } else if equation.kind == EquationKind::Subexpression {
        if equation.children.is_empty() {
            return;
        }
        append_child(builder, equation, 0, receipt);
        builder.tighten_next_boundary();
        append_word(
            builder,
            match equation.position {
                EquationPosition::Over => "/",
                EquationPosition::Superscript | EquationPosition::To => "^",
                _ => "_",
            },
            receipt,
        );
        if equation.children.len() > 1 {
            builder.tighten_next_boundary();
            append_child(builder, equation, 1, receipt);
            if matches!(
                equation.position,
                EquationPosition::FromTo | EquationPosition::SubscriptSuperscript
            ) {
                builder.tighten_next_boundary();
                append_word(builder, "^", receipt);
                if equation.children.len() > 2 {
                    builder.tighten_next_boundary();
                    append_child(builder, equation, 2, receipt);
                }
            }
        }
    } else {
        let parent = if equation.kind == EquationKind::Matrix {
            equation
                .children
                .first()
                .filter(|first| first.kind == EquationKind::List && first.expected_args > 1)
                .unwrap_or(equation)
        } else {
            equation
        };
        for (index, child) in parent.children.iter().enumerate() {
            if equation.kind == EquationKind::Pile
                && child.kind == EquationKind::List
                && child.expected_args > 1
                && child.actual_args == 1
            {
                if !child.children.is_empty() {
                    append_child(builder, child, 0, receipt);
                }
            } else {
                append_child(builder, parent, index, receipt);
            }
        }
    }
}

fn append_child(
    builder: &mut InlineBuilder,
    parent: &EquationBox,
    index: usize,
    receipt: &mut WordExecution,
) {
    append_box(
        builder,
        &parent.children[index],
        Some(parent),
        index
            .checked_sub(1)
            .and_then(|index| parent.children.get(index)),
        parent.children.get(index + 1),
        receipt,
    );
}

fn needs_fences(
    equation: &EquationBox,
    parent: Option<&EquationBox>,
    previous: Option<&EquationBox>,
    next: Option<&EquationBox>,
) -> bool {
    equation.kind == EquationKind::List && equation.expected_args > 1
        || equation.kind == EquationKind::Pile && (previous.is_some() || next.is_some())
        || parent.is_some_and(|parent| {
            parent.position == EquationPosition::Sqrt
                || (equation.top.is_some() || equation.bottom.is_some())
                    && parent.kind == EquationKind::Subexpression
                    && parent.position != EquationPosition::Over
                    && next.is_some()
                || equation.kind == EquationKind::Subexpression
                    && equation.position != EquationPosition::Sqrt
                    && (parent.kind == EquationKind::List && equation.expected_args == 1
                        || parent.kind == EquationKind::Subexpression)
        })
}
