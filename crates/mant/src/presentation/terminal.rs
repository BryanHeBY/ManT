//! CLI-only style and terminal safety over shared semantic renderers.
use super::RenderOptions;
use crate::{arguments::QueryFormat, error::Failure};
use anstyle::{AnsiColor, Style};
use mant_ir::{DocumentMeta, EntryKind, ResolvedContent, SourceFormat};
use mant_protocol::{QueryExcerpt, QueryOutline, QuerySearch};
use mant_render::sanitize_terminal_text;
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
        if let Some(heading) = &mut document.heading {
            sanitize_terminal_heading(heading);
        }
        for section in &mut document.sections {
            sanitize_terminal_section_headings(section);
        }
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
    for selection in &mut excerpt.selections {
        match selection {
            mant_protocol::ExcerptSelection::DocumentRoot {
                heading: Some(heading),
                ..
            } => sanitize_terminal_heading(heading),
            mant_protocol::ExcerptSelection::DocumentSection { section, .. } => {
                sanitize_terminal_section_headings(section);
            }
            _ => {}
        }
    }
    excerpt
}

fn sanitize_terminal_section_headings(section: &mut mant_ir::Section) {
    sanitize_terminal_heading(&mut section.heading);
    for child in &mut section.children {
        sanitize_terminal_section_headings(child);
    }
}

fn sanitize_terminal_heading(heading: &mut mant_ir::Heading) {
    use mant_ir::visit::VisitMut;
    struct Text;
    impl VisitMut for Text {
        fn visit_inline_mut(&mut self, inline: &mut mant_ir::Inline) {
            if let mant_ir::Inline::Text { value } | mant_ir::Inline::Code { value } = inline {
                *value = sanitize_terminal_text(value).into_owned();
            }
            mant_ir::visit::walk_inline_mut(self, inline);
        }
    }
    Text.visit_heading_mut(heading);
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
            if document.source.format == SourceFormat::Markdown {
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
