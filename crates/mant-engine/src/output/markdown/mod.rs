//! Renders the native query contract as deterministic portable `CommonMark`.

mod blocks;
mod inline;

use std::ops::Range;

use mant_ir::{
    Block, DefinitionCase, DefinitionRole, LayoutHint, NodeId, OutlinePath, Section, SourceSpan,
    TldrCommandPart, TldrDocument, TldrOrigin,
};
use mant_protocol::{ExcerptSelection, OutlineNode, QueryExcerpt, QueryOutline};

use self::{
    blocks::{RenderedBlocks, render_blocks, render_blocks_with_entries},
    inline::{code_span, escape_text},
};
use crate::{ResolvedContent, projection::DOCUMENT_ROOT_ID};

/// Markdown serialization controls that do not alter the query IR.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarkdownOptions {
    /// Emit stable raw-HTML destinations and links for document-local references.
    pub preserve_anchors: bool,
}

impl MarkdownOptions {
    /// Addressable Markdown used by consumers of `mant.markdown/v1`.
    pub const ADDRESSABLE: Self = Self {
        preserve_anchors: true,
    };
}

/// Render a complete query as clean Markdown without a trailing newline.
#[must_use]
pub fn render_markdown(query: &ResolvedContent) -> String {
    render_markdown_with_options(query, MarkdownOptions::default())
}

/// Render a complete query using explicit presentation-only options.
#[must_use]
pub fn render_markdown_with_options(query: &ResolvedContent, options: MarkdownOptions) -> String {
    render_markdown_artifact(query, options).text
}

pub(crate) struct MarkdownArtifact {
    pub(crate) text: String,
    pub(crate) nodes: Vec<MarkdownNodeRange>,
}

#[derive(Clone)]
pub(crate) struct MarkdownNodeRange {
    pub(crate) range: Range<usize>,
    pub(crate) node: MarkdownNode,
}

#[derive(Clone)]
pub(crate) struct MarkdownSection {
    pub(crate) path: OutlinePath,
    pub(crate) id: NodeId,
    pub(crate) title: String,
}

#[derive(Clone)]
pub(crate) enum MarkdownNode {
    Tldr,
    DocumentRoot,
    DocumentSection {
        section: MarkdownSection,
        source: Option<SourceSpan>,
    },
    DocumentEntry {
        path: OutlinePath,
        id: NodeId,
        title: String,
        role: DefinitionRole,
        case: DefinitionCase,
        names: Vec<String>,
        section: Option<MarkdownSection>,
        source: Option<SourceSpan>,
    },
}

pub(crate) fn render_addressable_markdown(query: &ResolvedContent) -> MarkdownArtifact {
    render_markdown_artifact(query, MarkdownOptions::ADDRESSABLE)
}

fn render_markdown_artifact(query: &ResolvedContent, options: MarkdownOptions) -> MarkdownArtifact {
    let mut output = ArtifactBuilder::default();
    output.push(&heading(1, &query.label));

    if let Some(tldr) = &query.tldr {
        for (index, block) in render_tldr(tldr).into_iter().enumerate() {
            let range = output.push(&block);
            if index == 0 {
                output.begin_tldr(range.start);
            }
        }
        if query.document.is_some() {
            output.push("---");
        }
    }

    if let Some(document) = &query.document {
        if !document.blocks.is_empty() {
            let start = if options.preserve_anchors {
                output.push(&inline::html_anchor(DOCUMENT_ROOT_ID)).start
            } else {
                output.text.len()
            };
            output.begin_root(start);
            let rendered = render_blocks_with_entries(&document.blocks, options);
            output.push_scope(rendered, None, None);
        }
        render_artifact_sections(&mut output, &document.sections, &[], 2, options);
    }
    output.finish()
}

#[derive(Default)]
struct ArtifactBuilder {
    text: String,
    nodes: Vec<MarkdownNodeRange>,
    tldr: Option<usize>,
    root: Option<usize>,
    last_section: Option<usize>,
}

impl ArtifactBuilder {
    fn push(&mut self, block: &str) -> Range<usize> {
        if block.is_empty() {
            return self.text.len()..self.text.len();
        }
        if !self.text.is_empty() {
            self.text.push_str("\n\n");
        }
        let start = self.text.len();
        self.text.push_str(block);
        start..self.text.len()
    }

    fn begin_tldr(&mut self, start: usize) {
        self.tldr = Some(self.node(start, MarkdownNode::Tldr));
    }

    fn begin_root(&mut self, start: usize) {
        self.close_tldr(start);
        self.root = Some(self.node(start, MarkdownNode::DocumentRoot));
    }

