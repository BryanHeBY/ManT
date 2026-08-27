//! Parses a conservative Markdown subset into the shared document contract.
//!
//! Supported syntax becomes semantic IR nodes. Recognized extensions outside
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
pub(crate) use options::is_semantic_entry_rejection_code;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    error::Error,
    fmt,
    ops::Range,
};

use mant_ir::{
    Block, Diagnostic, DiagnosticLevel, Document, DocumentMeta, DocumentSource, Inline, ParserInfo,
    Section, SourceFormat, TldrDocument, TldrOrigin, validate_document,
    visit::{self, VisitMut},
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use self::{
    blocks::parse_block,
    container::split_markdown,
    inline::{inline_text, parse_inlines},
    layout::normalize_markdown_layout,
    options::{extract_entry_directives, normalize_entry_lists},
    source::MarkdownSource,
};
use crate::text_safety::mask_terminal_controls;
use crate::{
    projection::DOCUMENT_ROOT_ID,
    tldr::{TldrPageLocation, TldrParseError, parse_tldr_page},
};

type SpannedEvent<'a> = (Event<'a>, Range<usize>);

/// Complete result of parsing one `ManT`-flavoured Markdown input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMarkdown {
    /// Authoritative normalized Markdown document.
    pub document: Document,
    /// Optional document-owned quick reference.
    pub tldr: Option<TldrDocument>,
}

/// Invalid structure in `ManT`'s optional top-level Markdown extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownParseError {
    /// The top-level `ManT` tldr container is malformed.
    TldrDirective(TldrDirectiveError),
    /// Embedded tldr Markdown is structurally invalid.
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

/// Split `ManT`'s optional leading tldr preface from the Markdown document.
///
/// Invisible HTML comments delimit the preface so `CommonMark` renderers can
/// present the enclosed tldr-pages Markdown without leaking extension syntax.
/// It must be the first non-empty construct. The remaining source is parsed
/// independently, so its first H1 remains document metadata rather than part
/// of the preface.
///
/// # Errors
///
/// Returns [`MarkdownParseError`] for an unterminated preface or malformed
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
    let mut entry_diagnostics = Vec::new();
    let (masked_document, declarations) =
        extract_entry_directives(parts.document.as_ref(), &mut entry_diagnostics);
    let document_source = masked_document
        .as_deref()
        .unwrap_or_else(|| parts.document.as_ref());
    let mut document = parse_document_with_entries(
        document_source,
        source_path,
        declarations,
        &mut entry_diagnostics,
    );
    if !entry_diagnostics.is_empty() {
        entry_diagnostics.extend(std::mem::take(&mut document.diagnostics));
        document.diagnostics = entry_diagnostics;
    }
    if !sanitize_diagnostics.is_empty() {
        sanitize_diagnostics.extend(std::mem::take(&mut document.diagnostics));
        document.diagnostics = sanitize_diagnostics;
    }
    Ok(ParsedMarkdown { document, tldr })
}

/// Mask a leading BOM and terminal-unsafe control characters with spaces.
///
/// A BOM would hide the tldr opening marker and demote the first heading, while
/// raw control characters would pass escape sequences through to terminals.
/// Replacements keep every byte offset valid for source coordinates.
fn sanitize_source(source_text: &str, diagnostics: &mut Vec<Diagnostic>) -> Option<String> {
    let bom = source_text.starts_with('\u{feff}');
    let rest = if bom {
        &source_text['\u{feff}'.len_utf8()..]
    } else {
        source_text
    };
    let (masked, controls) = mask_terminal_controls(rest);
    if !bom && masked.is_none() {
        return None;
    }

    let mut sanitized = String::with_capacity(source_text.len());
    if bom {
        sanitized.push_str("   ");
    }
    sanitized.push_str(masked.as_deref().unwrap_or(rest));

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
#[cfg(test)]
fn parse_document(source_text: &str, source_path: Option<String>) -> Document {
    let mut diagnostics = Vec::new();
    parse_document_with_entries(source_text, source_path, BTreeMap::new(), &mut diagnostics)
}

fn parse_document_with_entries(
    source_text: &str,
    source_path: Option<String>,
    mut declarations: BTreeMap<u32, options::EntryDeclaration>,
    entry_diagnostics: &mut Vec<Diagnostic>,
) -> Document {
    let source = MarkdownSource::new(source_text);
    let ParsedDocumentStructure {
        diagnostics,
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
    normalize_entry_lists(&mut root_blocks, &mut declarations, entry_diagnostics);
    normalize_section_entries(&mut sections, &mut declarations, entry_diagnostics);
    for declaration in declarations.into_values() {
        entry_diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some("markdown.semantic-entry-list".to_owned()),
            message: "semantic-entry directive did not resolve to a Markdown bullet list"
                .to_owned(),
            source: Some(declaration.source),
        });
    }
    let retained_targets = crate::definitions::identify_definitions(
        &mut root_blocks,
        &mut sections,
        &ids.targets.keys().cloned().collect(),
        source_path.as_deref(),
    );
    for target in retained_targets {
        ids.targets.insert(target.clone(), target);
    }
    entry_diagnostics.extend(crate::projection::semantic_selector_diagnostics(
        &root_blocks,
        &sections,
    ));
    let mut document = Document {
        parser: Some(markdown_parser()),
        source: DocumentSource {
            format: SourceFormat::Markdown,
            path: source_path,
        },
        meta: DocumentMeta {
            title,
            ..DocumentMeta::default()
        },
        diagnostics,
        blocks: root_blocks,
        sections,
    };
    LocalLinkResolver::new(&ids.targets).visit_document_mut(&mut document);
    document.diagnostics.extend(validate_document(&document));
    document
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
                    id: id.into(),
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

fn markdown_parser() -> ParserInfo {
    ParserInfo {
        name: "pulldown-cmark".to_owned(),
        version: "0.13".to_owned(),
    }
}

fn normalize_section_entries(
    sections: &mut [Section],
    declarations: &mut BTreeMap<u32, options::EntryDeclaration>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for section in sections {
        normalize_entry_lists(&mut section.blocks, declarations, diagnostics);
        normalize_section_entries(&mut section.children, declarations, diagnostics);
    }
}

fn markdown_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_WIKILINKS
}

