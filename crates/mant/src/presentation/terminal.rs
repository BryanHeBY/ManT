//! CLI-only style and terminal safety over shared semantic renderers.
use super::RenderOptions;
use crate::{arguments::QueryFormat, error::Failure};
use anstyle::{AnsiColor, Style};
use mant_ir::{DocumentMeta, EntryKind, ResolvedContent, SourceFormat};
use mant_protocol::{QueryExcerpt, QueryOutline, QuerySearch};
use mant_render::sanitize_terminal_text;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
/// Keep protocol-owned catalog text intact while applying optional CLI styling.
pub(crate) fn render_catalog_output(
    catalog: &mant_protocol::DocumentCatalog,
    grouped: bool,
    color: bool,
) -> String {
    let text = mant_render::render_catalog_coverage_text(catalog)
        .unwrap_or_else(|| mant_render::render_catalog_text(catalog, grouped));
    if !color || text.is_empty() {
        return text;
    }
    let style = terminal_style(TerminalRole::Path);
    let mut output = String::new();
    for line in text.split_inclusive('\n') {
        let (body, ending) = line
            .strip_suffix('\n')
            .map_or((line, ""), |body| (body, "\n"));
        let _ = write!(output, "{style}{body}{style:#}{ending}");
    }
    output
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalRole {
    Document,
    Heading,
    Entry(EntryKind),
    Match,
    Coordinate,
    Path,
    TreeGuide,
    Muted,
}

pub(super) fn render_terminal_outline(outline: &QueryOutline, color: bool) -> String {
    mant_render::render_outline_text_with(outline, |style, text| {
        super::content::decorate(style, text, color)
    })
}

pub(super) fn render_terminal_excerpt(excerpt: &QueryExcerpt, color: bool) -> String {
    mant_render::render_excerpt_text_with(excerpt, |style, text| {
        super::content::decorate(style, text, color)
    })
}

pub(super) fn render_terminal_explanation(
    explanation: &mant_protocol::QueryExplanation,
    color: bool,
) -> String {
    mant_render::render_explanation_text_with(explanation, |style, text| {
        super::content::decorate(style, text, color)
    })
}

pub(super) fn render_terminal_search(search: &QuerySearch, color: bool) -> String {
    mant_render::render_search_text_with(search, |role, value| decorate_search(role, value, color))
}

pub(super) fn decorate_search(
    role: mant_render::SearchTextRole,
    value: &str,
    color: bool,
) -> String {
    let value = sanitize_terminal_text(value);
    if !color {
        return value.into_owned();
    }
    let role = match role {
        mant_render::SearchTextRole::Plain => return value.into_owned(),
        mant_render::SearchTextRole::Document => TerminalRole::Document,
        mant_render::SearchTextRole::Coordinate => TerminalRole::Coordinate,
        mant_render::SearchTextRole::Path => TerminalRole::Path,
        mant_render::SearchTextRole::Heading => TerminalRole::Heading,
        mant_render::SearchTextRole::Definition(kind) => entry_kind_role(kind),
        mant_render::SearchTextRole::Match => TerminalRole::Match,
        mant_render::SearchTextRole::Muted => TerminalRole::Muted,
    };
    let style = terminal_style(role);
    format!("{style}{value}{style:#}")
}

const fn entry_kind_role(kind: EntryKind) -> TerminalRole {
    TerminalRole::Entry(kind)
}

pub(super) const fn terminal_style(role: TerminalRole) -> Style {
    match role {
        TerminalRole::Document => AnsiColor::BrightBlue.on_default().bold(),
        TerminalRole::Heading => AnsiColor::BrightCyan.on_default().bold(),
        TerminalRole::Entry(kind) => match mant_render::entry_tone(kind) {
            mant_render::EntryTone::Primary => Style::new().bold(),
            mant_render::EntryTone::Parameter => AnsiColor::BrightGreen.on_default().bold(),
            mant_render::EntryTone::Command => AnsiColor::BrightYellow.on_default().bold(),
            mant_render::EntryTone::Environment => AnsiColor::Magenta.on_default(),
            mant_render::EntryTone::Configuration => AnsiColor::BrightYellow.on_default(),
            mant_render::EntryTone::Variable => AnsiColor::BrightMagenta.on_default(),
            mant_render::EntryTone::Value => AnsiColor::BrightBlue.on_default(),
        },
        TerminalRole::Match => Style::new().underline().bold(),
        TerminalRole::Path => AnsiColor::BrightMagenta.on_default(),
        TerminalRole::Coordinate | TerminalRole::TreeGuide | TerminalRole::Muted => {
            AnsiColor::BrightBlack.on_default()
        }
    }
}

/// Copy a complete document with its terminal-visible identity made safe.
///
/// Engine text renderers produce structural newlines themselves, so sanitizing
/// their finished string would erase layout. The identity is the only
/// terminal-visible direct-input field that bypasses the parsed text-safety
/// boundary; sanitize it before rendering instead.
fn terminal_content(query: &ResolvedContent) -> ResolvedContent {
    let mut query = query.clone();
    query.label = sanitize_terminal_text(&query.label).into_owned();
    if let Some(document) = query.document.as_mut() {
        sanitize_terminal_meta(&mut document.meta);
        sanitize_terminal_document_headings(document);
    }
    query
}

pub(super) fn terminal_outline(outline: &QueryOutline) -> QueryOutline {
    let mut outline = outline.clone();
    outline.label = sanitize_terminal_text(&outline.label).into_owned();
    if let Some(title) = &mut outline.display_title {
        *title = sanitize_terminal_text(title).into_owned();
    }
    if let Some(meta) = outline.meta.as_mut() {
        sanitize_terminal_meta(meta);
    }
    outline
}

/// Copy an excerpt with its terminal-visible document identity made safe.
pub(super) fn terminal_excerpt(excerpt: &QueryExcerpt) -> QueryExcerpt {
    let mut excerpt = excerpt.clone();
    excerpt.label = sanitize_terminal_text(&excerpt.label).into_owned();
    if let Some(title) = &mut excerpt.display_title {
        *title = sanitize_terminal_text(title).into_owned();
    }
    if let Some(meta) = excerpt.meta.as_mut() {
        sanitize_terminal_meta(meta);
    }
    if let Some(projection) = excerpt.content_projection.as_mut() {
        sanitize_terminal_excerpt_headings(&mut excerpt.selections, &mut projection.content_store);
    }
    excerpt
}

fn sanitize_terminal_document_headings(document: &mut mant_ir::Document) {
    let mut atoms = HashSet::new();
    if let Some(heading) = &document.heading {
        collect_heading_atoms(heading, &mut atoms);
    }
    for section in &document.sections {
        collect_section_heading_atoms(section, &mut atoms);
    }
    let original = sanitize_terminal_atoms(&mut document.content_store, &atoms);
    remap_document_content_refs(document, &original);
}

fn sanitize_terminal_excerpt_headings(
    selections: &mut [mant_protocol::ExcerptSelection],
    store: &mut mant_ir::ContentStore,
) {
    use mant_ir::visit::VisitMut as _;
    let mut atoms = HashSet::new();
    for selection in &*selections {
        match selection {
            mant_protocol::ExcerptSelection::DocumentRoot {
                heading: Some(heading),
                ..
            } => collect_heading_atoms(heading, &mut atoms),
            mant_protocol::ExcerptSelection::DocumentSection { section, .. } => {
                collect_section_heading_atoms(section, &mut atoms);
            }
            mant_protocol::ExcerptSelection::Tldr { .. }
            | mant_protocol::ExcerptSelection::DocumentRoot { heading: None, .. }
            | mant_protocol::ExcerptSelection::DocumentEntry { .. } => {}
        }
    }
    let original = sanitize_terminal_atoms(store, &atoms);
    let mut remap = ContentRefRemap {
        original: &original,
    };
    for selection in selections {
        match selection {
            mant_protocol::ExcerptSelection::DocumentRoot {
                heading: Some(heading),
                ..
            } => remap.visit_heading_mut(heading),
            mant_protocol::ExcerptSelection::DocumentSection { section, .. } => {
                remap.visit_section_mut(section);
            }
            mant_protocol::ExcerptSelection::Tldr { .. }
            | mant_protocol::ExcerptSelection::DocumentRoot { heading: None, .. }
            | mant_protocol::ExcerptSelection::DocumentEntry { .. } => {}
        }
    }
    remap_link_label_refs(store, &original);
}

fn collect_section_heading_atoms(
    section: &mant_ir::Section,
    atoms: &mut HashSet<mant_ir::ContentAtomKey>,
) {
    collect_heading_atoms(&section.heading, atoms);
    for child in &section.children {
        collect_section_heading_atoms(child, atoms);
    }
}

fn collect_heading_atoms(heading: &mant_ir::Heading, atoms: &mut HashSet<mant_ir::ContentAtomKey>) {
    fn collect(inline: &mant_ir::Inline, atoms: &mut HashSet<mant_ir::ContentAtomKey>) {
        match inline {
            mant_ir::Inline::Text { content } | mant_ir::Inline::Code { content } => {
                atoms.insert(content.atom);
            }
            mant_ir::Inline::Strong { children }
            | mant_ir::Inline::Emphasis { children }
            | mant_ir::Inline::Link { children, .. } => {
                for child in children {
                    collect(child, atoms);
                }
            }
            mant_ir::Inline::Anchor { .. } | mant_ir::Inline::LineBreak { .. } => {}
        }
    }

    for inline in &heading.content {
        collect(inline, atoms);
    }
}

fn sanitize_terminal_atoms(
    store: &mut mant_ir::ContentStore,
    keys: &HashSet<mant_ir::ContentAtomKey>,
) -> HashMap<mant_ir::ContentAtomKey, String> {
    let mut original = HashMap::new();
    for atom in &mut store.atoms {
        if !keys.contains(&atom.key) {
            continue;
        }
        let text = match &mut atom.kind {
            mant_ir::ContentAtomKind::Text { text, .. }
            | mant_ir::ContentAtomKind::Whitespace { text, .. } => text,
            mant_ir::ContentAtomKind::BreakOpportunity {}
            | mant_ir::ContentAtomKind::HardBreak {} => continue,
        };
        if matches!(sanitize_terminal_text(text), std::borrow::Cow::Owned(_)) {
            let value = std::mem::take(text);
            *text = sanitize_terminal_text(&value).into_owned();
            original.insert(atom.key, value);
        }
    }
    original
}

fn remap_document_content_refs(
    document: &mut mant_ir::Document,
    original: &HashMap<mant_ir::ContentAtomKey, String>,
) {
    use mant_ir::visit::VisitMut as _;
    ContentRefRemap { original }.visit_document_mut(document);
    remap_link_label_refs(&mut document.content_store, original);
}

struct ContentRefRemap<'a> {
    original: &'a HashMap<mant_ir::ContentAtomKey, String>,
}

