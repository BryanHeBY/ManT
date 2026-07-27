//! Parses a conservative Markdown subset into the shared document contract.
//!
//! Supported syntax becomes semantic AST nodes. Recognized extensions outside
//! the subset remain visible as exact source text with an attached diagnostic.

mod blocks;
mod container;
mod inline;
mod layout;
mod options;
mod source;

#[cfg(test)]
mod tests;

pub use container::TldrDirectiveError;

use std::{collections::HashMap, error::Error, fmt, ops::Range};

use mant_ast::{
    Block, Diagnostic, DiagnosticLevel, DocumentMeta, DocumentSchema, DocumentSource, Engine,
    Inline, MantDocument, Producer, Section, SourceFormat, TldrDocument, TldrOrigin,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use self::{
    blocks::parse_block,
    container::split_markdown,
    inline::{inline_text, parse_inlines},
    layout::normalize_markdown_layout,
    options::normalize_option_lists,
    source::MarkdownSource,
};
use crate::{
    projection::DOCUMENT_ROOT_ID,
    tldr::{TldrPageLocation, TldrParseError, parse_tldr_page},
};

type SpannedEvent<'a> = (Event<'a>, Range<usize>);

/// Complete result of parsing one ManT-flavoured Markdown input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMarkdown {
    pub document: MantDocument,
    pub tldr: Option<TldrDocument>,
}

/// Invalid structure in `ManT`'s optional top-level Markdown extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownParseError {
    TldrDirective(TldrDirectiveError),
    TldrPage(TldrParseError),
}

impl fmt::Display for MarkdownParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TldrDirective(error) => error.fmt(formatter),
            Self::TldrPage(error) => write!(formatter, "invalid embedded tldr page: {error}"),
        }
    }
}

impl Error for MarkdownParseError {}

/// Split `ManT`'s optional leading tldr directive from the Markdown document.
///
/// The directive must be the first non-empty construct and uses the existing
/// tldr-pages Markdown dialect. The remaining source is parsed independently,
/// so its first H1 remains document metadata rather than part of the preface.
///
/// # Errors
///
/// Returns [`MarkdownParseError`] for an unterminated directive or malformed
/// embedded tldr page.
pub fn parse_markdown(
    source_text: &str,
    source_path: Option<String>,
) -> Result<ParsedMarkdown, MarkdownParseError> {
    let mut sanitize_diagnostics = Vec::new();
    let sanitized = sanitize_source(source_text, &mut sanitize_diagnostics);
    let source_text = sanitized.as_deref().unwrap_or(source_text);
    let parts = split_markdown(source_text).map_err(MarkdownParseError::TldrDirective)?;
    let tldr = parts
        .tldr
        .map(|source| {
            parse_tldr_page(
                source,
                TldrPageLocation {
                    platform: "embedded".to_owned(),
                    language: "und".to_owned(),
                    source_path: source_path.clone().unwrap_or_else(|| "<stdin>".to_owned()),
                },
            )
            .map(|mut page| {
                page.origin = TldrOrigin::Embedded;
                page
            })
            .map_err(MarkdownParseError::TldrPage)
        })
        .transpose()?;
    let mut document = parse_document(parts.document.as_ref(), source_path);
    if !sanitize_diagnostics.is_empty() {
        sanitize_diagnostics.extend(std::mem::take(&mut document.diagnostics));
        document.diagnostics = sanitize_diagnostics;
    }
    Ok(ParsedMarkdown { document, tldr })
}

/// Mask a leading BOM and terminal-unsafe control characters with spaces.
///
/// A BOM would hide the tldr directive and demote the first heading, while
/// raw control characters would pass escape sequences through to terminals.
/// Replacements keep every byte offset valid for source coordinates.
fn sanitize_source(source_text: &str, diagnostics: &mut Vec<Diagnostic>) -> Option<String> {
    let keeps_character =
        |character: char| !character.is_control() || matches!(character, '\t' | '\n' | '\r');
    let bom = source_text.starts_with('\u{feff}');
    if !bom && source_text.chars().all(keeps_character) {
        return None;
    }

    let mut sanitized = String::with_capacity(source_text.len());
    let mut controls = 0usize;
    let rest = if bom {
        sanitized.push_str("   ");
        &source_text['\u{feff}'.len_utf8()..]
    } else {
        source_text
    };
    for character in rest.chars() {
        if keeps_character(character) {
            sanitized.push(character);
        } else {
            controls += 1;
            sanitized.extend(std::iter::repeat_n(' ', character.len_utf8()));
        }
    }

    if bom {
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some("markdown.byte-order-mark".to_owned()),
            message: "masked a leading byte-order mark".to_owned(),
            source: None,
        });
    }
    if controls > 0 {
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some("markdown.control-characters".to_owned()),
            message: format!("masked {controls} terminal-unsafe control character(s)"),
            source: None,
        });
    }
    Some(sanitized)
}