    fn begin_section(
        &mut self,
        start: usize,
        section: MarkdownSection,
        source: Option<SourceSpan>,
    ) {
        self.close_tldr(start);
        if let Some(root) = self.root.take() {
            self.nodes[root].range.end = start;
        }
        if let Some(previous) = self.last_section {
            self.nodes[previous].range.end = start;
        }
        self.last_section =
            Some(self.node(start, MarkdownNode::DocumentSection { section, source }));
    }

    fn push_scope(
        &mut self,
        rendered: RenderedBlocks,
        section: Option<&MarkdownSection>,
        coordinates: Option<&[usize]>,
    ) {
        if rendered.text.is_empty() {
            return;
        }
        let block = self.push(&rendered.text);
        for entry in rendered.entries {
            let path = OutlinePath::entry(coordinates, entry.index)
                .expect("enumerated entry paths are one-based");
            self.nodes.push(MarkdownNodeRange {
                range: block.start + entry.start..block.start + entry.end,
                node: MarkdownNode::DocumentEntry {
                    path,
                    id: entry.identity.id,
                    title: entry.identity.names.join(", "),
                    role: entry.identity.role,
                    case: entry.identity.case,
                    names: entry.identity.names,
                    section: section.cloned(),
                    source: entry.source,
                },
            });
        }
    }

    fn node(&mut self, start: usize, node: MarkdownNode) -> usize {
        let index = self.nodes.len();
        self.nodes.push(MarkdownNodeRange {
            range: start..self.text.len(),
            node,
        });
        index
    }

    fn close_tldr(&mut self, end: usize) {
        if let Some(tldr) = self.tldr.take() {
            self.nodes[tldr].range.end = end;
        }
    }

    fn finish(mut self) -> MarkdownArtifact {
        let end = self.text.trim_end().len();
        self.text.truncate(end);
        self.close_tldr(end);
        if let Some(root) = self.root.take() {
            self.nodes[root].range.end = end;
        }
        if let Some(section) = self.last_section {
            self.nodes[section].range.end = end;
        }
        for node in &mut self.nodes {
            node.range.end = node.range.end.min(end);
        }
        MarkdownArtifact {
            text: self.text,
            nodes: self.nodes,
        }
    }
}

fn render_artifact_sections(
    output: &mut ArtifactBuilder,
    sections: &[Section],
    parent: &[usize],
    depth: usize,
    options: MarkdownOptions,
) {
    for (index, section) in sections.iter().enumerate() {
        let mut coordinates = parent.to_vec();
        coordinates.push(index + 1);
        let path =
            OutlinePath::section(&coordinates).expect("enumerated section paths are one-based");
        let rendered_heading = if options.preserve_anchors {
            format!(
                "{}\n\n{}",
                inline::html_anchor(&section.id),
                heading(depth, &section.title)
            )
        } else {
            heading(depth, &section.title)
        };
        let range = output.push(&rendered_heading);
        let reference = MarkdownSection {
            path,
            id: section.id.clone(),
            title: section.title.clone(),
        };
        output.begin_section(range.start, reference.clone(), section.source);
        output.push_scope(
            render_blocks_with_entries(&section.blocks, options),
            Some(&reference),
            Some(&coordinates),
        );
        render_artifact_sections(
            output,
            &section.children,
            &coordinates,
            depth.saturating_add(1),
            options,
        );
    }
}

/// Render a complete query outline as a nested `CommonMark` list.
#[must_use]
pub fn render_outline_markdown(outline: &QueryOutline) -> String {
    let label = document_label(
        &outline.label,
        outline
            .meta
            .as_ref()
            .and_then(|meta| meta.section.as_deref()),
    );
    let mut blocks = vec![heading(1, &format!("{label} outline"))];
    if !outline.nodes.is_empty() {
        blocks.push(outline_list(&outline.nodes, 0));
    }
    blocks.join("\n\n").trim_end().to_owned()
}

/// Render selected query nodes with their outline context.
#[must_use]
pub fn render_excerpt_markdown(excerpt: &QueryExcerpt) -> String {
    render_excerpt_markdown_with_options(excerpt, MarkdownOptions::default())
}