impl mant_ir::visit::VisitMut for ContentRefRemap<'_> {
    fn visit_inline_mut(&mut self, inline: &mut mant_ir::Inline) {
        if let mant_ir::Inline::Text { content } | mant_ir::Inline::Code { content } = inline {
            remap_content_ref(content, self.original);
        }
        mant_ir::visit::walk_inline_mut(self, inline);
    }
}

fn remap_link_label_refs(
    store: &mut mant_ir::ContentStore,
    original: &HashMap<mant_ir::ContentAtomKey, String>,
) {
    for occurrence in &mut store.links {
        for part in &mut occurrence.label {
            if let mant_ir::LinkLabelPart::Content { content } = part {
                remap_content_ref(content, original);
            }
        }
    }
}

fn remap_content_ref(
    content: &mut mant_ir::ContentRef,
    original: &HashMap<mant_ir::ContentAtomKey, String>,
) {
    let Some(value) = original.get(&content.atom) else {
        return;
    };
    if let Some(start) = sanitized_byte_offset(value, content.bytes.start)
        && let Some(end) = sanitized_byte_offset(value, content.bytes.end)
    {
        content.bytes.start = start;
        content.bytes.end = end;
    }
}

fn sanitized_byte_offset(value: &str, offset: u32) -> Option<u32> {
    let prefix = value.get(..usize::try_from(offset).ok()?)?;
    prefix.chars().try_fold(0_u32, |length, character| {
        let bytes = if character.is_control() {
            '\u{fffd}'.len_utf8()
        } else {
            character.len_utf8()
        };
        length.checked_add(u32::try_from(bytes).ok()?)
    })
}

