//! Lowers semantic inline nodes into styled text, anchors, and link targets.

use super::model::GlyphProjection;
use super::{
    DocumentAddress, ExternalUri, HashMap, Inline, LinkTarget, LogicalLinkRange, Modifier, Section,
    Span, Style, StyledInlineLine, theme,
};
use mant_ir::{ContentContext, InlineView, LinkOccurrenceKey};

pub(super) fn tldr_style(role: mant_render::TldrRole) -> Style {
    use mant_render::TldrRole;

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

/// Anchor ownership follows original hard lines, independently of styling or
/// visual wrapping. Carry the row across nested wrappers instead of flattening
/// targets into an unordered set at the start of the whole paragraph.
pub(super) fn inline_anchor_rows<'a>(
    content: ContentContext<'a>,
    nodes: &'a [Inline],
) -> Vec<(String, usize)> {
    let mut ids = Vec::new();
    collect_anchor_rows(content, nodes, &mut 0, &mut ids)
        .expect("validated document content must resolve while locating anchors");
    ids
}

fn collect_anchor_rows<'a>(
    content: ContentContext<'a>,
    nodes: &'a [Inline],
    row: &mut usize,
    ids: &mut Vec<(String, usize)>,
) -> Result<(), mant_ir::ContentReadError> {
    for node in nodes {
        match content.inline(node)? {
            InlineView::Anchor(anchor) => {
                ids.push((anchor.id().to_string(), *row));
                ids.extend(
                    anchor
                        .fragment_aliases()
                        .iter()
                        .map(|alias| (alias.to_string(), *row)),
                );
            }
            InlineView::Strong(children) | InlineView::Emphasis(children) => {
                collect_anchor_rows(content, children, row, ids)?;
            }
            InlineView::Link(link) => collect_anchor_rows(content, link.children(), row, ids)?,
            InlineView::Text(value) | InlineView::Code(value) => {
                *row += value.matches('\n').count();
            }
            InlineView::LineBreak => *row += 1,
            _ => return Err(mant_ir::ContentReadError),
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn styled_inline_lines(
    nodes: &[Inline],
    style: Style,
    current_address: Option<&DocumentAddress>,
) -> Vec<StyledInlineLine> {
    styled_bound_inline_lines(
        crate::test_content::content(),
        nodes,
        style,
        current_address,
        &[],
    )
}

#[cfg(test)]
pub(super) fn styled_bound_inline_lines<'a>(
    content: ContentContext<'a>,
    nodes: &'a [Inline],
    style: Style,
    current_address: Option<&DocumentAddress>,
    names: &[mant_render::InlineNameRange],
) -> Vec<StyledInlineLine> {
    styled_display_inline_lines(content, nodes, style, current_address, names, false)
}

#[cfg(test)]
pub(super) fn styled_display_inline_lines<'a>(
    content: ContentContext<'a>,
    nodes: &'a [Inline],
    style: Style,
    current_address: Option<&DocumentAddress>,
    names: &[mant_render::InlineNameRange],
    code: bool,
) -> Vec<StyledInlineLine> {
    styled_reference_inline_lines(
        content,
        nodes,
        style,
        current_address,
        names,
        code,
        &std::collections::HashMap::default(),
        &mut HashMap::new(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn styled_reference_inline_lines<'a>(
    content: ContentContext<'a>,
    nodes: &'a [Inline],
    style: Style,
    current_address: Option<&DocumentAddress>,
    names: &[mant_render::InlineNameRange],
    code: bool,
    origins: &super::references::ReferenceOrigins,
    link_targets: &mut HashMap<super::LinkIdentity, LinkTarget>,
) -> Vec<StyledInlineLine> {
    let mut lines = vec![StyledInlineLine::default()];
    append_inline(
        content,
        nodes,
        style,
        current_address,
        names,
        code,
        link_targets,
        &mut lines,
    );
    reference_marks(content, nodes, origins, &mut lines, &mut 0, &mut 0)
        .expect("validated document content must resolve while marking references and anchors");
    lines
}

fn reference_marks<'a>(
    content: ContentContext<'a>,
    nodes: &'a [Inline],
    origins: &super::references::ReferenceOrigins,
    lines: &mut [StyledInlineLine],
    row: &mut usize,
    column: &mut usize,
) -> Result<(), mant_ir::ContentReadError> {
    for node in nodes {
        match content.inline(node)? {
            InlineView::Link(link) => {
                if let Some(id) = origins.get(&link.occurrence())
                    && let Some(line) = lines.get_mut(*row)
                {
                    line.reference_marks.push(super::model::ReferenceMark {
                        id: std::sync::Arc::clone(id),
                        scalar_offset: *column,
                    });
                }
                reference_marks(content, link.children(), origins, lines, row, column)?;
            }
            InlineView::Strong(children) | InlineView::Emphasis(children) => {
                reference_marks(content, children, origins, lines, row, column)?;
            }
            InlineView::LineBreak => {
                *row += 1;
                *column = 0;
            }
            InlineView::Text(value) | InlineView::Code(value) => {
                for (index, part) in value.split('\n').enumerate() {
                    if index > 0 {
                        *row += 1;
                        *column = 0;
                    }
                    *column += part.chars().count();
                }
            }
            InlineView::Anchor(anchor) => {
                if let Some(line) = lines.get_mut(*row) {
                    line.reference_marks.push(super::model::ReferenceMark {
                        id: std::sync::Arc::from(anchor.id().as_str()),
                        scalar_offset: *column,
                    });
                    for alias in anchor.fragment_aliases() {
                        line.reference_marks.push(super::model::ReferenceMark {
                            id: std::sync::Arc::from(alias.as_str()),
                            scalar_offset: *column,
                        });
                    }
                }
            }
            _ => return Err(mant_ir::ContentReadError),
        }
    }
    Ok(())
}

pub(super) fn shifted_reference_marks(
    mut marks: Vec<super::model::ReferenceMark>,
    scalars: usize,
) -> Vec<super::model::ReferenceMark> {
    for mark in &mut marks {
        mark.scalar_offset += scalars;
    }
    marks
}

pub(super) fn spans_width(spans: &[Span<'_>]) -> usize {
    if let [span] = spans {
        return mant_render::cells::graphemes(&span.content)
            .map(|g| g.columns())
            .sum();
    }
    let text = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    mant_render::cells::graphemes(&text)
        .map(|g| g.columns())
        .sum()
}

pub(super) fn shifted_links(links: Vec<LogicalLinkRange>, scalars: usize) -> Vec<LogicalLinkRange> {
    links
        .into_iter()
        .map(|mut link| {
            link.start_scalar += scalars;
            link.end_scalar += scalars;
            link
        })
        .collect()
}

pub(super) fn shifted_glyph_projections(
    mut projections: Vec<GlyphProjection>,
    scalars: usize,
) -> Vec<GlyphProjection> {
    for projection in &mut projections {
        projection.scalar += scalars;
    }
    projections
}

pub(super) fn projected_spans_width(spans: &[Span<'_>], projections: &[GlyphProjection]) -> usize {
    let logical = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let mut display = String::with_capacity(logical.len());
    let mut projections = projections.iter().peekable();
    for (scalar, character) in logical.chars().enumerate() {
        while projections
            .peek()
            .is_some_and(|projection| projection.scalar < scalar)
        {
            projections.next();
        }
        if projections
            .peek()
            .is_some_and(|projection| projection.scalar == scalar)
        {
            let projection = projections.next().expect("peeked projection");
            display.push_str(&projection.glyphs);
        } else {
            display.push(character);
        }
    }
    mant_render::cells::graphemes(&display)
        .map(|grapheme| grapheme.columns())
        .sum()
}

pub(super) fn count_sections(sections: &[Section]) -> usize {
    sections
        .iter()
        .map(|section| 1 + count_sections(&section.children))
        .sum()
}

#[allow(clippy::too_many_arguments)]
fn append_inline<'a>(
    content: ContentContext<'a>,
    nodes: &'a [Inline],
    style: Style,
    current_address: Option<&DocumentAddress>,
    names: &[mant_render::InlineNameRange],
    code: bool,
    link_targets: &mut HashMap<super::LinkIdentity, LinkTarget>,
    lines: &mut Vec<StyledInlineLine>,
) {
    mant_render::visit_inline_display_text(content, nodes, names, |source, link, text, display| {
        let first_line = lines.len() - 1;
        let first_scalar = spans_scalars(&lines[first_line].spans);
        if code {
            // Lexical code accents are weaker than authored markup and names.
            for span in crate::code::highlight(vec![Span::styled(text.to_owned(), style)]) {
                append_text(
                    &span.content,
                    source_style(span.style, source, link.map(|link| link.target)),
                    lines,
                );
            }
        } else {
            append_text(
                text,
                source_style(style, source, link.map(|link| link.target)),
                lines,
            );
        }
        if let Some(display) = display
            && display != text
            && text.chars().count() == 1
            && !text.contains('\n')
        {
            // Native projected atoms are one logical scalar. The original
            // span stays untouched for matching, copy, and reference offsets.
            lines[first_line].glyph_projections.push(GlyphProjection {
                scalar: first_scalar,
                glyphs: display.to_owned(),
            });
        }
        if let Some((link, target)) = link.and_then(|link| {
            local_link_target(link.target, current_address).map(|target| (link, target))
        }) {
            link_targets
                .entry(super::LinkIdentity::Content(link.occurrence))
                .or_insert(target);
            record_link(lines, first_line, first_scalar, link.occurrence);
        }
    })
    .expect("validated document content must resolve while lowering inline text");
}

/// Layer source markup, then the more specific validated semantic name color.
/// No layer discards inherited modifiers. Link affordance survives Code.
fn source_style(
    mut style: Style,
    source: mant_render::InlinePresentation,
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

pub(super) fn local_link_target(
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
        } => Some(manual_section.as_ref().map_or_else(
            || LinkTarget::Manual {
                name: name.clone(),
                manual_section: None,
            },
            |manual_section| LinkTarget::Document {
                address: DocumentAddress::Manual {
                    name: name.clone(),
                    manual_section: manual_section.clone(),
                },
                fragment: None,
            },
        )),
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
    first_scalar: usize,
    occurrence: LinkOccurrenceKey,
) {
    let last_line = lines.len() - 1;
    for (line_index, line) in lines
        .iter_mut()
        .enumerate()
        .take(last_line + 1)
        .skip(first_line)
    {
        let start_scalar = if line_index == first_line {
            first_scalar
        } else {
            0
        };
        let end_scalar = spans_scalars(&line.spans);
        if end_scalar > start_scalar {
            line.links.push(LogicalLinkRange {
                identity: super::LinkIdentity::Content(occurrence),
                start_scalar,
                end_scalar,
            });
        }
    }
}

pub(super) fn spans_scalars(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
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