/// Lower the ordinary document portion after extension extraction.
fn parse_document(source_text: &str, source_path: Option<String>) -> MantDocument {
    let source = MarkdownSource::new(source_text);
    let ParsedDocumentStructure {
        mut diagnostics,
        mut root_blocks,
        flat_sections,
        mut ids,
        title,
        document_title_id,
    } = lower_document_structure(source_text, &source);
    let mut sections = nest_sections(flat_sections);
    let extracted_title = extract_document_title(
        &mut root_blocks,
        &mut sections,
        document_title_id.as_deref(),
    );
    if extracted_title {
        let replacement = if root_blocks.is_empty() {
            sections.first().map(|section| section.id.as_str())
        } else {
            Some(DOCUMENT_ROOT_ID)
        };
        ids.remap_target(document_title_id.as_deref(), replacement);
    }
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

struct ParsedDocumentStructure {
    diagnostics: Vec<Diagnostic>,
    root_blocks: Vec<Block>,
    flat_sections: Vec<FlatSection>,
    ids: SectionIds,
    title: Option<String>,
    document_title_id: Option<String>,
}

/// Lower the Markdown event stream without imposing final document layout.
fn lower_document_structure(
    source_text: &str,
    source: &MarkdownSource<'_>,
) -> ParsedDocumentStructure {
    let parser = Parser::new_ext(source_text, markdown_options());
    let mut cursor = EventCursor::new(parser.into_offset_iter().collect());
    let mut diagnostics = Vec::new();
    let mut root_blocks = Vec::new();
    let mut flat_sections = Vec::new();
    let mut ids = SectionIds::default();
    let mut title = None;
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
            let (children, end) = parse_inlines(
                &mut cursor,
                source,
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
            let is_document_title = !saw_heading && level == HeadingLevel::H1;
            saw_heading = true;
            if is_document_title {
                title = Some(heading.clone());
            }
            let id = ids.allocate(&heading, explicit_id.as_deref());
            if is_document_title {
                document_title_id = Some(id.clone());
            }
            flat_sections.push(FlatSection {
                level: heading_level(level),
                is_document_title,
                section: Section {
                    id,
                    title: heading.clone(),
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
        title,
        document_title_id,
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

/// A leading H1 names the document; it is metadata rather than manual content.
fn extract_document_title(
    root_blocks: &mut Vec<Block>,
    sections: &mut Vec<Section>,
    document_title_id: Option<&str>,
) -> bool {
    let Some(document_title_id) = document_title_id else {
        return false;
    };
    if sections.first().map(|section| section.id.as_str()) != Some(document_title_id) {
        return false;
    }
    let title = sections.remove(0);
    root_blocks.extend(title.blocks);
    sections.splice(0..0, title.children);
    true
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
        } else if is_reserved_id(&base) {
            // Reserved selectors and bare tree paths would shadow this
            // heading in excerpt selection; keep it addressable instead.
            format!("{base}-section")
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
        // Ambiguous human-facing keys resolve to the first section that
        // claimed them, matching the bare slug this heading renders as its
        // anchor. A later duplicate owns only its own disambiguated id.
        self.targets.entry(base).or_insert_with(|| id.clone());
        self.targets
            .entry(slug(title))
            .or_insert_with(|| id.clone());
        self.targets.insert(id.clone(), id.clone());
        id
    }

    fn remap_target(&mut self, current: Option<&str>, replacement: Option<&str>) {
        let Some(current) = current else {
            return;
        };
        if let Some(replacement) = replacement {
            for target in self.targets.values_mut() {
                if target == current {
                    replacement.clone_into(target);
                }
            }
        } else {
            self.targets.retain(|_, target| target != current);
        }
    }
}

/// IDs that excerpt selection interprets before document section IDs.
fn is_reserved_id(base: &str) -> bool {
    base == crate::projection::TLDR_ID
        || base == crate::projection::DOCUMENT_ROOT_PATH
        || base == crate::projection::DOCUMENT_ROOT_ID
        || base.bytes().all(|byte| byte.is_ascii_digit())
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
    depth: usize,
}

/// Recursion budget shared by nested block containers and inline spans.
///
/// Parsing recurses once per nesting level, so unbounded input depth would
/// overflow the stack before any allocation limit applies. Subtrees beyond
/// this depth are preserved as unsupported source text with a diagnostic.
const MAX_NESTING_DEPTH: usize = 64;

impl<'a> EventCursor<'a> {
    fn new(events: Vec<SpannedEvent<'a>>) -> Self {
        Self {
            events,
            position: 0,
            depth: 0,
        }
    }

    /// Reserve one nesting level; callers must pair with [`Self::ascend`].
    pub(super) fn try_descend(&mut self) -> bool {
        if self.depth >= MAX_NESTING_DEPTH {
            return false;
        }
        self.depth += 1;
        true
    }

    pub(super) fn ascend(&mut self) {
        self.depth = self.depth.saturating_sub(1);
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
