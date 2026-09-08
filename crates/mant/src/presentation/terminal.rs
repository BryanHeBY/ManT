//! CLI-only style and terminal safety over shared semantic renderers.
use super::RenderOptions;
use crate::{arguments::QueryFormat, error::Failure};
use anstyle::{AnsiColor, Style};
use mant_ir::{DocumentMeta, EntryKind, ResolvedContent, SourceFormat};
use mant_protocol::{OutlineNode, QueryExcerpt, QueryOutline, QuerySearch, sanitize_terminal_text};
use std::fmt::Write as _;
/// Keep protocol-owned catalog text intact while applying optional CLI styling.
pub(crate) fn render_catalog_output(
    catalog: &mant_protocol::DocumentCatalog,
    grouped: bool,
    color: bool,
) -> String {
    let text = mant_protocol::render_catalog_coverage_text(catalog)
        .unwrap_or_else(|| mant_protocol::render_catalog_text(catalog, grouped));
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

struct TerminalText {
    value: String,
    color: bool,
}

impl TerminalText {
    fn new(color: bool) -> Self {
        Self {
            value: String::new(),
            color,
        }
    }

    fn plain(&mut self, value: &str) {
        self.value.push_str(&sanitize_terminal_text(value));
    }

    fn styled(&mut self, role: TerminalRole, value: &str) {
        let value = sanitize_terminal_text(value);
        if !self.color || value.is_empty() {
            self.plain(&value);
            return;
        }
        let style = terminal_style(role);
        let _ = write!(self.value, "{style}{value}{style:#}");
    }

    fn line(&mut self) {
        self.value.push('\n');
    }

    fn finish(self) -> String {
        self.value.trim_end().to_owned()
    }
}

pub(super) fn render_terminal_outline(outline: &QueryOutline, color: bool) -> String {
    if outline.nodes.is_empty() {
        return mant_engine::render_outline_text(outline);
    }
    let mut output = TerminalText::new(color);
    output.styled(
        TerminalRole::Document,
        &document_label(
            &outline.label,
            outline
                .meta
                .as_ref()
                .and_then(|meta| meta.manual_section.as_deref()),
        ),
    );
    if !outline.nodes.is_empty() {
        output.line();
        render_outline_nodes(&outline.nodes, "", &mut output);
    }
    output.finish()
}

fn render_outline_nodes(nodes: &[OutlineNode], prefix: &str, output: &mut TerminalText) {
    for (index, node) in nodes.iter().enumerate() {
        let last = index + 1 == nodes.len();
        output.styled(TerminalRole::TreeGuide, prefix);
        output.styled(TerminalRole::TreeGuide, if last { "└─" } else { "├─" });
        output.plain(" ");
        output.styled(TerminalRole::Path, node.path());
        output.styled(TerminalRole::TreeGuide, " [");
        output.styled(TerminalRole::Coordinate, node.id());
        output.styled(TerminalRole::TreeGuide, "] ");
        output.styled(outline_node_role(node), node.title());
        if let Some(summary) = outline_node_summary(node) {
            output.styled(
                TerminalRole::Muted,
                &mant_engine::render_outline_entry_summary(summary),
            );
        }
        output.styled(
            TerminalRole::Muted,
            &mant_engine::render_outline_relationships(node),
        );
        if index + 1 < nodes.len() || !node.children().is_empty() {
            output.line();
        }
        let child_prefix = format!("{prefix}{}", if last { "  " } else { "│ " });
        render_outline_nodes(node.children(), &child_prefix, output);
        if !node.children().is_empty() && index + 1 < nodes.len() {
            output.line();
        }
    }
}

fn outline_node_summary(node: &OutlineNode) -> Option<&mant_ir::EntrySummary> {
    match node {
        OutlineNode::DocumentRoot { entry_summary, .. }
        | OutlineNode::DocumentSection { entry_summary, .. }
        | OutlineNode::DocumentEntry { entry_summary, .. } => entry_summary.as_ref(),
        OutlineNode::Tldr { .. } => None,
    }
}

pub(super) fn render_terminal_excerpt(excerpt: &QueryExcerpt, color: bool) -> String {
    mant_engine::render_excerpt_text_with(excerpt, |style, text| {
        super::content::decorate(style, text, color)
    })
}

pub(super) fn render_terminal_explanation(
    explanation: &mant_protocol::QueryExplanation,
    color: bool,
) -> String {
    let plain = mant_engine::render_explanation_text(explanation);
    if !color {
        return plain;
    }
    let headings = explanation
        .evidence
        .iter()
        .map(mant_protocol::render_evidence_heading)
        .collect::<Vec<_>>();
    let terms = explanation
        .evidence
        .iter()
        .filter_map(|evidence| evidence.entry.as_ref())
        .flat_map(|entry| entry.names.iter().map(|name| (name.clone(), entry.kind)))
        .collect();
    style_content_text(&plain, &headings, terms)
}

pub(super) fn render_terminal_scope_explanation(
    explanation: &mant_protocol::ScopeExplanation,
    color: bool,
) -> String {
    let plain = mant_engine::render_scope_explanation_text(explanation);
    if !color {
        return plain;
    }
    let headings = explanation
        .evidence
        .iter()
        .map(|e| mant_protocol::render_evidence_heading(&e.evidence))
        .collect::<Vec<_>>();
    let terms = explanation
        .evidence
        .iter()
        .filter_map(|e| e.evidence.entry.as_ref())
        .flat_map(|e| e.names.iter().cloned().map(|name| (name, e.kind)))
        .collect();
    style_content_text(&plain, &headings, terms)
}

/// Add presentation styling without changing the text renderer's layout.
fn style_content_text(
    plain: &str,
    headings: &[String],
    mut terms: Vec<(String, EntryKind)>,
) -> String {
    terms.sort_by_key(|term| std::cmp::Reverse(term.0.len()));

    let mut output = TerminalText::new(true);
    for (index, line) in plain.split('\n').enumerate() {
        if index > 0 {
            output.line();
        }
        render_excerpt_line(line, index == 0, headings, &terms, &mut output);
    }
    output.finish()
}

pub(super) fn render_terminal_search(search: &QuerySearch, color: bool) -> String {
    mant_engine::render_search_text_with(search, |role, value| {
        let value = sanitize_terminal_text(value);
        if !color {
            return value.into_owned();
        }
        let role = match role {
            mant_engine::SearchTextRole::Plain => return value.into_owned(),
            mant_engine::SearchTextRole::Document => TerminalRole::Document,
            mant_engine::SearchTextRole::Coordinate => TerminalRole::Coordinate,
            mant_engine::SearchTextRole::Path => TerminalRole::Path,
            mant_engine::SearchTextRole::Heading => TerminalRole::Heading,
            mant_engine::SearchTextRole::Definition(kind) => entry_kind_role(kind),
            mant_engine::SearchTextRole::Match => TerminalRole::Match,
            mant_engine::SearchTextRole::Muted => TerminalRole::Muted,
        };
        let style = terminal_style(role);
        format!("{style}{value}{style:#}")
    })
}

fn render_excerpt_line(
    line: &str,
    document_line: bool,
    headings: &[String],
    terms: &[(String, EntryKind)],
    output: &mut TerminalText,
) {
    if document_line {
        output.styled(TerminalRole::Document, line);
        return;
    }
    if line == "TLDR" {
        output.styled(TerminalRole::Heading, line);
        return;
    }
    if let Some(rest) = line.strip_prefix("Outline ")
        && let Some((path, breadcrumb)) = rest.split_once(": ")
    {
        output.styled(TerminalRole::Muted, "Outline ");
        output.styled(TerminalRole::Path, path);
        output.styled(TerminalRole::Muted, ": ");
        for (index, title) in breadcrumb.split(" > ").enumerate() {
            if index > 0 {
                output.styled(TerminalRole::TreeGuide, " > ");
            }
            output.styled(TerminalRole::Heading, title);
        }
        return;
    }

    let trimmed = line.trim_start();
    let indent = line.len().saturating_sub(trimmed.len());
    if headings.iter().any(|heading| heading == trimmed) {
        output.plain(&line[..indent]);
        output.styled(TerminalRole::Heading, trimmed);
        return;
    }
    if let Some((term, role)) = terms
        .iter()
        .find(|(term, _)| !term.is_empty() && trimmed.starts_with(term))
    {
        output.plain(&line[..indent]);
        output.styled(entry_kind_role(*role), term);
        output.plain(&trimmed[term.len()..]);
        return;
    }
    output.plain(line);
}

const fn outline_node_role(node: &OutlineNode) -> TerminalRole {
    match node {
        OutlineNode::DocumentEntry { entry_kind, .. } => entry_kind_role(*entry_kind),
        OutlineNode::Tldr { .. }
        | OutlineNode::DocumentRoot { .. }
        | OutlineNode::DocumentSection { .. } => TerminalRole::Heading,
    }
}

const fn entry_kind_role(kind: EntryKind) -> TerminalRole {
    TerminalRole::Entry(kind)
}

pub(super) const fn terminal_style(role: TerminalRole) -> Style {
    match role {
        TerminalRole::Document => AnsiColor::BrightBlue.on_default().bold(),
        TerminalRole::Heading => AnsiColor::BrightCyan.on_default().bold(),
        TerminalRole::Entry(kind) => match mant_protocol::entry_tone(kind) {
            mant_protocol::EntryTone::Primary => Style::new().bold(),
            mant_protocol::EntryTone::Parameter => AnsiColor::BrightGreen.on_default().bold(),
            mant_protocol::EntryTone::Command => AnsiColor::BrightYellow.on_default().bold(),
            mant_protocol::EntryTone::Environment => AnsiColor::BrightCyan.on_default(),
            mant_protocol::EntryTone::Configuration => AnsiColor::BrightYellow.on_default(),
            mant_protocol::EntryTone::Variable => AnsiColor::BrightMagenta.on_default(),
            mant_protocol::EntryTone::Value => AnsiColor::BrightBlue.on_default(),
        },
        TerminalRole::Match => Style::new().underline().bold(),
        TerminalRole::Path => AnsiColor::BrightMagenta.on_default(),
        TerminalRole::Coordinate | TerminalRole::TreeGuide | TerminalRole::Muted => {
            AnsiColor::BrightBlack.on_default()
        }
    }
}

fn document_label(document: &str, section: Option<&str>) -> String {
    section.map_or_else(
        || document.to_owned(),
        |section| format!("{document}({section})"),
    )
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
    }
    query
}

pub(super) fn terminal_outline(outline: &QueryOutline) -> QueryOutline {
    let mut outline = outline.clone();
    outline.label = sanitize_terminal_text(&outline.label).into_owned();
    if let Some(meta) = outline.meta.as_mut() {
        sanitize_terminal_meta(meta);
    }
    outline
}

/// Copy an excerpt with its terminal-visible document identity made safe.
pub(super) fn terminal_excerpt(excerpt: &QueryExcerpt) -> QueryExcerpt {
    let mut excerpt = excerpt.clone();
    excerpt.label = sanitize_terminal_text(&excerpt.label).into_owned();
    if let Some(meta) = excerpt.meta.as_mut() {
        sanitize_terminal_meta(meta);
    }
    excerpt
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
            Ok(mant_engine::render_markdown_with_options(
                terminal_copy.as_ref().unwrap_or(query),
                mant_engine::MarkdownOptions {
                    preserve_anchors,
                    ..Default::default()
                },
            ))
        }
        QueryFormat::Text => Ok(mant_engine::render_query_text_with(query, |style, text| {
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
            Ok(mant_engine::render_query_man(&query))
        }
        QueryFormat::Json => {
            mant_engine::render_query_json(query, pretty).map_err(Failure::operational)
        }
    }
}
