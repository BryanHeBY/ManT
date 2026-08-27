//! Deterministic rendering of already materialized query views.

use std::fmt::Write as _;

use anstyle::{AnsiColor, Style};
use mant_engine::QueryViewResult;
use mant_ir::{
    Block, DefinitionRole, DocumentMeta, EntryKind, Inline, ResolvedContent, Section, SourceFormat,
};
use mant_protocol::{
    ExcerptSelection, OutlineNode, QueryExcerpt, QueryOutline, QuerySearch, ScopeQueryResponse,
    ScopeQueryResult, ScopedQueryFailure, ScopedSearchDocument, SearchQuery, SearchSchema,
    sanitize_terminal_text,
};
use serde::Serialize;

use crate::{arguments::QueryFormat, error::Failure};

/// Physical destination characteristics that may affect terminal safety only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OutputTarget {
    Stream,
    Terminal,
}

/// Complete rendering policy after command-line defaults have been resolved.
#[derive(Debug, Clone, Copy)]
pub(super) struct RenderOptions {
    pub(super) format: QueryFormat,
    pub(super) pretty: bool,
    pub(super) preserve_anchors: bool,
    pub(super) color: bool,
    pub(super) target: OutputTarget,
}

impl RenderOptions {
    const fn terminal(self) -> bool {
        matches!(self.target, OutputTarget::Terminal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalRole {
    Document,
    Heading,
    Option,
    Command,
    Environment,
    Variable,
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

fn render_terminal_outline(outline: &QueryOutline, color: bool) -> String {
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

fn render_terminal_excerpt(excerpt: &QueryExcerpt, color: bool) -> String {
    let excerpt = terminal_excerpt(excerpt);
    let plain = mant_engine::render_excerpt_text(&excerpt);
    if !color || plain.is_empty() {
        return plain;
    }

    let mut headings = Vec::new();
    let mut terms = Vec::new();
    for selection in &excerpt.selections {
        collect_excerpt_semantics(selection, &mut headings, &mut terms);
    }
    terms.sort_by_key(|term| std::cmp::Reverse(term.0.len()));

    let mut output = TerminalText::new(true);
    for (index, line) in plain.split('\n').enumerate() {
        if index > 0 {
            output.line();
        }
        render_excerpt_line(line, index == 0, &headings, &terms, &mut output);
    }
    output.finish()
}

fn render_terminal_search(search: &QuerySearch, color: bool) -> String {
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
            mant_engine::SearchTextRole::Definition(role) => definition_role(role),
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
    terms: &[(String, DefinitionRole)],
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
        output.styled(definition_role(*role), term);
        output.plain(&trimmed[term.len()..]);
        return;
    }
    output.plain(line);
}

fn collect_excerpt_semantics(
    selection: &ExcerptSelection,
    headings: &mut Vec<String>,
    terms: &mut Vec<(String, DefinitionRole)>,
) {
    match selection {
        ExcerptSelection::Tldr { .. } | ExcerptSelection::DocumentRoot { .. } => {}
        ExcerptSelection::DocumentSection { section, .. } => {
            collect_section_semantics(section, headings, terms);
        }
        ExcerptSelection::DocumentEntry { entry, .. } => collect_definition(entry, terms),
    }
}

fn collect_section_semantics(
    section: &Section,
    headings: &mut Vec<String>,
    terms: &mut Vec<(String, DefinitionRole)>,
) {
    headings.push(section.title.clone());
    collect_block_semantics(&section.blocks, terms);
    for child in &section.children {
        collect_section_semantics(child, headings, terms);
    }
}

fn collect_block_semantics(blocks: &[Block], terms: &mut Vec<(String, DefinitionRole)>) {
    for block in blocks {
        match block {
            Block::DefinitionList { items, .. } => {
                for item in items {
                    collect_definition(item, terms);
                    collect_block_semantics(&item.description, terms);
                }
            }
            Block::List { items, .. } => {
                for item in items {
                    collect_block_semantics(&item.blocks, terms);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    collect_block_semantics(&cell.blocks, terms);
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn collect_definition(item: &mant_ir::DefinitionItem, terms: &mut Vec<(String, DefinitionRole)>) {
    let Some(identity) = &item.identity else {
        return;
    };
    terms.extend(
        item.terms
            .iter()
            .map(|term| (inline_text(term), identity.role)),
    );
}

fn inline_text(inlines: &[Inline]) -> String {
    let mut output = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => output.push_str(&inline_text(children)),
            Inline::Anchor { .. } => {}
            Inline::LineBreak => output.push('\n'),
        }
    }
    output
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
    match kind {
        EntryKind::Parameter { .. } => TerminalRole::Option,
        EntryKind::Command => TerminalRole::Command,
        EntryKind::EnvironmentVariable => TerminalRole::Environment,
        EntryKind::Variable | EntryKind::ConfigurationKey => TerminalRole::Variable,
        EntryKind::Value | EntryKind::Term => TerminalRole::Muted,
    }
}

const fn definition_role(role: DefinitionRole) -> TerminalRole {
    match role {
        DefinitionRole::Option | DefinitionRole::Marker | DefinitionRole::Operand => {
            TerminalRole::Option
        }
        DefinitionRole::Command => TerminalRole::Command,
        DefinitionRole::EnvironmentVariable => TerminalRole::Environment,
        DefinitionRole::ConfigurationKey | DefinitionRole::Variable => TerminalRole::Variable,
        DefinitionRole::Value | DefinitionRole::Term => TerminalRole::Muted,
    }
}

const fn terminal_style(role: TerminalRole) -> Style {
    match role {
        TerminalRole::Document => AnsiColor::BrightBlue.on_default().bold(),
        TerminalRole::Heading => AnsiColor::BrightCyan.on_default().bold(),
        TerminalRole::Option => AnsiColor::BrightGreen.on_default().bold(),
        TerminalRole::Command | TerminalRole::Match => AnsiColor::BrightYellow.on_default().bold(),
        TerminalRole::Environment => AnsiColor::BrightCyan.on_default(),
        TerminalRole::Variable | TerminalRole::Path => AnsiColor::BrightMagenta.on_default(),
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

fn terminal_outline(outline: &QueryOutline) -> QueryOutline {
    let mut outline = outline.clone();
    outline.label = sanitize_terminal_text(&outline.label).into_owned();
    if let Some(meta) = outline.meta.as_mut() {
        sanitize_terminal_meta(meta);
    }
    outline
}

/// Copy an excerpt with its terminal-visible document identity made safe.
fn terminal_excerpt(excerpt: &QueryExcerpt) -> QueryExcerpt {
    let mut excerpt = excerpt.clone();
    excerpt.label = sanitize_terminal_text(&excerpt.label).into_owned();
    if let Some(meta) = excerpt.meta.as_mut() {
        sanitize_terminal_meta(meta);
    }
    excerpt
}

fn terminal_search(search: &QuerySearch) -> QuerySearch {
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

pub(super) fn render_query_result(
    result: &QueryViewResult,
    options: RenderOptions,
) -> Result<String, Failure> {
    let RenderOptions {
        format,
        pretty,
        color,
        ..
    } = options;
    let output_terminal = options.terminal();
    match result {
        QueryViewResult::Full(query) => render_full_query(query, options),
        QueryViewResult::Outline(outline) => match format {
            QueryFormat::Markdown if output_terminal => Ok(mant_engine::render_outline_markdown(
                &terminal_outline(outline),
            )),
            QueryFormat::Markdown => Ok(mant_engine::render_outline_markdown(outline)),
            QueryFormat::Text => Ok(render_terminal_outline(outline, color)),
            QueryFormat::Man => Err(Failure::usage(
                "--format man applies only to full documents",
            )),
            QueryFormat::Json => {
                mant_engine::render_outline_json(outline, pretty).map_err(Failure::operational)
            }
        },
        QueryViewResult::Excerpt(excerpt) => render_excerpt(excerpt, options),
        QueryViewResult::Search(search) => match format {
            QueryFormat::Markdown if output_terminal => Ok(mant_engine::render_search_markdown(
                &terminal_search(search),
            )),
            QueryFormat::Markdown => Ok(mant_engine::render_search_markdown(search)),
            QueryFormat::Text => Ok(render_terminal_search(search, color)),
            QueryFormat::Man => Err(Failure::usage(
                "--format man applies only to full documents",
            )),
            QueryFormat::Json => {
                mant_engine::render_search_json(search, pretty).map_err(Failure::operational)
            }
        },
    }
}

pub(super) fn render_scope_query_result(
    response: &ScopeQueryResponse,
    options: RenderOptions,
) -> Result<String, Failure> {
    let RenderOptions {
        format,
        pretty,
        preserve_anchors,
        color,
        ..
    } = options;
    let output_terminal = options.terminal();
    if format == QueryFormat::Json {
        return render_json(response, pretty);
    }
    if format == QueryFormat::Man {
        return Err(Failure::usage(
            "--format man applies only to one full native manual",
        ));
    }
    let mut output = String::new();
    match &response.result {
        ScopeQueryResult::Explain {
            entry,
            matches,
            missed,
            failures,
        } => {
            if matches.is_empty() && failures.is_empty() && *missed > 0 {
                write_scope_explain_miss(
                    &mut output,
                    response,
                    entry,
                    *missed,
                    format,
                    output_terminal,
                );
            }
            for (index, found) in matches.iter().enumerate() {
                if index > 0 {
                    output.push_str("\n\n");
                }
                write_scope_heading(
                    &mut output,
                    &found.address.catalog_path(),
                    format,
                    color,
                    output_terminal,
                );
                output.push('\n');
                let rendered = match format {
                    QueryFormat::Markdown => {
                        let excerpt = output_terminal.then(|| terminal_excerpt(&found.excerpt));
                        mant_engine::render_excerpt_markdown_with_options(
                            excerpt.as_ref().unwrap_or(&found.excerpt),
                            mant_engine::MarkdownOptions { preserve_anchors },
                        )
                    }
                    QueryFormat::Text => render_terminal_excerpt(&found.excerpt, color),
                    QueryFormat::Json | QueryFormat::Man => unreachable!(),
                };
                output.push_str(rendered.trim());
            }
            write_scope_failures(&mut output, failures, format, color, output_terminal);
        }
        ScopeQueryResult::Search { search } => {
            for (index, found) in search.documents.iter().enumerate() {
                if index > 0 {
                    output.push_str("\n\n");
                }
                write_scope_heading(
                    &mut output,
                    &found.address.catalog_path(),
                    format,
                    color,
                    output_terminal,
                );
                output.push('\n');
                let local_search = scoped_search_projection(found, &search.query);
                let rendered = match format {
                    QueryFormat::Markdown => {
                        let search = output_terminal.then(|| terminal_search(&local_search));
                        mant_engine::render_search_markdown(
                            search.as_ref().unwrap_or(&local_search),
                        )
                    }
                    QueryFormat::Text => render_terminal_search(&local_search, color),
                    QueryFormat::Json | QueryFormat::Man => unreachable!(),
                };
                output.push_str(rendered.trim());
            }
        }
    }
    Ok(output)
}

fn write_scope_failures(
    output: &mut String,
    failures: &[ScopedQueryFailure],
    format: QueryFormat,
    color: bool,
    output_terminal: bool,
) {
    for failure in failures {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        write_scope_heading(
            output,
            &failure.address.catalog_path(),
            format,
            color,
            output_terminal,
        );
        output.push('\n');
        if format == QueryFormat::Text || output_terminal {
            output.push_str(&sanitize_terminal_text(&failure.reason));
        } else {
            output.push_str(&failure.reason);
        }
    }
}

fn write_scope_explain_miss(
    output: &mut String,
    response: &ScopeQueryResponse,
    entry: &str,
    missed: u32,
    format: QueryFormat,
    output_terminal: bool,
) {
    let document = response.scope.documents.first().map_or_else(
        || "DOCUMENT".to_owned(),
        |document| document.address.catalog_path(),
    );
    let message = format!(
        "No semantic entry '{entry}' across {missed} resolved documents.\n\
         hint: run `mant {document} --outline --outline-entries all --format json` \
         for available selectors, then repeat for the other resolved documents; \
         use `--search` when the term may occur only in prose"
    );
    if format == QueryFormat::Text || output_terminal {
        output.push_str(&sanitize_terminal_text(&message));
    } else {
        output.push_str(&message);
    }
}

fn scoped_search_projection(found: &ScopedSearchDocument, query: &SearchQuery) -> QuerySearch {
    let (label, meta) = match &found.address {
        mant_protocol::DocumentAddress::Manual {
            name,
            manual_section,
        } => (
            name.clone(),
            Some(DocumentMeta {
                manual_section: Some(manual_section.clone()),
                ..DocumentMeta::default()
            }),
        ),
        mant_protocol::DocumentAddress::Markdown { path, .. } => (path.clone(), None),
    };
    let returned = u32::try_from(found.matches.len()).unwrap_or(u32::MAX);
    QuerySearch {
        schema: SearchSchema::V0Dot10,
        label,
        source: None,
        meta,
        query: query.clone(),
        render: found.render.clone(),
        total: returned,
        returned,
        offset: 0,
        truncated: false,
        next_offset: None,
        matches: found.matches.clone(),
    }
}

fn write_scope_heading(
    output: &mut String,
    address: &str,
    format: QueryFormat,
    color: bool,
    output_terminal: bool,
) {
    let address = if format == QueryFormat::Text || output_terminal {
        sanitize_terminal_text(address)
    } else {
        std::borrow::Cow::Borrowed(address)
    };
    match format {
        QueryFormat::Markdown => {
            output.push_str("## ");
            output.push_str(&address);
        }
        QueryFormat::Text if color => {
            let style = terminal_style(TerminalRole::Document);
            write!(output, "{style}{address}{style:#}").expect("writing to String cannot fail");
        }
        QueryFormat::Text => output.push_str(&address),
        QueryFormat::Json | QueryFormat::Man => unreachable!(),
    }
}

fn render_excerpt(
    excerpt: &mant_protocol::QueryExcerpt,
    options: RenderOptions,
) -> Result<String, Failure> {
    let RenderOptions {
        format,
        pretty,
        preserve_anchors,
        color,
        ..
    } = options;
    let output_terminal = options.terminal();
    match format {
        QueryFormat::Markdown => {
            let terminal_copy = output_terminal.then(|| terminal_excerpt(excerpt));
            Ok(mant_engine::render_excerpt_markdown_with_options(
                terminal_copy.as_ref().unwrap_or(excerpt),
                mant_engine::MarkdownOptions { preserve_anchors },
            ))
        }
        QueryFormat::Text => Ok(render_terminal_excerpt(excerpt, color)),
        QueryFormat::Man => Err(Failure::usage(
            "--format man applies only to full documents",
        )),
        QueryFormat::Json => {
            mant_engine::render_excerpt_json(excerpt, pretty).map_err(Failure::operational)
        }
    }
}

fn render_full_query(query: &ResolvedContent, options: RenderOptions) -> Result<String, Failure> {
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
                mant_engine::MarkdownOptions { preserve_anchors },
            ))
        }
        QueryFormat::Text => {
            let query = terminal_content(query);
            Ok(mant_engine::render_query_text(&query))
        }
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

pub(super) fn render_json(value: &impl Serialize, pretty: bool) -> Result<String, Failure> {
    if pretty {
        serde_json::to_string_pretty(value).map_err(Failure::operational)
    } else {
        serde_json::to_string(value).map_err(Failure::operational)
    }
}

#[cfg(test)]
mod tests {
    use mant_engine::{project_query_view, query_markdown_text};
    use mant_protocol::{EntryProjection, QueryView};

    use super::{OutputTarget, QueryFormat, RenderOptions, render_query_result};

    const PAGE: &str = r"# demo

## Options

<!-- mant:entries role=option -->
- `--color WHEN`: Select terminal colour behavior.

The selected color is visible in terminal output.
";

    #[test]
    fn terminal_styles_do_not_change_visible_query_text() {
        for view in [
            QueryView::Outline {
                entries: EntryProjection::All,
                root: None,
            },
            QueryView::Explain {
                entry: "--color".to_owned(),
            },
            QueryView::Search {
                pattern: "color".to_owned(),
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Insensitive,
                scope: mant_protocol::SearchScope::Visible,
                word: false,
                context_lines: 1,
                limit: 100,
                offset: 0,
            },
        ] {
            let query = query_markdown_text(PAGE, None).expect("Markdown query");
            let result = project_query_view(query, &view).expect("query projection");
            let plain = render_query_result(
                &result,
                options(QueryFormat::Text, false, OutputTarget::Stream),
            )
            .expect("plain terminal text");
            let colored = render_query_result(
                &result,
                options(QueryFormat::Text, true, OutputTarget::Terminal),
            )
            .expect("colored terminal text");
            assert!(colored.contains("\x1b["));
            assert_eq!(strip_ansi(&colored), plain);
        }
    }

    #[test]
    fn terminal_outline_reports_an_empty_kind_projection() {
        let query = query_markdown_text(PAGE, None).expect("Markdown query");
        let result = project_query_view(
            query,
            &QueryView::Outline {
                entries: EntryProjection::Kinds {
                    kinds: vec![mant_ir::EntryKind::EnvironmentVariable],
                },
                root: None,
            },
        )
        .expect("empty environment outline");

        let rendered = render_query_result(
            &result,
            options(QueryFormat::Text, true, OutputTarget::Terminal),
        )
        .expect("terminal outline");
        assert_eq!(
            rendered,
            "stdin\n0 matching semantic entries for: environment variables"
        );
    }

    #[test]
    fn structured_and_markdown_formats_never_receive_terminal_styles() {
        let query = query_markdown_text(PAGE, None).expect("Markdown query");
        let result = project_query_view(
            query,
            &QueryView::Explain {
                entry: "--color".to_owned(),
            },
        )
        .expect("explanation");
        for format in [QueryFormat::Markdown, QueryFormat::Json] {
            let rendered =
                render_query_result(&result, options(format, true, OutputTarget::Stream))
                    .expect("deterministic output");
            assert!(!rendered.contains("\x1b["));
        }
    }

    #[test]
    fn uncoloured_terminal_presentations_mask_controls_in_direct_input_labels() {
        let source_path = "ev\u{1b}[31mil.md".to_owned();
        let views = [
            QueryView::Full {},
            QueryView::Outline {
                entries: EntryProjection::All,
                root: None,
            },
            QueryView::Explain {
                entry: "--color".to_owned(),
            },
            QueryView::Search {
                pattern: "color".to_owned(),
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Insensitive,
                scope: mant_protocol::SearchScope::Visible,
                word: false,
                context_lines: 1,
                limit: 100,
                offset: 0,
            },
        ];

        for view in views {
            let query = query_markdown_text(PAGE, Some(source_path.clone()))
                .expect("Markdown query with hostile label");
            let result = project_query_view(query, &view).expect("query projection");
            let rendered = render_query_result(
                &result,
                options(QueryFormat::Text, false, OutputTarget::Terminal),
            )
            .expect("plain terminal text");

            assert!(!rendered.contains('\u{1b}'));
            assert!(rendered.contains("ev�[31mil.md"));
        }
    }

    #[test]
    fn terminal_markdown_masks_dynamic_controls_without_rewriting_redirected_data() {
        for view in [
            QueryView::Full {},
            QueryView::Outline {
                entries: EntryProjection::All,
                root: None,
            },
            QueryView::Explain {
                entry: "--color".to_owned(),
            },
            QueryView::Search {
                pattern: "color".to_owned(),
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Insensitive,
                scope: mant_protocol::SearchScope::Visible,
                word: false,
                context_lines: 1,
                limit: 100,
                offset: 0,
            },
        ] {
            let mut query = query_markdown_text(PAGE, Some("ris\u{1b}c.md".to_owned()))
                .expect("Markdown query with hostile label");
            query.document.as_mut().expect("parsed document").meta.title =
                Some("ris\u{1b}c".to_owned());
            let result = project_query_view(query, &view).expect("query projection");
            let redirected = render_query_result(
                &result,
                options(QueryFormat::Markdown, false, OutputTarget::Stream),
            )
            .expect("redirected Markdown");
            let terminal = render_query_result(
                &result,
                options(QueryFormat::Markdown, false, OutputTarget::Terminal),
            )
            .expect("terminal Markdown");

            assert!(redirected.contains('\u{1b}'), "{view:?}: {redirected:?}");
            assert!(!terminal.contains('\u{1b}'), "{view:?}: {terminal:?}");
            assert!(terminal.contains("ris�c"));
        }
    }

    const fn options(format: QueryFormat, color: bool, target: OutputTarget) -> RenderOptions {
        RenderOptions {
            format,
            pretty: true,
            preserve_anchors: false,
            color,
            target,
        }
    }

    fn strip_ansi(value: &str) -> String {
        let mut output = String::with_capacity(value.len());
        let bytes = value.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'[') {
                index += 2;
                while index < bytes.len() {
                    let byte = bytes[index];
                    index += 1;
                    if byte.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                let character = value[index..].chars().next().expect("UTF-8 character");
                output.push(character);
                index += character.len_utf8();
            }
        }
        output
    }
}
