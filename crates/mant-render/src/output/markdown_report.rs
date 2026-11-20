//! `CommonMark` presentation of query reports, separate from document encoding.

#[cfg(test)]
mod tests;

use crate::presentation::navigation_title;
use mant_protocol::{ExcerptSelection, OutlineNode, QueryExcerpt, QueryOutline};

use mant_codec::encode::{
    MarkdownFragmentOptions, ScopedMarkdownFragments, heading, heading_has_local_link, render_tldr,
    section_headings_have_local_links,
};
use mant_codec::encode::{commonmark_code_span as code_span, escape_commonmark as escape_text};

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
    let references = crate::presentation::render_reference_inventory(&outline.references);
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
    options: MarkdownFragmentOptions,
) -> String {
    render_selected_excerpt_markdown(
        excerpt,
        ScopedMarkdownFragments::for_report(options, &excerpt.diagnostics),
    )
}

/// Assemble a report inside the trusted source selection operation.
///
/// As with the individual fragment encoders, borrowed local destinations must
/// come from the same projection. For an independent DTO, use
/// [`render_excerpt_markdown_with_options`].
#[must_use]
pub fn render_selected_excerpt_markdown(
    excerpt: &QueryExcerpt,
    fragments: ScopedMarkdownFragments<'_>,
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
    let fragments = if heading_links {
        fragments.with_anchors()
    } else {
        fragments
    };
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
                output.extend(fragments.overview(heading.as_ref(), blocks, 2));
            }
            ExcerptSelection::DocumentSection { section, .. } => {
                fragments.sections(&mut output, std::slice::from_ref(section), 2);
            }
            ExcerptSelection::DocumentEntry { entry, .. } => {
                output.extend(fragments.blocks(std::slice::from_ref(entry), None));
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
            escape_text(&navigation_title(node.title()))
        ));
        let children = outline_list(node.children(), depth + 1);
        if !children.is_empty() {
            lines.push(children);
        }
    }
    lines.join("\n")
}

fn selection_context(selection: &ExcerptSelection) -> String {
    use std::fmt::Write;

    let trail = selection.outline();
    let breadcrumb = trail
        .ancestors
        .iter()
        .map(|ancestor| escape_text(&navigation_title(&ancestor.title)))
        .chain(std::iter::once(escape_text(&navigation_title(
            trail.title(),
        ))))
        .collect::<Vec<_>>()
        .join(" → ");
    // CommonMark cannot close emphasis after whitespace, even NBSP.
    // Encode the trailing characters outside the delimiter so document
    // assembly and Markdown line trimming cannot consume them either.
    let label = format!("Outline {}: {breadcrumb}", code_span(trail.path()));
    let emphasis = label.trim_end();
    let mut context = format!("*{emphasis}*");
    for ch in label[emphasis.len()..].chars() {
        write!(context, "&#{};", u32::from(ch)).expect("writing to a String");
    }
    context
}

fn document_label(title: &str, section: Option<&str>) -> String {
    section.map_or_else(|| title.to_owned(), |section| format!("{title}({section})"))
}