pub(super) fn terminal_search(search: &QuerySearch) -> QuerySearch {
    let mut search = search.clone();
    search.label = sanitize_terminal_text(&search.label).into_owned();
    if let Some(meta) = search.meta.as_mut() {
        sanitize_terminal_meta(meta);
    }
    search
}

fn sanitize_terminal_meta(meta: &mut DocumentMeta) {
    for value in [
        &mut meta.title,
        &mut meta.date,
        &mut meta.volume,
        &mut meta.os,
        &mut meta.arch,
        &mut meta.alias_target,
    ]
    .into_iter()
    .flatten()
    {
        *value = sanitize_terminal_text(value).into_owned();
    }
    if let Some(section) = meta.manual_section.as_mut() {
        *section = sanitize_terminal_text(section).into_owned();
    }
    for name in &mut meta.names {
        *name = sanitize_terminal_text(name).into_owned();
    }
}

pub(super) fn render_full_query(
    query: &ResolvedContent,
    options: RenderOptions,
) -> Result<String, Failure> {
    let RenderOptions {
        format,
        pretty,
        preserve_anchors,
        ..
    } = options;
    let output_terminal = options.terminal();
    match format {
        QueryFormat::Markdown => {
            let terminal_copy = output_terminal.then(|| terminal_content(query));
            Ok(mant_codec::encode::render_markdown_with_options(
                terminal_copy.as_ref().unwrap_or(query),
                mant_codec::encode::MarkdownOptions {
                    preserve_anchors,
                    ..Default::default()
                },
            ))
        }
        QueryFormat::Text => Ok(mant_render::render_query_text_with(query, |style, text| {
            super::content::decorate(style, text, options.color)
        })),
        QueryFormat::Man => {
            let Some(document) = query.document.as_ref() else {
                return Err(Failure::operational(
                    "manual page is unavailable; --format man cannot render tldr-only content",
                ));
            };
            if document.root_format() == Some(SourceFormat::Markdown) {
                return Err(Failure::usage(
                    "--format man applies only to roff manual pages",
                ));
            }
            let query = terminal_content(query);
            Ok(mant_render::render_query_man(&query))
        }
        QueryFormat::Json => {
            mant_render::render_query_json(query, pretty).map_err(Failure::operational)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_terminal_document_headings;
    use mant_ir::{
        ContentOwnerKind, ContentRootKind, ContentStoreBuilder, ContentStyle, Document,
        DocumentMeta, Heading, Inline, LinkTarget, Provenance, SourceCoordinates, SourceFormat,
        SourceIdentity, SourceKey, SourceRecord,
    };

    #[test]
    fn terminal_heading_sanitization_keeps_store_ranges_and_link_labels_closed() {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Document, Provenance::Unknown);
        let root = builder.push_root(owner, ContentRootKind::Heading, Provenance::Unknown);
        let occurrence = builder.push_link(
            owner,
            LinkTarget::External {
                uri: "https://example.com".to_owned(),
            },
            None,
            Provenance::Unknown,
        );
        let content = builder.push_text(
            root,
            "safe\u{1b}title".to_owned(),
            None,
            ContentStyle::default(),
            None,
            Some(occurrence),
            Provenance::Unknown,
        );
        let mut document = Document {
            parser: None,
            sources: vec![SourceRecord {
                key: SourceKey::FIRST,
                identity: SourceIdentity::Anonymous {
                    name: "terminal-test".to_owned(),
                },
                format: SourceFormat::Markdown,
                decoded_byte_length: 0,
                content_sha256: None,
                coordinates: SourceCoordinates::DecodedUtf8Bytes,
            }],
            root_source: SourceKey::FIRST,
            content_store: builder.finish(),
            meta: DocumentMeta::default(),
            heading: Some(Heading {
                content: vec![Inline::Link {
                    occurrence,
                    children: vec![Inline::Text { content }],
                }],
                source: None,
            }),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: Vec::new(),
        };

        sanitize_terminal_document_headings(&mut document);

        let document_content = document.content();
        assert_eq!(
            document
                .heading
                .as_ref()
                .unwrap()
                .plain_text(document_content),
            "safe\u{fffd}title"
        );
        assert_eq!(
            document_content.occurrence_plain_text(occurrence).unwrap(),
            "safe\u{fffd}title"
        );
        mant_ir::validate_content_store(&document.content_store).unwrap();
    }
}
