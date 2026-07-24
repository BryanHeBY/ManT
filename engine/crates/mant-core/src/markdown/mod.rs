//! Parses a conservative Markdown subset into the shared document contract.
//!
//! Supported syntax becomes semantic AST nodes. Recognized extensions outside
//! the subset remain visible as exact source text with an attached diagnostic.

mod blocks;
mod inline;
mod layout;
mod options;
mod source;

#[cfg(test)]
mod tests;

use std::{collections::HashMap, ops::Range};

use mant_ast::{
    Block, Diagnostic, DiagnosticLevel, DocumentMeta, DocumentSchema, DocumentSource, Engine,
    Inline, MantDocument, Producer, Section, SectionRole, SourceFormat,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use self::{
    blocks::parse_block,
    inline::{inline_text, parse_inlines},
    layout::normalize_markdown_layout,
    options::normalize_option_lists,
    source::MarkdownSource,
};

type SpannedEvent<'a> = (Event<'a>, Range<usize>);

/// Parse one UTF-8 Markdown document without performing filesystem I/O.
#[must_use]
pub fn parse_markdown(source_text: &str, source_path: Option<String>) -> MantDocument {
    let source = MarkdownSource::new(source_text);
    let parser = Parser::new_ext(source_text, markdown_options());
    let mut cursor = EventCursor::new(parser.into_offset_iter().collect());
    let mut diagnostics = Vec::new();
    let mut root_blocks = Vec::new();
    let mut flat_sections = Vec::new();
    let mut ids = SectionIds::default();
    let mut title = None;

    while let Some((event, range)) = cursor.peek().cloned() {
        if let Event::Start(Tag::Heading {
            level,
            id: explicit_id,
            ..
        }) = event
        {
            let _ = cursor.next();
            let (children, end) = parse_inlines(
                &mut cursor,
                &source,
                &mut diagnostics,
                TagEnd::Heading(level),
            );
            let heading = inline_text(&children);
            if heading.is_empty() {
                diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Warning,
                    code: Some("markdown.empty-heading".to_owned()),
                    message: "ignored an empty Markdown heading".to_owned(),
                    source: Some(source.span(&(range.start..end))),
                });
                continue;
            }
            let is_document_title = title.is_none() && level == HeadingLevel::H1;
            if is_document_title {
                title = Some(heading.clone());
            }
            let id = ids.allocate(&heading, explicit_id.as_deref());
            flat_sections.push(FlatSection {
                level: heading_level(level),
                is_document_title,
                section: Section {
                    id,
                    title: heading.clone(),
                    role: quick_reference_role(&heading),
                    spacing_before_lines: u16::from(!flat_sections.is_empty()),
                    blocks: Vec::new(),
                    children: Vec::new(),
                    source: Some(source.span(&(range.start..end))),
                },
            });
            continue;
        }

        let Some(block) = parse_block(&mut cursor, &source, &mut diagnostics) else {
            continue;
        };
        if let Some(current) = flat_sections.last_mut() {
            current.section.blocks.push(block);
        } else {
            root_blocks.push(block);
        }
    }

    let mut sections = nest_sections(flat_sections);
    normalize_markdown_layout(&source, &mut root_blocks, &mut sections);
    normalize_option_lists(&mut root_blocks);
    normalize_section_options(&mut sections);
    let retained_targets = crate::definitions::identify_definitions(
        &mut sections,
        &ids.targets.keys().cloned().collect(),
    );
    for target in retained_targets {
        ids.targets.insert(target.clone(), target);
    }
    resolve_local_links(
        &mut root_blocks,
        &mut sections,
        &ids.targets,
        &mut diagnostics,
    );

    MantDocument {
        schema: DocumentSchema::V3,
        producer: markdown_producer(),
        source: DocumentSource {
            format: SourceFormat::Markdown,
            path: source_path,
            renderer: None,
        },
        meta: DocumentMeta {
            title,
            ..DocumentMeta::default()
        },
        diagnostics,
        blocks: root_blocks,
        sections,
    }
}

fn markdown_producer() -> Producer {
    Producer {
        name: "mant".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        engine: Some(Engine {
            name: "pulldown-cmark".to_owned(),
            version: "0.13".to_owned(),
        }),
    }
}

fn normalize_section_options(sections: &mut [Section]) {
    for section in sections {
        normalize_option_lists(&mut section.blocks);
        normalize_section_options(&mut section.children);
    }
}

fn markdown_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_WIKILINKS
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn quick_reference_role(title: &str) -> Option<SectionRole> {
    let normalized = title.trim();
    (normalized.eq_ignore_ascii_case("tldr")
        || normalized.eq_ignore_ascii_case("tldr quick reference")
        || normalized.eq_ignore_ascii_case("quick reference"))
    .then_some(SectionRole::QuickReference)
}

struct FlatSection {
    level: u8,
    is_document_title: bool,
    section: Section,
}

