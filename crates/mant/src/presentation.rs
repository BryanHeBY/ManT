//! Deterministic rendering of already materialized query views.

use std::fmt::Write as _;

use anstyle::{AnsiColor, Style};
use mant_engine::QueryViewResult;
use mant_ir::{Block, DefinitionRole, Inline, ResolvedContent, Section, SourceFormat};
use mant_protocol::{
    ExcerptSelection, OutlineNode, QueryExcerpt, QueryOutline, QuerySearch, SearchNode,
};
use serde::Serialize;

use crate::{arguments::QueryFormat, error::Failure};

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
        self.value.push_str(value);
    }

    fn styled(&mut self, role: TerminalRole, value: &str) {
        if !self.color || value.is_empty() {
            self.plain(value);
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
    if !color {
        return mant_engine::render_outline_text(outline);
    }
    let mut output = TerminalText::new(true);
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

fn render_terminal_excerpt(excerpt: &QueryExcerpt, color: bool) -> String {
    let plain = mant_engine::render_excerpt_text(excerpt);
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
    if !color {
        return mant_engine::render_search_text(search);
    }
    let label = document_label(
        &search.label,
        search
            .meta
            .as_ref()
            .and_then(|meta| meta.manual_section.as_deref()),
    );
    let mut output = TerminalText::new(true);
    if search.total == 0 {
        output.plain("No matches for \"");
        output.styled(TerminalRole::Match, &search.query.pattern);
        output.plain("\" in ");
        output.styled(TerminalRole::Document, &label);
        output.plain(".");
        return output.finish();
    }
    if search.matches.is_empty() {
        output.plain("No matches returned at offset ");
        output.styled(TerminalRole::Coordinate, &search.offset.to_string());
        output.plain(" for \"");
        output.styled(TerminalRole::Match, &search.query.pattern);
        output.plain("\" in ");
        output.styled(TerminalRole::Document, &label);
        output.plain(" (");
        output.styled(TerminalRole::Coordinate, &search.total.to_string());
        output.plain(" total).");
        return output.finish();
    }

    for (index, found) in search.matches.iter().enumerate() {
        if index > 0 {
            output.line();
            output.line();
        }
        output.styled(TerminalRole::Document, &label);
        output.plain(":");
        output.styled(
            TerminalRole::Coordinate,
            &format!(
                "{}:{}",
                found.markdown.start_line, found.markdown.start_column
            ),
        );
        output.styled(TerminalRole::TreeGuide, " [");
        output.styled(TerminalRole::Path, found.node.path());
        output.styled(TerminalRole::TreeGuide, "] ");
        output.styled(search_node_role(&found.node), found.node.title());
        output.line();
        if found.context.is_empty() {
            output.plain("  ");
            // The stable search contract identifies intersecting rendered
            // lines, while generated anchors can shift display columns. Mark
            // the complete matching line instead of implying a false exact
            // terminal range.
            output.styled(TerminalRole::Match, &found.preview);
        } else {
            for (line_index, line) in found.context.iter().enumerate() {
                if line_index > 0 {
                    output.line();
                }
                output.plain("  ");
                output.styled(
                    if line.matched {
                        TerminalRole::Match
                    } else {
                        TerminalRole::Muted
                    },
                    if line.matched { ">" } else { " " },
                );
                output.plain(" ");
                output.styled(TerminalRole::Coordinate, &line.line.to_string());
                output.plain(" ");
                if line.matched {
                    output.styled(TerminalRole::Match, &line.text);
                } else {
                    output.plain(&line.text);
                }
            }
        }
    }
    if let Some(next_offset) = search.next_offset {
        output.line();
        output.line();
        output.styled(TerminalRole::Coordinate, &search.total.to_string());
        output.plain(" total matches; continue with ");
        output.styled(TerminalRole::Heading, "--offset");
        output.plain(" ");
        output.styled(TerminalRole::Coordinate, &next_offset.to_string());
        output.plain(".");
    }
    output.finish()
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
        OutlineNode::DocumentEntry { role, .. } => definition_role(*role),
        OutlineNode::Tldr { .. }
        | OutlineNode::DocumentRoot { .. }
        | OutlineNode::DocumentSection { .. } => TerminalRole::Heading,
    }
}

const fn search_node_role(node: &SearchNode) -> TerminalRole {
    match node {
        SearchNode::DocumentEntry { role, .. } => definition_role(*role),
        SearchNode::Tldr { .. }
        | SearchNode::DocumentRoot { .. }
        | SearchNode::DocumentSection { .. } => TerminalRole::Heading,
    }
}

const fn definition_role(role: DefinitionRole) -> TerminalRole {
    match role {
        DefinitionRole::Option => TerminalRole::Option,
        DefinitionRole::Command => TerminalRole::Command,
        DefinitionRole::EnvironmentVariable => TerminalRole::Environment,
        DefinitionRole::Variable => TerminalRole::Variable,
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

pub(super) fn render_query_result(
    result: &QueryViewResult,
    format: QueryFormat,
    pretty: bool,
    preserve_anchors: bool,
    color: bool,
) -> Result<String, Failure> {
    match result {
        QueryViewResult::Full(query) => render_full_query(query, format, pretty, preserve_anchors),
        QueryViewResult::Outline(outline) => match format {
            QueryFormat::Markdown => Ok(mant_engine::render_outline_markdown(outline)),
            QueryFormat::Text => Ok(render_terminal_outline(outline, color)),
            QueryFormat::Man => Err(Failure::usage(
                "--format man applies only to full documents",
            )),
            QueryFormat::Json => {
                mant_engine::render_outline_json(outline, pretty).map_err(Failure::operational)
            }
        },
        QueryViewResult::Excerpt(excerpt) => {
            render_excerpt(excerpt, format, pretty, preserve_anchors, color)
        }
        QueryViewResult::Search(search) => match format {
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

fn render_excerpt(
    excerpt: &mant_protocol::QueryExcerpt,
    format: QueryFormat,
    pretty: bool,
    preserve_anchors: bool,
    color: bool,
) -> Result<String, Failure> {
    match format {
        QueryFormat::Markdown => Ok(mant_engine::render_excerpt_markdown_with_options(
            excerpt,
            mant_engine::MarkdownOptions { preserve_anchors },
        )),
        QueryFormat::Text => Ok(render_terminal_excerpt(excerpt, color)),
        QueryFormat::Man => Err(Failure::usage(
            "--format man applies only to full documents",
        )),
        QueryFormat::Json => {
            mant_engine::render_excerpt_json(excerpt, pretty).map_err(Failure::operational)
        }
    }
}

fn render_full_query(
    query: &ResolvedContent,
    format: QueryFormat,
    pretty: bool,
    preserve_anchors: bool,
) -> Result<String, Failure> {
    match format {
        QueryFormat::Markdown => Ok(mant_engine::render_markdown_with_options(
            query,
            mant_engine::MarkdownOptions { preserve_anchors },
        )),
        QueryFormat::Text => Ok(mant_engine::render_query_text(query)),
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
            Ok(mant_engine::render_query_man(query))
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
    use mant_protocol::{OutlineDetail, QueryView};

    use super::{QueryFormat, render_query_result};

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
                detail: OutlineDetail::Entries,
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
            let plain = render_query_result(&result, QueryFormat::Text, true, false, false)
                .expect("plain terminal text");
            let colored = render_query_result(&result, QueryFormat::Text, true, false, true)
                .expect("colored terminal text");
            assert!(colored.contains("\x1b["));
            assert_eq!(strip_ansi(&colored), plain);
        }
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
            let rendered = render_query_result(&result, format, true, false, true)
                .expect("deterministic output");
            assert!(!rendered.contains("\x1b["));
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
