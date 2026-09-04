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

pub(super) fn styled_inline_lines(
    nodes: &[Inline],
    style: Style,
    current_address: Option<&DocumentAddress>,
) -> Vec<StyledInlineLine> {
    let mut lines = vec![StyledInlineLine::default()];
    append_inline(nodes, style, current_address, &mut lines);
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
    lines: &mut Vec<StyledInlineLine>,
) {
    for node in nodes {
        match node {
            Inline::Text { value } => append_text(value, style, lines),
            Inline::Strong { children } => {
                append_inline(
                    children,
                    style.fg(theme::STRONG).add_modifier(Modifier::BOLD),
                    current_address,
                    lines,
                );
            }
            Inline::Emphasis { children } => {
                append_inline(
                    children,
                    style.fg(theme::SUBTEXT).add_modifier(Modifier::ITALIC),
                    current_address,
                    lines,
                );
            }
            Inline::Code { value } => {
                append_text(value, Style::default().fg(theme::HEADING), lines);
            }
            Inline::Link {
                target, children, ..
            } => match target {
                mant_ir::LinkTarget::External { uri } => {
                    append_external_link(uri, children, current_address, lines);
                }
                mant_ir::LinkTarget::Email { address } => {
                    append_email_link(address, children, current_address, lines);
                }
                mant_ir::LinkTarget::Document { name, fragment } => {
                    let target = markdown_reference_address(current_address, name).map(|address| {
                        LinkTarget::Document {
                            address,
                            fragment: fragment.clone(),
                        }
                    });
                    append_addressable_inline(
                        children,
                        Style::default()
                            .fg(theme::LINK)
                            .add_modifier(Modifier::UNDERLINED),
                        current_address,
                        lines,
                        target.as_ref(),
                    );
                }
                mant_ir::LinkTarget::Manual {
                    name,
                    manual_section,
                } => {
                    let target =
                        manual_section
                            .as_ref()
                            .map(|manual_section| LinkTarget::Document {
                                address: DocumentAddress::Manual {
                                    name: name.clone(),
                                    manual_section: manual_section.clone(),
                                },
                                fragment: None,
                            });
                    append_addressable_inline(
                        children,
                        Style::default()
                            .fg(theme::LINK)
                            .add_modifier(Modifier::UNDERLINED),
                        current_address,
                        lines,
                        target.as_ref(),
                    );
                }
                mant_ir::LinkTarget::Section { id } => append_addressable_inline(
                    children,
                    Style::default()
                        .fg(theme::LINK)
                        .add_modifier(Modifier::UNDERLINED),
                    current_address,
                    lines,
                    Some(&LinkTarget::Section(id.to_string())),
                ),
            },
            Inline::Anchor { .. } => {}
            Inline::LineBreak => lines.push(StyledInlineLine::default()),
        }
    }
}

fn append_external_link(
    uri: &str,
    children: &[Inline],
    current_address: Option<&DocumentAddress>,
    lines: &mut Vec<StyledInlineLine>,
) {
    let target = ExternalUri::parse(uri).map(LinkTarget::External);
    append_addressable_inline(
        children,
        Style::default()
            .fg(theme::BLUE)
            .add_modifier(Modifier::UNDERLINED),
        current_address,
        lines,
        target.as_ref(),
    );
}

fn append_email_link(
    address: &str,
    children: &[Inline],
    current_address: Option<&DocumentAddress>,
    lines: &mut Vec<StyledInlineLine>,
) {
    let target = mant_ir::mailto_uri_for_email_address(address)
        .as_deref()
        .and_then(ExternalUri::parse)
        .map(LinkTarget::External);
    append_addressable_inline(
        children,
        Style::default()
            .fg(theme::BLUE)
            .add_modifier(Modifier::UNDERLINED),
        current_address,
        lines,
        target.as_ref(),
    );
}

fn append_addressable_inline(
    children: &[Inline],
    style: Style,
    current_address: Option<&DocumentAddress>,
    lines: &mut Vec<StyledInlineLine>,
    target: Option<&LinkTarget>,
) {
    let first_line = lines.len() - 1;
    let first_column = spans_width(&lines[first_line].spans);
    append_inline(children, style, current_address, lines);
    if let Some(target) = target {
        record_link(lines, first_line, first_column, target);
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
