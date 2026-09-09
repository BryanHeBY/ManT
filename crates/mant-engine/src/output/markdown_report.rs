//! `CommonMark` presentation of query reports, separate from document encoding.

#[cfg(test)]
mod tests;

use mant_ir::DOCUMENT_ROOT_ID;
use mant_protocol::{ExcerptSelection, OutlineNode, QueryExcerpt, QueryOutline};

use mant_codec::encode::{
    MarkdownFragmentOptions, heading, heading_has_local_link,
    render_heading_fragment as render_heading, render_sections_fragment as render_sections,
    render_tldr, section_headings_have_local_links,
};
use mant_codec::encode::{
    commonmark_code_span as code_span, escape_commonmark as escape_text, html_anchor,
    render_blocks_fragment as render_blocks,
};

/// Render a complete query outline as a nested `CommonMark` list.
#[must_use]
pub fn render_outline_markdown(outline: &QueryOutline) -> String {
    let label = document_label(
        outline.display_title.as_deref().unwrap_or(&outline.label),
        outline
            .meta
            .as_ref()
            .and_then(|meta| meta.manual_section.as_deref()),
    );
    let mut blocks = vec![heading(1, &format!("{label} outline"))];
    if let Some(message) = super::outline_empty_message(outline) {
        blocks.push(message);
    } else if !outline.nodes.is_empty() {
        blocks.push(outline_list(&outline.nodes, 0));
    }
    let references = mant_protocol::render_reference_inventory(&outline.references);
    if !references.is_empty() {
        // Reference facts are plain data, never executable Markdown links.
        let fence = "`".repeat(
            references
                .split(|c| c != '`')
                .map(str::len)
                .max()
                .unwrap_or(0)
                .max(2)
                + 1,
        );
        blocks.push(format!("{fence}text\n{references}\n{fence}"));
    }
    blocks.join("\n\n").trim_end().to_owned()
}

/// Render selected query nodes with their outline context.
#[must_use]
pub fn render_excerpt_markdown(excerpt: &QueryExcerpt) -> String {
    render_excerpt_markdown_with_options(excerpt, MarkdownFragmentOptions::default())
}

/// Render selected nodes using explicit presentation-only options.
#[must_use]
pub fn render_excerpt_markdown_with_options(
    excerpt: &QueryExcerpt,
    mut options: MarkdownFragmentOptions,
) -> String {
    let heading_links = excerpt.selections.iter().any(|selection| match selection {
        ExcerptSelection::DocumentRoot { heading, .. } => {
            heading.as_ref().is_some_and(heading_has_local_link)
        }
        ExcerptSelection::DocumentSection { section, .. } => {
            section_headings_have_local_links(std::slice::from_ref(section))
        }
        ExcerptSelection::DocumentEntry { .. } | ExcerptSelection::Tldr { .. } => false,
    });
    if heading_links {
        options.preserve_anchors = true;
    }
    let label = document_label(
        excerpt.display_title.as_deref().unwrap_or(&excerpt.label),
        excerpt
            .meta
            .as_ref()
            .and_then(|meta| meta.manual_section.as_deref()),
    );
    let mut output = vec![heading(1, &label)];
    if !excerpt.semantics_complete {
        output.push("Semantic entries are incomplete; use search to inspect unclassified or rejected content.".to_owned());
    }
    for (index, selection) in excerpt.selections.iter().enumerate() {
        if index > 0 {
            output.push("---".to_owned());
        }
        output.push(selection_context(selection));
        match selection {
            ExcerptSelection::Tldr { document, .. } => output.extend(render_tldr(document)),
            ExcerptSelection::DocumentRoot {
                heading, blocks, ..
            } => {
                if let Some(heading) = heading {
                    if options.preserve_anchors {
                        output.push(html_anchor(DOCUMENT_ROOT_ID));
                    }
                    output.push(render_heading(2, heading, options));
                }
                output.extend(render_blocks(blocks, options));
            }
            ExcerptSelection::DocumentSection { section, .. } => {
                render_sections(&mut output, std::slice::from_ref(section), 2, options);
            }
            ExcerptSelection::DocumentEntry { entry, .. } => {
                output.extend(render_blocks(std::slice::from_ref(entry), options));
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
    let trail = selection.outline();
    let breadcrumb = trail
        .ancestors
        .iter()
        .map(|ancestor| escape_text(&ancestor.title))
        .chain(std::iter::once(escape_text(trail.title())))
        .collect::<Vec<_>>()
        .join(" → ");
    format!("*Outline {}: {breadcrumb}*", code_span(trail.path()))
}

fn document_label(title: &str, section: Option<&str>) -> String {
    section.map_or_else(|| title.to_owned(), |section| format!("{title}({section})"))
}
