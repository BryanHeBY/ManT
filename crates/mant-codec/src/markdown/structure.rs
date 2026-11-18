//! One event-stream driver assigns original blocks to roots and flat headings.
#[cfg(test)]
mod tests;
use super::{
    blocks::parse_block,
    events::{EventCursor, SpannedEvent},
    headings::{FlatSection, assign_section_ids, heading_level, take_explicit_heading_id},
    inline::{inline_text, parse_inlines},
    source::MarkdownSource,
};
use mant_ir::{Block, Diagnostic, DiagnosticLevel, Heading, Section};
use pulldown_cmark::{Event, HeadingLevel, Tag, TagEnd};

pub(super) struct ParsedDocumentStructure {
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) root_blocks: Vec<Block>,
    pub(super) flat_sections: Vec<FlatSection>,
    pub(super) document_title_id: Option<String>,
}

/// Lower the Markdown event stream without imposing final document layout.
pub(super) fn lower_document_structure(
    events: Vec<SpannedEvent<'_>>,
    metadata_origins: super::events::MetadataOrigins,
    source: &MarkdownSource<'_>,
) -> ParsedDocumentStructure {
    let mut cursor = EventCursor::new(events);
    cursor.metadata_origins = metadata_origins;
    let mut diagnostics = Vec::new();
    let mut root_blocks = Vec::new();
    let mut flat_sections = Vec::new();
    let mut saw_heading = false;

    while let Some((event, range)) = cursor.peek().cloned() {
        if let Event::Start(Tag::Heading { level, .. }) = event {
            let _ = cursor.next();
            let explicit_id = take_explicit_heading_id(&mut cursor, source, range.start);
            let (children, end) = parse_inlines(
                &mut cursor,
                source,
                &mut diagnostics,
                TagEnd::Heading(level),
            );
            let heading = inline_text(&children);
            if heading.is_empty() {
                diagnostics.push(Diagnostic {
                    impact: mant_ir::DiagnosticImpact::None,
                    level: DiagnosticLevel::Warning,
                    code: Some("markdown.empty-heading".to_owned()),
                    message: "preserved a Markdown heading without visible text".to_owned(),
                    source: Some(source.span(&(range.start..end))),
                });
            }
            let is_document_title = !saw_heading && level == HeadingLevel::H1;
            saw_heading = true;
            flat_sections.push(FlatSection {
                level: heading_level(level),
                is_document_title,
                section: Section {
                    id: "".into(),
                    fragment_aliases: explicit_id.into_iter().map(Into::into).collect(),
                    heading: Heading {
                        inline_layout: mant_ir::InlineLayout::default(),
                        content: children,
                        source: Some(source.span(&(range.start..end))),
                    },
                    spacing_before_lines: u16::from(!flat_sections.is_empty()),
                    blocks: Vec::new(),
                    children: Vec::new(),
                    source: Some(source.span(&(range.start..end))),
                },
            });
            continue;
        }

        let Some(block) = parse_block(&mut cursor, source, &mut diagnostics) else {
            continue;
        };
        if let Some(current) = flat_sections.last_mut() {
            current.section.blocks.push(block);
        } else {
            root_blocks.push(block);
        }
    }

    let document_title_id = assign_section_ids(&mut flat_sections);
    ParsedDocumentStructure {
        diagnostics,
        root_blocks,
        flat_sections,
        document_title_id,
    }
}