fn take_explicit_heading_id(children: &mut Vec<Inline>) -> Option<String> {
    let (id, empty) = {
        let Inline::Text { value } = children.last_mut()? else {
            return None;
        };
        let trimmed = value.trim_end();
        let opening = trimmed.rfind("{#")?;
        if !trimmed.ends_with('}') {
            return None;
        }
        if opening != 0
            && !trimmed[..opening]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
        {
            return None;
        }
        let id = trimmed
            .get(opening + 2..trimmed.len().checked_sub(1)?)?
            .to_owned();
        if id.is_empty()
            || id.bytes().any(|byte| {
                byte.is_ascii_whitespace() || matches!(byte, b'{' | b'}' | b'\\' | b'<' | b'>')
            })
        {
            return None;
        }
        let title_end = trimmed[..opening].trim_end().len();
        value.truncate(title_end);
        (id, value.is_empty())
    };
    if empty {
        children.pop();
    }
    Some(id)
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
    assigned: HashSet<String>,
    targets: HashMap<String, String>,
}

impl SectionIds {
    fn allocate(&mut self, title: &str, explicit: Option<&str>) -> String {
        let explicit = explicit
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let base = explicit.clone().unwrap_or_else(|| slug(title));
        let base = if base.is_empty() {
            "section".to_owned()
        } else if crate::projection::is_reserved_selector(&base) {
            // Reserved selectors and bare tree paths would shadow this
            // heading in excerpt selection; keep it addressable instead.
            format!("{base}-section")
        } else {
            base
        };
        // Disambiguate on the final id, not the per-base count: `# Foo 2`
        // slugs to base `foo-2`, which collides with the `foo-2` a second
        // `# Foo` produces. Counting per base alone would hand both the same
        // id, silently misattributing search ownership between them.
        let count = self.counts.entry(base.clone()).or_default();
        let id = loop {
            *count += 1;
            let candidate = if *count == 1 {
                base.clone()
            } else {
                format!("{base}-{}", *count)
            };
            if self.assigned.insert(candidate.clone()) {
                break candidate;
            }
        };
        // Ambiguous human-facing keys resolve to the first section that
        // claimed them, matching the bare slug this heading renders as its
        // anchor. A later duplicate owns only its own disambiguated id.
        self.targets
            .entry(base.clone())
            .or_insert_with(|| id.clone());
        // Heading attributes are source-level link aliases. Preserve the
        // original alias even when its final section ID had to move out of the
        // selector namespace (`{#root}`, `{#2.1}`, or `{#2.1/e3}`).
        if let Some(explicit) = explicit {
            self.targets.entry(explicit).or_insert_with(|| id.clone());
        }
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

struct LocalLinkResolver<'targets> {
    targets: &'targets HashMap<String, String>,
}

impl<'targets> LocalLinkResolver<'targets> {
    fn new(targets: &'targets HashMap<String, String>) -> Self {
        Self { targets }
    }
}

impl VisitMut for LocalLinkResolver<'_> {
    fn visit_inline_mut(&mut self, inline: &mut Inline) {
        if let Inline::Link {
            target: mant_ir::LinkTarget::Section { id },
            ..
        } = inline
        {
            let lookup = id.trim().trim_start_matches('#');
            if let Some(resolved) = self
                .targets
                .get(lookup)
                .or_else(|| self.targets.get(&slug(lookup)))
            {
                *id = resolved.as_str().into();
            }
        }
        visit::walk_inline_mut(self, inline);
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
