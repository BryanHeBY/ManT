//! Renders the native query contract as deterministic portable `CommonMark`.

mod blocks;
mod inline;

use mant_ast::{
    Block, ExcerptSelection, LayoutHint, OutlineNode, QueryBundle, QueryExcerpt, QueryOutline,
    Section, TldrCommandPart, TldrDocument,
};

use self::{
    blocks::render_blocks,
    inline::{code_span, escape_text},
};

/// Markdown serialization controls that do not alter the query AST.
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
pub fn render_markdown(query: &QueryBundle) -> String {
    render_markdown_with_options(query, MarkdownOptions::default())
}

/// Render a complete query using explicit presentation-only options.
#[must_use]
pub fn render_markdown_with_options(query: &QueryBundle, options: MarkdownOptions) -> String {
    let mut output = Vec::new();
    output.push(heading(1, &query.label));

    if let Some(tldr) = &query.tldr {
        output.extend(render_tldr(tldr));
        if query.document.is_some() {
            output.push("---".to_owned());
        }
    }

    if let Some(manual) = &query.document {
        render_sections(&mut output, &manual.sections, 2, options);
    }
    output
        .into_iter()
        .filter(|block| !block.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim_end()
        .to_owned()
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
        ExcerptSelection::Tldr { path, title, .. } => {
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
    output.push(format!(
        "*tldr-pages · CC BY 4.0 · {} · {}*",
        escape_text(&page.platform),
        escape_text(&page.language)
    ));
    output
}

pub(crate) use inline::html_anchor;

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
