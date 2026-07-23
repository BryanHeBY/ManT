//! Renders query, outline, and excerpt contracts as unstyled semantic text.

use mant_ast::{
    Block, DefinitionItem, ExcerptSelection, Inline, ListItem, ListKind, OutlineNode, QueryBundle,
    QueryExcerpt, QueryOutline, Section, TableCell, TldrCommandPart, TldrDocument,
};

/// Render a complete query without Markdown or terminal escape sequences.
#[must_use]
pub fn render_query_text(query: &QueryBundle) -> String {
    render_query_body(query, true)
}

/// Render the manual as `man(1)`-faithful plain text.
///
/// Identical to [`render_query_text`] except the prepended tldr block is
/// omitted, so the output stays a faithful, noise-free subset of the manual
/// page (no page furniture, overstrike, or hyphenation — those never enter
/// the document model because the source is parsed directly).
#[must_use]
pub fn render_query_man(query: &QueryBundle) -> String {
    render_query_body(query, false)
}

fn render_query_body(query: &QueryBundle, include_tldr: bool) -> String {
    let section = query
        .manual
        .as_ref()
        .and_then(|manual| manual.meta.section.as_deref())
        .or(query.section.as_deref());
    let mut parts = vec![document_label(&query.topic, section)];
    if include_tldr && let Some(tldr) = &query.tldr {
        parts.push(render_tldr_text(tldr));
    }
    if let Some(manual) = &query.manual {
        parts.push(render_sections(&manual.sections, 0));
    }
    join_parts(parts)
}

/// Render a complete query outline as a copyable Unicode tree.
#[must_use]
pub fn render_outline_text(outline: &QueryOutline) -> String {
    let mut lines = vec![document_label(
        &outline.topic,
        outline.manual_section.as_deref(),
    )];
    render_outline_nodes(&outline.nodes, "", &mut lines);
    lines.join("\n").trim_end().to_owned()
}

/// Render selected query nodes as unstyled text with outline context.
#[must_use]
pub fn render_excerpt_text(excerpt: &QueryExcerpt) -> String {
    let mut parts = vec![document_label(
        &excerpt.topic,
        excerpt.manual_section.as_deref(),
    )];
    for selection in &excerpt.selections {
        parts.push(render_selection(selection));
    }
    join_parts(parts)
}

fn render_outline_nodes(nodes: &[OutlineNode], prefix: &str, output: &mut Vec<String>) {
    for (index, node) in nodes.iter().enumerate() {
        let last = index + 1 == nodes.len();
        let connector = if last { "└─" } else { "├─" };
        output.push(format!(
            "{prefix}{connector} {} [{}] {}",
            node.path(),
            node.id(),
            node.title()
        ));
        let child_prefix = format!("{prefix}{}", if last { "  " } else { "│ " });
        render_outline_nodes(node.children(), &child_prefix, output);
    }
}

fn render_selection(selection: &ExcerptSelection) -> String {
    match selection {
        ExcerptSelection::Tldr { document, .. } => render_tldr_text(document),
        ExcerptSelection::ManualSection {
            path,
            title,
            breadcrumbs,
            section,
            ..
        } => {
            let mut parts = Vec::new();
            if !breadcrumbs.is_empty() {
                let breadcrumb = breadcrumbs
                    .iter()
                    .map(|ancestor| ancestor.title.as_str())
                    .chain(std::iter::once(title.as_str()))
                    .collect::<Vec<_>>()
                    .join(" > ");
                parts.push(format!("Outline {path}: {breadcrumb}"));
            }
            parts.push(render_section(section, 0));
            join_parts(parts)
        }
        ExcerptSelection::ManualEntry {
            path,
            title,
            breadcrumbs,
            entry,
            ..
        } => {
            let breadcrumb = breadcrumbs
                .iter()
                .map(|ancestor| ancestor.title.as_str())
                .chain(std::iter::once(title.as_str()))
                .collect::<Vec<_>>()
                .join(" > ");
            join_parts(vec![
                format!("Outline {path}: {breadcrumb}"),
                render_definitions(std::slice::from_ref(entry), true, 0),
            ])
        }
    }
}

