//! Stateful roff font decoding shared by prose, macro operands and table cells.
use super::super::reference::trailing_sphinx_manual_reference;
use super::{
    Font, FontState, Inline, InlineBuilder, Node, NodeKind, RoffInlineEvent, append_inline_node,
    decode,
};
use std::borrow::Cow;

pub(in crate::mandoc) fn parse_roff_text(source: &str) -> Vec<Inline> {
    parse_roff_text_with_font(source, Font::Regular, true)
}

/// Decode one roff text run using the font selected by its enclosing macro.
/// Explicit `\\f` escapes change `font` while the run is scanned, so a reset
/// to regular text remains visible even inside an alternating `.BI` argument.
pub(super) fn parse_roff_text_with_font(
    source: &str,
    initial_font: Font,
    recognize_generated_references: bool,
) -> Vec<Inline> {
    let mut state = FontState::new();
    state.select(initial_font);
    parse_roff_text_with_state(source, &mut state, recognize_generated_references)
}

pub(super) fn lower_man_font_scope(
    node: &Node,
    default_name: Option<&str>,
    spacing: bool,
    state: &mut FontState,
) -> Vec<Inline> {
    state.select(Font::Regular);
    match node.macro_name.as_deref() {
        Some("B" | "SB") => state.select(Font::Strong),
        Some("I") => state.select(Font::Emphasis),
        _ => {}
    }
    let result = lower_inline_nodes_with_font_state(&node.children, default_name, spacing, state);
    state.select(Font::Regular);
    result
}

pub(super) fn lower_font_scope(
    nodes: &[Node],
    default_name: Option<&str>,
    spacing: bool,
    initial_font: Font,
) -> Vec<Inline> {
    let mut builder = InlineBuilder::with_spacing(spacing);
    builder.font.select(initial_font);
    for node in nodes {
        append_inline_node(&mut builder, node, default_name);
    }
    builder.finish()
}

pub(in crate::mandoc) fn lower_inline_nodes_with_font_state(
    nodes: &[Node],
    default_name: Option<&str>,
    spacing: bool,
    state: &mut FontState,
) -> Vec<Inline> {
    let mut builder = InlineBuilder::with_spacing(spacing);
    builder.font = *state;
    for node in nodes {
        append_inline_node(&mut builder, node, default_name);
    }
    *state = builder.font;
    builder.finish()
}

pub(super) fn parse_roff_text_with_state(
    source: &str,
    state: &mut FontState,
    recognize_generated_references: bool,
) -> Vec<Inline> {
    let mut output = Vec::new();
    let mut buffer = String::new();
    let mut font = state.current;
    let mut link: Option<String> = None;

    for event in decode(source) {
        match event {
            RoffInlineEvent::Text(value) => {
                buffer.push_str(&normalize_redundant_escaped_font(&value, font));
            }
            RoffInlineEvent::Font(next_font) => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                state.select(next_font);
                font = state.current;
            }
            RoffInlineEvent::PreviousFont => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                state.restore();
                font = state.current;
            }
            RoffInlineEvent::Link(target) => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                link = target;
            }
            RoffInlineEvent::EmptyDestination => {
                if !recognize_generated_references
                    || !promote_sphinx_manual_reference(
                        &mut output,
                        &mut buffer,
                        font,
                        link.as_deref(),
                    )
                {
                    buffer.push_str("<>");
                }
            }
            RoffInlineEvent::LineBreak => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                if !matches!(output.last(), Some(Inline::LineBreak)) {
                    output.push(Inline::LineBreak);
                }
            }
            RoffInlineEvent::Presentation { .. } => {}
        }
    }
    flush_segment(&mut output, &mut buffer, font, link.as_deref());
    output
}

/// Some generated manuals wrap a link label in a font and then escape another
/// copy of that same font request as visible text. libmandoc correctly reports
/// the enclosing font, so remove only the redundant escaped request. Keeping
/// this conditional on the enclosing font preserves literal `\\f` examples in
/// formatter manuals and ordinary prose.
fn normalize_redundant_escaped_font(source: &str, font: Font) -> Cow<'_, str> {
    let opening = match font {
        Font::Strong => r"\fB",
        Font::Emphasis => r"\fI",
        Font::StrongEmphasis => r"\f[BI]",
        Font::Code => r"\fC",
        Font::CodeStrong => r"\f[CB]",
        Font::CodeEmphasis => r"\f[CI]",
        Font::Regular => return Cow::Borrowed(source),
    };
    if !source.contains(opening) {
        return Cow::Borrowed(source);
    }

    Cow::Owned(source.replace(opening, "").replace(r"\fR", ""))
}

/// Lower a text node after honoring a macro-provided default font. Nodes marked
/// non-printing by libmandoc are never allowed to escape through this shortcut.
pub(super) fn lower_text_node(node: &Node, initial_font: Font) -> Vec<Inline> {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        Vec::new()
    } else {
        parse_roff_text_with_font(
            node.text.as_deref().unwrap_or_default(),
            initial_font,
            !node.flags.no_fill,
        )
    }
}

fn promote_sphinx_manual_reference(
    output: &mut Vec<Inline>,
    buffer: &mut String,
    font: Font,
    external_link: Option<&str>,
) -> bool {
    if external_link.is_some() || matches!(font, Font::Code | Font::CodeStrong | Font::CodeEmphasis)
    {
        return false;
    }
    let Some(reference) = trailing_sphinx_manual_reference(buffer) else {
        return false;
    };
    let prefix = reference.prefix.to_owned();
    let display = reference.display.to_owned();
    let name = reference.name.to_owned();
    let manual_section = reference.manual_section.to_owned();
    *buffer = prefix;
    flush_segment(output, buffer, font, None);
    output.push(Inline::Link {
        target: mant_ir::LinkTarget::Manual {
            name,
            manual_section: Some(manual_section),
        },
        title: None,
        children: vec![styled_segment(display, font)],
    });
    true
}

fn flush_segment(output: &mut Vec<Inline>, buffer: &mut String, font: Font, link: Option<&str>) {
    if buffer.is_empty() {
        return;
    }
    let value = std::mem::take(buffer);
    let styled = styled_segment(value, font);
    if let Some(target) = link {
        output.push(Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: target.to_owned(),
            },
            title: None,
            children: vec![styled],
        });
    } else {
        output.push(styled);
    }
}

fn styled_segment(value: String, font: Font) -> Inline {
    match font {
        Font::Regular => Inline::Text { value },
        Font::Strong => Inline::Strong {
            children: vec![Inline::Text { value }],
        },
        Font::Emphasis => Inline::Emphasis {
            children: vec![Inline::Text { value }],
        },
        Font::StrongEmphasis => Inline::Strong {
            children: vec![Inline::Emphasis {
                children: vec![Inline::Text { value }],
            }],
        },
        Font::Code => Inline::Code { value },
        Font::CodeStrong => Inline::Strong {
            children: vec![Inline::Code { value }],
        },
        Font::CodeEmphasis => Inline::Emphasis {
            children: vec![Inline::Code { value }],
        },
    }
}
