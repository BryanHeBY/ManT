//! Lowers semantic inline nodes into styled text, anchors, and link targets.

use super::{
    DocumentAddress, ExternalUri, Inline, LinkTarget, LogicalLinkRange, Modifier, Section, Span,
    Style, StyledInlineLine, UnicodeWidthStr, theme,
};

pub(super) fn tldr_style(role: crate::tldr::TldrRole) -> Style {
    use crate::tldr::TldrRole;

    match role {
        TldrRole::Title => Style::default()
            .fg(theme::MAUVE)
            .add_modifier(Modifier::BOLD),
        TldrRole::Body | TldrRole::Placeholder => Style::default().fg(theme::TEXT),
        TldrRole::Example => Style::default().fg(theme::GREEN),
        TldrRole::Command => Style::default().fg(theme::PEACH),
        TldrRole::Link => Style::default()
            .fg(theme::BLUE)
            .add_modifier(Modifier::UNDERLINED),
        TldrRole::Attribution => Style::default().fg(theme::SUBTEXT),
    }
}

pub(super) fn inline_anchor_ids(nodes: &[Inline]) -> Vec<String> {
    let mut ids = Vec::new();
    for node in nodes {
        match node {
            Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } => {
                ids.push(id.to_string());
                ids.extend(fragment_aliases.iter().map(ToString::to_string));
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => ids.extend(inline_anchor_ids(children)),
            Inline::Text { .. } | Inline::Code { .. } | Inline::LineBreak => {}
        }
    }
    ids
}

#[cfg(test)]
pub(super) fn styled_inline_lines(
    nodes: &[Inline],
    style: Style,
    current_address: Option<&DocumentAddress>,
) -> Vec<StyledInlineLine> {
    styled_bound_inline_lines(nodes, style, current_address, &[])
}

pub(super) fn styled_bound_inline_lines(
    nodes: &[Inline],
    style: Style,
    current_address: Option<&DocumentAddress>,
    names: &[mant_protocol::InlineNameRange],
) -> Vec<StyledInlineLine> {
    styled_display_inline_lines(nodes, style, current_address, names, false)
}

pub(super) fn styled_display_inline_lines(
    nodes: &[Inline],
    style: Style,
    current_address: Option<&DocumentAddress>,
    names: &[mant_protocol::InlineNameRange],
    code: bool,
) -> Vec<StyledInlineLine> {
    let mut lines = vec![StyledInlineLine::default()];
    append_inline(nodes, style, current_address, names, code, &mut lines);
    lines
}

pub(super) fn spans_width(spans: &[Span<'_>]) -> usize {
    spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

pub(super) fn shifted_links(links: Vec<LogicalLinkRange>, columns: usize) -> Vec<LogicalLinkRange> {
    links
        .into_iter()
        .map(|mut link| {
            link.start_column += columns;
            link.end_column += columns;
            link
        })
        .collect()
}

pub(super) fn count_sections(sections: &[Section]) -> usize {
    sections
        .iter()
        .map(|section| 1 + count_sections(&section.children))
        .sum()
}

fn append_inline(
    nodes: &[Inline],
    style: Style,
    current_address: Option<&DocumentAddress>,
    names: &[mant_protocol::InlineNameRange],
    code: bool,
    lines: &mut Vec<StyledInlineLine>,
) {
    mant_protocol::visit_inline_text(nodes, names, |source, target, text| {
        let first_line = lines.len() - 1;
        let first_column = spans_width(&lines[first_line].spans);
        if code {
            // Lexical code accents are weaker than authored markup and names.
            for span in crate::code::highlight(vec![Span::styled(text.to_owned(), style)]) {
                append_text(
                    &span.content,
                    source_style(span.style, source, target),
                    lines,
                );
            }
        } else {
            append_text(text, source_style(style, source, target), lines);
        }
        if let Some(target) = target.and_then(|target| local_link_target(target, current_address)) {
            record_link(lines, first_line, first_column, &target);
        }
    });
}

/// Layer source markup, then the more specific validated semantic name color.
/// No layer discards inherited modifiers. Link affordance survives Code.
fn source_style(
    mut style: Style,
    source: mant_protocol::InlinePresentation,
    target: Option<&mant_ir::LinkTarget>,
) -> Style {
    if source.strong {
        style = style.fg(theme::STRONG).add_modifier(Modifier::BOLD);
    }
    if source.emphasis {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if source.code {
        style = style.fg(theme::HEADING);
    }
    if source.link {
        let color = match target {
            Some(mant_ir::LinkTarget::External { .. } | mant_ir::LinkTarget::Email { .. }) => {
                theme::BLUE
            }
            _ => theme::LINK,
        };
        style = style.fg(color).add_modifier(Modifier::UNDERLINED);
    }
    if let Some(kind) = source.entry_kind {
        style = style.fg(theme::entry_color(kind));
    }
    style
}

fn local_link_target(
    target: &mant_ir::LinkTarget,
    current: Option<&DocumentAddress>,
) -> Option<LinkTarget> {
    match target {
        mant_ir::LinkTarget::External { uri } => ExternalUri::parse(uri).map(LinkTarget::External),
        mant_ir::LinkTarget::Email { address } => mant_ir::mailto_uri_for_email_address(address)
            .as_deref()
            .and_then(ExternalUri::parse)
            .map(LinkTarget::External),
        mant_ir::LinkTarget::Document { name, fragment } => {
            markdown_reference_address(current, name).map(|address| LinkTarget::Document {
                address,
                fragment: fragment.clone(),
            })
        }
        mant_ir::LinkTarget::Manual {
            name,
            manual_section,
        } => manual_section
            .as_ref()
            .map(|manual_section| LinkTarget::Document {
                address: DocumentAddress::Manual {
                    name: name.clone(),
                    manual_section: manual_section.clone(),
                },
                fragment: None,
            }),
        mant_ir::LinkTarget::Section { id } => Some(LinkTarget::Section(id.to_string())),
    }
}

fn markdown_reference_address(
    current: Option<&DocumentAddress>,
    name: &str,
) -> Option<DocumentAddress> {
    current?.resolve_document_reference(name)
}

fn record_link(
    lines: &mut [StyledInlineLine],
    first_line: usize,
    first_column: usize,
    target: &LinkTarget,
) {
    let last_line = lines.len() - 1;
    for (line_index, line) in lines
        .iter_mut()
        .enumerate()
        .take(last_line + 1)
        .skip(first_line)
    {
        let start_column = if line_index == first_line {
            first_column
        } else {
            0
        };
        let end_column = spans_width(&line.spans);
        if end_column > start_column {
            line.links.push(LogicalLinkRange {
                target: target.clone(),
                start_column,
                end_column,
            });
        }
    }
}

fn append_text(value: &str, style: Style, lines: &mut Vec<StyledInlineLine>) {
    for (index, part) in value.split('\n').enumerate() {
        if index > 0 {
            lines.push(StyledInlineLine::default());
        }
        if !part.is_empty() {
            lines
                .last_mut()
                .expect("inline builder always owns one line")
                .spans
                .push(Span::styled(part.to_owned(), style));
        }
    }
}