fn render_tldr_text(tldr: &TldrDocument) -> String {
    let mut lines = vec!["TLDR".to_owned()];
    lines.extend(tldr.description.iter().map(|line| line.trim().to_owned()));
    if let Some(information) = &tldr.more_information {
        lines.push(format!("More information: {}", information.trim()));
    }
    for example in &tldr.examples {
        if !example.description.trim().is_empty() {
            lines.push(example.description.trim().to_owned());
        }
        let command = example
            .command_parts
            .iter()
            .map(|part| match part {
                TldrCommandPart::Text { value } | TldrCommandPart::Placeholder { value } => {
                    value.as_str()
                }
            })
            .collect::<String>();
        lines.push(if command.is_empty() {
            example.command.clone()
        } else {
            command
        });
    }
    lines.join("\n\n")
}

fn render_sections(sections: &[Section], depth: usize) -> String {
    sections
        .iter()
        .map(|section| render_section(section, depth))
        .filter(|section| !section.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_section(section: &Section, depth: usize) -> String {
    let heading_indent = "  ".repeat(depth);
    let mut parts = vec![format!("{heading_indent}{}", section.title)];
    let blocks = render_blocks(&section.blocks, depth.saturating_mul(2));
    if !blocks.is_empty() {
        parts.push(blocks);
    }
    let children = render_sections(&section.children, depth + 1);
    if !children.is_empty() {
        parts.push(children);
    }
    join_parts(parts)
}

fn render_blocks(blocks: &[Block], base_indent: usize) -> String {
    // Blocks are separated by a single blank line by default. An explicit
    // vertical-space node *sets* the gap before the next block rather than
    // adding to it, so `.sp` and blank input lines are not double-counted
    // against the default paragraph separation (which previously turned one
    // requested blank line into several). Leading and trailing gaps are
    // dropped so a section never opens or closes with blank lines.
    let mut output = String::new();
    let mut has_content = false;
    let mut pending_blank_lines: Option<usize> = None;
    for block in blocks {
        if let Block::VerticalSpace { lines, .. } = block {
            if has_content {
                let requested = usize::from(*lines);
                pending_blank_lines = Some(pending_blank_lines.unwrap_or(0).max(requested));
            }
            continue;
        }
        let Some(text) = render_block(block, base_indent) else {
            continue;
        };
        if has_content {
            let blank_lines = pending_blank_lines.unwrap_or(1);
            output.push_str(&"\n".repeat(blank_lines + 1));
        }
        output.push_str(&text);
        has_content = true;
        pending_blank_lines = None;
    }
    output
}

fn render_block(block: &Block, base_indent: usize) -> Option<String> {
    let (value, layout_indent) = match block {
        Block::Paragraph {
            children, layout, ..
        }
        | Block::Preformatted {
            children, layout, ..
        } => (inline_text(children), usize::from(layout.indent_columns)),
        Block::List {
            kind,
            start,
            items,
            layout,
            ..
        } => (
            render_list(*kind, *start, items, base_indent),
            usize::from(layout.indent_columns),
        ),
        Block::DefinitionList {
            items,
            compact,
            layout,
            ..
        } => (
            render_definitions(items, *compact, base_indent),
            usize::from(layout.indent_columns),
        ),
        Block::Table { rows, layout, .. } => (
            rows.iter()
                .map(|row| {
                    row.cells
                        .iter()
                        .map(cell_text)
                        .collect::<Vec<_>>()
                        .join(" | ")
                })
                .collect::<Vec<_>>()
                .join("\n"),
            usize::from(layout.indent_columns),
        ),
        Block::Equation { value, layout, .. }
        | Block::Unsupported {
            text: value,
            layout,
            ..
        } => (value.clone(), usize::from(layout.indent_columns)),
        // Vertical space is handled as an inter-block separator in
        // `render_blocks`, never as a standalone rendered block.
        Block::VerticalSpace { .. } => return None,
    };
    let value = value.trim_matches('\n');
    (!value.trim().is_empty()).then(|| indent_lines(value, base_indent + layout_indent))
}

fn render_list(
    kind: ListKind,
    start: Option<u64>,
    items: &[ListItem],
    base_indent: usize,
) -> String {
    items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let marker = match kind {
                ListKind::Ordered => format!(
                    "{}. ",
                    start
                        .unwrap_or(1)
                        .saturating_add(u64::try_from(index).unwrap_or(u64::MAX))
                ),
                ListKind::Bullet => "- ".to_owned(),
                ListKind::Plain => String::new(),
            };
            prefix_text_item(&render_blocks(&item.blocks, base_indent), &marker)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_definitions(items: &[DefinitionItem], compact: bool, base_indent: usize) -> String {
    let rendered = items
        .iter()
        .filter_map(|item| {
            let terms = item
                .terms
                .iter()
                .map(|term| inline_text(term))
                .filter(|term| !term.trim().is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            let description = render_blocks(&item.description, base_indent);
            let value = match (terms.is_empty(), description.is_empty()) {
                (false, false) => {
                    if item.inline_term {
                        Some(format!("{terms} {description}"))
                    } else {
                        Some(format!("{terms}\n{}", indent_lines(&description, 2)))
                    }
                }
                (false, true) => Some(terms),
                (true, false) => Some(description),
                (true, true) => None,
            }?;
            Some((value, item.spacing_before_lines))
        })
        .collect::<Vec<_>>();

    let Some((first, rest)) = rendered.split_first() else {
        return String::new();
    };
    let mut output = first.0.clone();
    for (item, spacing_before_lines) in rest {
        let blank_lines = spacing_before_lines.unwrap_or(u16::from(!compact));
        output.push_str(&"\n".repeat(usize::from(blank_lines) + 1));
        output.push_str(item);
    }
    output
}

fn cell_text(cell: &TableCell) -> String {
    render_blocks(&cell.blocks, 0).replace('\n', " ")
}

fn inline_text(children: &[Inline]) -> String {
    let mut output = String::new();
    for child in children {
        match child {
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => output.push_str(&inline_text(children)),
            Inline::Anchor { .. } => {}
            Inline::LineBreak => output.push('\n'),
        }
    }
    output
}

fn prefix_text_item(content: &str, marker: &str) -> Option<String> {
    if content.trim().is_empty() {
        return None;
    }
    let continuation = " ".repeat(marker.chars().count());
    let mut lines = content.lines();
    let mut output = format!("{marker}{}", lines.next()?);
    for line in lines {
        output.push('\n');
        output.push_str(&continuation);
        output.push_str(line);
    }
    Some(output)
}

fn indent_lines(value: &str, columns: usize) -> String {
    if columns == 0 {
        return value.to_owned();
    }
    let prefix = " ".repeat(columns);
    value
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn document_label(topic: &str, section: Option<&str>) -> String {
    section.map_or_else(|| topic.to_owned(), |section| format!("{topic}({section})"))
}

fn join_parts(parts: Vec<String>) -> String {
    parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim_end()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use mant_ast::{
        Block, DocumentMeta, DocumentSchema, DocumentSource, Inline, LayoutHint, MantDocument,
        Producer, QueryBundle, QuerySchema, Section, SourceFormat, TldrDocument,
    };

    use super::{render_excerpt_text, render_outline_text, render_query_man, render_query_text};
    use crate::{build_outline, select_excerpt};

    fn query() -> QueryBundle {
        QueryBundle {
            schema: QuerySchema::V2,
            topic: "demo".to_owned(),
            section: None,
            manual: Some(MantDocument {
                schema: DocumentSchema::V2,
                producer: Producer {
                    name: "test".to_owned(),
                    version: "1".to_owned(),
                    engine: None,
                },
                source: DocumentSource {
                    format: SourceFormat::Man,
                    path: None,
                    renderer: None,
                },
                meta: DocumentMeta {
                    section: Some("1".to_owned()),
                    ..DocumentMeta::default()
                },
                diagnostics: Vec::new(),
                sections: vec![Section {
                    id: "options-1".to_owned(),
                    title: "OPTIONS".to_owned(),
                    spacing_before_lines: 0,
                    blocks: vec![paragraph("parent details", true)],
                    children: vec![Section {
                        id: "common-2".to_owned(),
                        title: "Common options".to_owned(),
                        spacing_before_lines: 1,
                        blocks: vec![paragraph("child details", false)],
                        children: Vec::new(),
                        source: None,
                    }],
                    source: None,
                }],
            }),
            tldr: None,
        }
    }

    fn paragraph(value: &str, strong: bool) -> Block {
        let text = vec![Inline::Text {
            value: value.to_owned(),
        }];
        Block::Paragraph {
            children: if strong {
                vec![Inline::Strong { children: text }]
            } else {
                text
            },
            layout: LayoutHint::default(),
            source: None,
        }
    }

    #[test]
    fn renders_plain_queries_without_markup_and_uses_resolved_manual_sections() {
        let output = render_query_text(&query());

        assert!(output.starts_with("demo(1)\n\nOPTIONS"));
        assert!(output.contains("parent details"));
        assert!(output.contains("Common options"));
        assert!(!output.contains("**"));
    }

    #[test]
    fn renders_copyable_outline_trees_and_contextual_excerpts() {
        let query = query();
        let outline = build_outline(&query).expect("outline");
        assert_eq!(
            render_outline_text(&outline),
            "demo(1)\n└─ 1 [options-1] OPTIONS\n  └─ 1.1 [common-2] Common options"
        );

        let excerpt = select_excerpt(&query, &["1.1".to_owned()]).expect("excerpt");
        let output = render_excerpt_text(&excerpt);
        assert!(output.contains("Outline 1.1: OPTIONS > Common options"));
        assert!(output.contains("child details"));
        assert!(!output.contains("parent details"));
    }

    #[test]
    fn renders_tldr_as_zero_in_outlines_and_standalone_excerpts() {
        let mut query = query();
        query.tldr = Some(TldrDocument {
            title: "demo".to_owned(),
            description: vec!["A small demonstration.".to_owned()],
            more_information: None,
            examples: Vec::new(),
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "/cache/tldr/demo.md".to_owned(),
        });

        let outline = render_outline_text(&build_outline(&query).expect("combined outline"));
        assert!(outline.contains("├─ 0 [tldr] TLDR QUICK REFERENCE"));
        assert!(outline.contains("└─ 1 [options-1] OPTIONS"));

        let excerpt = select_excerpt(&query, &["tldr".to_owned()]).expect("tldr excerpt");
        assert_eq!(
            render_excerpt_text(&excerpt),
            "demo\n\nTLDR\n\nA small demonstration."
        );
    }

    #[test]
    fn man_format_renders_the_manual_but_omits_the_prepended_tldr() {
        let mut query = query();
        query.tldr = Some(TldrDocument {
            title: "demo".to_owned(),
            description: vec!["A small demonstration.".to_owned()],
            more_information: None,
            examples: Vec::new(),
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "/cache/tldr/demo.md".to_owned(),
        });

        let text = render_query_text(&query);
        let man = render_query_man(&query);

        // text keeps the tldr block; man drops it entirely.
        assert!(text.contains("TLDR"));
        assert!(text.contains("A small demonstration."));
        assert!(!man.contains("TLDR"));
        assert!(!man.contains("A small demonstration."));

        // man still renders the manual body verbatim, without markup.
        assert!(man.starts_with("demo(1)\n\nOPTIONS"));
        assert!(man.contains("parent details"));
        assert!(man.contains("Common options"));
        assert!(!man.contains("**"));
    }

    #[test]
    fn vertical_space_sets_the_gap_instead_of_stacking_blank_lines() {
        fn document_with(blocks: Vec<Block>) -> QueryBundle {
            QueryBundle {
                schema: QuerySchema::V2,
                topic: "demo".to_owned(),
                section: Some("1".to_owned()),
                manual: Some(MantDocument {
                    schema: DocumentSchema::V2,
                    producer: Producer {
                        name: "test".to_owned(),
                        version: "1".to_owned(),
                        engine: None,
                    },
                    source: DocumentSource {
                        format: SourceFormat::Man,
                        path: None,
                        renderer: None,
                    },
                    meta: DocumentMeta {
                        section: Some("1".to_owned()),
                        ..DocumentMeta::default()
                    },
                    diagnostics: Vec::new(),
                    sections: vec![Section {
                        id: "s-1".to_owned(),
                        title: "S".to_owned(),
                        spacing_before_lines: 0,
                        blocks,
                        children: Vec::new(),
                        source: None,
                    }],
                }),
                tldr: None,
            }
        }
        fn para(value: &str) -> Block {
            Block::Paragraph {
                children: vec![Inline::Text {
                    value: value.to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }
        }
        let vspace = |lines: u16| Block::VerticalSpace {
            lines,
            source: None,
        };

        // One vertical-space line yields exactly one blank line, not several.
        let one = render_query_text(&document_with(vec![
            para("first"),
            vspace(1),
            para("second"),
        ]));
        assert!(one.contains("first\n\nsecond"), "got: {one:?}");
        assert!(!one.contains("first\n\n\nsecond"), "got: {one:?}");

        // A larger explicit gap is preserved rather than collapsed.
        let wide = render_query_text(&document_with(vec![
            para("first"),
            vspace(2),
            para("second"),
        ]));
        assert!(wide.contains("first\n\n\nsecond"), "got: {wide:?}");

        // Leading and trailing vertical space never adds blank lines at the edges.
        let edges = render_query_text(&document_with(vec![vspace(2), para("only"), vspace(3)]));
        assert!(edges.ends_with("only"), "got: {edges:?}");
        assert!(edges.contains("S\n\nonly"), "got: {edges:?}");
    }
}