fn nest_sections(flat: Vec<FlatSection>) -> Vec<Section> {
    let mut roots = Vec::new();
    let mut stack: Vec<FlatSection> = Vec::new();

    for next in flat {
        while stack
            .last()
            .is_some_and(|current| current.is_document_title || current.level >= next.level)
        {
            attach_completed(&mut stack, &mut roots);
        }
        stack.push(next);
    }
    while !stack.is_empty() {
        attach_completed(&mut stack, &mut roots);
    }
    roots
}

fn attach_completed(stack: &mut Vec<FlatSection>, roots: &mut Vec<Section>) {
    let completed = stack.pop().expect("caller checks non-empty stack").section;
    if let Some(parent) = stack.last_mut() {
        parent.section.children.push(completed);
    } else {
        roots.push(completed);
    }
}

#[derive(Default)]
struct SectionIds {
    counts: HashMap<String, usize>,
    targets: HashMap<String, String>,
}

impl SectionIds {
    fn allocate(&mut self, title: &str, explicit: Option<&str>) -> String {
        let base = explicit
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map_or_else(|| slug(title), ToOwned::to_owned);
        let base = if base.is_empty() {
            "section".to_owned()
        } else {
            base
        };
        let count = self.counts.entry(base.clone()).or_default();
        *count += 1;
        let id = if *count == 1 {
            base.clone()
        } else {
            format!("{base}-{}", *count)
        };
        self.targets.insert(base, id.clone());
        self.targets.insert(slug(title), id.clone());
        self.targets.insert(id.clone(), id.clone());
        id
    }
}

fn slug(value: &str) -> String {
    let mut output = String::new();
    let mut separator = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || character == '_' {
            if separator && !output.is_empty() {
                output.push('-');
            }
            separator = false;
            output.push(character);
        } else {
            separator = true;
        }
    }
    output.trim_matches('-').to_owned()
}

fn resolve_local_links(
    root_blocks: &mut [Block],
    sections: &mut [Section],
    targets: &HashMap<String, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    resolve_blocks(root_blocks, targets, diagnostics);
    for section in sections {
        resolve_blocks(&mut section.blocks, targets, diagnostics);
        resolve_local_links(&mut [], &mut section.children, targets, diagnostics);
    }
}

fn resolve_blocks(
    blocks: &mut [Block],
    targets: &HashMap<String, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for block in blocks {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                resolve_inlines(children, targets, diagnostics);
            }
            Block::List { items, .. } => {
                for item in items {
                    resolve_blocks(&mut item.blocks, targets, diagnostics);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    for term in &mut item.terms {
                        resolve_inlines(term, targets, diagnostics);
                    }
                    resolve_blocks(&mut item.description, targets, diagnostics);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        resolve_blocks(&mut cell.blocks, targets, diagnostics);
                    }
                }
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn resolve_inlines(
    inlines: &mut [Inline],
    targets: &HashMap<String, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for inline in inlines {
        match inline {
            Inline::SectionReference { target, children } => {
                let lookup = target.trim().trim_start_matches('#');
                if let Some(id) = targets.get(lookup).or_else(|| targets.get(&slug(lookup))) {
                    *target = id.clone();
                } else {
                    diagnostics.push(Diagnostic {
                        level: DiagnosticLevel::Warning,
                        code: Some("markdown.unresolved-reference".to_owned()),
                        message: format!("unresolved Markdown document link '#{lookup}'"),
                        source: None,
                    });
                }
                resolve_inlines(children, targets, diagnostics);
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. } => {
                resolve_inlines(children, targets, diagnostics);
            }
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Anchor { .. }
            | Inline::LineBreak => {}
        }
    }
}

pub(super) struct EventCursor<'a> {
    events: Vec<SpannedEvent<'a>>,
    position: usize,
}

impl<'a> EventCursor<'a> {
    fn new(events: Vec<SpannedEvent<'a>>) -> Self {
        Self {
            events,
            position: 0,
        }
    }

    pub(super) fn peek(&self) -> Option<&SpannedEvent<'a>> {
        self.events.get(self.position)
    }

    pub(super) fn next(&mut self) -> Option<SpannedEvent<'a>> {
        let event = self.events.get(self.position)?.clone();
        self.position += 1;
        Some(event)
    }

    /// Consume the remainder of a just-opened tag, including nested tags.
    pub(super) fn consume_balanced(&mut self, start: Range<usize>) -> Range<usize> {
        let mut depth = 1usize;
        let mut end = start.end;
        while let Some((event, range)) = self.next() {
            end = range.end;
            match event {
                Event::Start(_) => depth = depth.saturating_add(1),
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        start.start..end
    }

    pub(super) fn subtree_contains_task_marker(&self) -> bool {
        let mut depth = 1usize;
        for (event, _) in &self.events[self.position..] {
            match event {
                Event::TaskListMarker(_) => return true,
                Event::Start(_) => depth = depth.saturating_add(1),
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return false;
                    }
                }
                _ => {}
            }
        }
        false
    }
}
