//! Explicit mathematical blocks share the inline encoder's operation context.
use super::{BlockSyntax, MappedText, nonempty};
use crate::encode::{
    MarkdownMathIssueKind,
    inline::{code_span, fenced_code},
    math_markdown::{MathContext, RenderOptions},
};
use mant_ir::Block;

pub(super) fn render(block: &Block, options: RenderOptions<'_>) -> Option<MappedText> {
    let Block::Equation {
        value,
        display,
        expression,
        ..
    } = block
    else {
        return None;
    };
    let mathematical = options.math.and_then(|context| {
        expression.as_ref().map_or_else(
            || {
                context.missing(block);
                None
            },
            |expression| context.encoded(expression, value, *display),
        )
    });
    if options.math.is_some_and(MathContext::failed) {
        return None;
    }
    if let Some(body) = mathematical {
        let source = if options.options.math == crate::encode::MarkdownMath::Mathml {
            body.clone()
        } else if *display {
            format!("$${body}$$")
        } else {
            format!("Equation: ${body}$")
        };
        if mathematical_block_is_valid(&source, &body, *display, options.options.math) {
            Some(
                MappedText::from(source)
                    .syntax_site(BlockSyntax::Phrasing)
                    .tail_grammar(BlockSyntax::Phrasing, false),
            )
        } else {
            if let (Some(context), Some(expression)) = (options.math, expression) {
                context.reading(expression, value, MarkdownMathIssueKind::DelimiterContext);
            }
            if options.math.is_some_and(MathContext::failed) {
                return None;
            }
            portable_equation(value, *display)
        }
    } else {
        portable_equation(value, *display)
    }
}

fn portable_equation(value: &str, display: bool) -> Option<MappedText> {
    if display {
        Some(
            MappedText::from(fenced_code(value, None))
                .syntax_site(BlockSyntax::Fence)
                .tail_grammar(BlockSyntax::Fence, false),
        )
    } else {
        nonempty(format!("Equation: {}", code_span(value)))
    }
}

fn mathematical_block_is_valid(
    source: &str,
    body: &str,
    display: bool,
    mode: crate::encode::MarkdownMath,
) -> bool {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    if mode == crate::encode::MarkdownMath::Mathml {
        return super::super::math_markdown::mathml::is_phrasing(source);
    }
    let mut found = false;
    for event in Parser::new_ext(source, Options::ENABLE_MATH) {
        match event {
            Event::DisplayMath(actual) if display && !found && actual.as_ref() == body => {
                found = true;
            }
            Event::InlineMath(actual) if !display && !found && actual.as_ref() == body => {
                found = true;
            }
            Event::Text(text) if !display && text.as_ref() == "Equation: " => {}
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
            _ => return false,
        }
    }
    found
}
