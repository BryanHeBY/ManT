//! One event-stream driver assigns original blocks to roots and flat headings.
#[cfg(test)]
mod tests;
use super::{
    blocks::parse_block,
    events::{EventCursor, SpannedEvent},
    headings::{FlatSection, SectionIds, heading_level, take_explicit_heading_id},
    inline::{inline_text, parse_inlines},
    source::MarkdownSource,
};
use mant_ir::{Block, Diagnostic, DiagnosticLevel, Heading, Section};
use pulldown_cmark::{Event, HeadingLevel, Tag, TagEnd};

pub(super) struct ParsedDocumentStructure {
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) root_blocks: Vec<Block>,
    pub(super) flat_sections: Vec<FlatSection>,
    pub(super) ids: SectionIds,
    pub(super) document_title_id: Option<String>,
}

/// Lower the Markdown event stream without imposing final document layout.
pub(super) fn lower_document_structure(
    events: Vec<SpannedEvent<'_>>,
    source: &MarkdownSource<'_>,
) -> ParsedDocumentStructure {
    let mut cursor = EventCursor::new(events);
    let mut diagnostics = Vec::new();
    let mut root_blocks = Vec::new();
    let mut flat_sections = Vec::new();
    let mut ids = SectionIds::default();
    let mut document_title_id = None;
    let mut saw_heading = false;

    while let Some((event, range)) = cursor.peek().cloned() {
        if let Event::Start(Tag::Heading {
            level,
            id: explicit_id,
            ..
        }) = event
        {
            let _ = cursor.next();
            let (mut children, end) = parse_inlines(
                &mut cursor,
                source,
                &mut diagnostics,
                TagEnd::Heading(level),
            );
            // `pulldown-cmark` treats every trailing brace group as heading
            // attributes and removes it before reporting whether it contains
            // a useful attribute.  ManT only consumes one explicit `#id`, so
            // recognize that narrow extension ourselves and leave ordinary
            // API paths such as `/users/{id}` in the title.
            let explicit_id = explicit_id
                .map(pulldown_cmark::CowStr::into_string)
                .or_else(|| take_explicit_heading_id(&mut children));
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
            let id = ids.allocate(&heading, explicit_id.as_deref());
            let fragment_aliases = explicit_id
                .as_deref()
                .map(mant_ir::FragmentAlias::from)
                .into_iter()
                .collect();
            if is_document_title {
                document_title_id = Some(id.clone());
            }
            flat_sections.push(FlatSection {
                level: heading_level(level),
                is_document_title,
                section: Section {
                    id: id.into(),
                    fragment_aliases,
                    heading: Heading {
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

    ParsedDocumentStructure {
        diagnostics,
        root_blocks,
        flat_sections,
        ids,
        document_title_id,
    }
}