/// Render selected nodes using explicit presentation-only options.
#[must_use]
pub fn render_excerpt_markdown_with_options(
    excerpt: &QueryExcerpt,
    options: MarkdownOptions,
) -> String {
    let label = document_label(
        &excerpt.label,
        excerpt
            .meta
            .as_ref()
            .and_then(|meta| meta.section.as_deref()),
    );
    let mut output = vec![heading(1, &label)];
    for (index, selection) in excerpt.selections.iter().enumerate() {
        if index > 0 {
            output.push("---".to_owned());
        }
        output.push(selection_context(selection));
        match selection {
            ExcerptSelection::Tldr { document, .. } => output.extend(render_tldr(document)),
            ExcerptSelection::DocumentRoot { blocks, .. } => {
                output.extend(render_blocks(blocks, options));
            }
            ExcerptSelection::DocumentSection { section, .. } => {
                render_sections(&mut output, std::slice::from_ref(section), 2, options);
            }
            ExcerptSelection::DocumentEntry { entry, .. } => {
                output.extend(render_blocks(
                    &[Block::DefinitionList {
                        items: vec![entry.clone()],
                        compact: true,
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                    options,
                ));
            }
        }
    }
    output
        .into_iter()
        .filter(|block| !block.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim_end()
        .to_owned()
}

fn outline_list(nodes: &[OutlineNode], depth: usize) -> String {
    let mut lines = Vec::new();
    for node in nodes {
        lines.push(format!(
            "{}- {} ({}) {}",
            "  ".repeat(depth),
            code_span(node.path()),
            code_span(node.id()),
            escape_text(node.title())
        ));
        let children = outline_list(node.children(), depth + 1);
        if !children.is_empty() {
            lines.push(children);
        }
    }
    lines.join("\n")
}

fn selection_context(selection: &ExcerptSelection) -> String {
    match selection {
        ExcerptSelection::Tldr { path, title, .. }
        | ExcerptSelection::DocumentRoot { path, title, .. } => {
            format!("*Outline {}: {}*", code_span(path), escape_text(title))
        }
        ExcerptSelection::DocumentSection {
            path,
            title,
            breadcrumbs,
            ..
        } => {
            let breadcrumb = breadcrumbs
                .iter()
                .map(|ancestor| escape_text(&ancestor.title))
                .chain(std::iter::once(escape_text(title)))
                .collect::<Vec<_>>()
                .join(" → ");
            format!("*Outline {}: {breadcrumb}*", code_span(path))
        }
        ExcerptSelection::DocumentEntry {
            path,
            title,
            breadcrumbs,
            ..
        } => {
            let breadcrumb = breadcrumbs
                .iter()
                .map(|ancestor| escape_text(&ancestor.title))
                .chain(std::iter::once(escape_text(title)))
                .collect::<Vec<_>>()
                .join(" → ");
            format!("*Outline {}: {breadcrumb}*", code_span(path))
        }
    }
}

fn render_sections(
    output: &mut Vec<String>,
    sections: &[Section],
    depth: usize,
    options: MarkdownOptions,
) {
    for section in sections {
        if options.preserve_anchors {
            output.push(format!(
                "{}\n\n{}",
                inline::html_anchor(&section.id),
                heading(depth, &section.title)
            ));
        } else {
            output.push(heading(depth, &section.title));
        }
        output.extend(render_blocks(&section.blocks, options));
        render_sections(output, &section.children, depth.saturating_add(1), options);
    }
}

fn render_tldr(page: &TldrDocument) -> Vec<String> {
    let mut output = vec![heading(2, "TLDR")];
    output.extend(
        page.description
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| escape_text(line.trim())),
    );

    if let Some(value) = page.more_information.as_deref() {
        output.push(render_more_information(value));
    }
    if !page.examples.is_empty() {
        output.push(heading(3, "Examples"));
        for example in &page.examples {
            if !example.description.trim().is_empty() {
                output.push(format!("**{}**", escape_text(example.description.trim())));
            }
            if !example.command.is_empty() {
                let resolved = example
                    .command_parts
                    .iter()
                    .map(|part| match part {
                        TldrCommandPart::Text { value }
                        | TldrCommandPart::Placeholder { value } => value.as_str(),
                    })
                    .collect::<String>();
                output.push(inline::fenced_code(
                    if resolved.is_empty() {
                        &example.command
                    } else {
                        &resolved
                    },
                    Some("sh"),
                ));
            }
        }
    }
    if page.origin == TldrOrigin::TldrPages {
        output.push(format!(
            "*tldr-pages · CC BY 4.0 · {} · {}*",
            escape_text(&page.platform),
            escape_text(&page.language)
        ));
    }
    output
}

fn render_more_information(value: &str) -> String {
    let value = value.trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        let (url, punctuation) = value
            .strip_suffix('.')
            .map_or((value, ""), |url| (url, "."));
        if !url.chars().any(char::is_whitespace) && !url.contains(['<', '>']) {
            return format!("**More information:** <{url}>{punctuation}");
        }
    }
    format!("**More information:** {}", escape_text(value))
}

fn heading(depth: usize, title: &str) -> String {
    format!("{} {}", "#".repeat(depth.clamp(1, 6)), escape_text(title))
}

fn document_label(label: &str, section: Option<&str>) -> String {
    section.map_or_else(|| label.to_owned(), |section| format!("{label}({section})"))
}

#[cfg(test)]
mod tests;
