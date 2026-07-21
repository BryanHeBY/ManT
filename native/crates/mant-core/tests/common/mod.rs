//! Shared helpers, section schedules, and assertion utilities consumed by
//! every distribution-specific test directory.

use std::collections::HashSet;

use mant_ast::{
    Block, Inline, MantDocument, OutlineNode, QueryBundle, QuerySchema, Section, SourceFormat,
};

pub const LS_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "AUTHOR",
    "REPORTING BUGS",
    "COPYRIGHT",
    "SEE ALSO",
];
pub const GIT_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "OPTIONS",
    "GIT COMMANDS",
    "HIGH-LEVEL COMMANDS (PORCELAIN)",
    "LOW-LEVEL COMMANDS (PLUMBING)",
    "GUIDES",
    "REPOSITORY, COMMAND AND FILE INTERFACES",
    "FILE FORMATS, PROTOCOLS AND OTHER DEVELOPER INTERFACES",
    "CONFIGURATION MECHANISM",
    "IDENTIFIER TERMINOLOGY",
    "SYMBOLIC IDENTIFIERS",
    "FILE/DIRECTORY STRUCTURE",
    "TERMINOLOGY",
    "ENVIRONMENT VARIABLES",
    "DISCUSSION",
    "SECURITY",
    "FURTHER DOCUMENTATION",
    "AUTHORS",
    "REPORTING BUGS",
    "SEE ALSO",
    "GIT",
    "NOTES",
];
pub const GCC_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "OPTIONS",
    "ENVIRONMENT",
    "BUGS",
    "FOOTNOTES",
    "SEE ALSO",
    "AUTHOR",
    "COPYRIGHT",
];
pub const CLANG_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "OPTIONS",
    "ENVIRONMENT",
    "BUGS",
    "SEE ALSO",
    "Author",
    "Copyright",
];
pub const TAR_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "NOTE",
    "DESCRIPTION",
    "OPTIONS",
    "RETURN VALUE",
    "SEE ALSO",
    "BUG REPORTS",
    "COPYRIGHT",
];
pub const DEBIAN_MT_GNU_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "BUG REPORTS",
    "COPYRIGHT",
];
pub const DEBIAN_GROFF_ME_SECTIONS: &[&str] = &[
    "Name",
    "Synopsis",
    "Description",
    "Files",
    "Notes",
    "See also",
];
pub const DEBIAN_GROFF_MAN_STYLE_SECTIONS: &[&str] = &[
    "Name",
    "Synopsis",
    "Description",
    "Options",
    "Files",
    "Notes",
    "Authors",
    "See also",
];

pub fn query_for_document(name: &str, document: &MantDocument) -> QueryBundle {
    let manual = document.clone();
    QueryBundle {
        schema: QuerySchema::V2,
        topic: name.to_owned(),
        section: manual.meta.section.clone(),
        manual: Some(manual),
        tldr: None,
    }
}

// ---------------------------------------------------------------------------
// Block / section traversal
// ---------------------------------------------------------------------------

pub fn collect_sections<'a>(sections: &'a [Section], output: &mut Vec<&'a Section>) {
    for section in sections {
        output.push(section);
        collect_sections(&section.children, output);
    }
}

pub fn section<'a>(document: &'a MantDocument, title: &str) -> &'a Section {
    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    sections
        .into_iter()
        .find(|section| section.title == title)
        .unwrap_or_else(|| panic!("missing section {title}"))
}

pub fn document_blocks(document: &MantDocument) -> Vec<&Block> {
    document_blocks_from_sections(&document.sections)
}

pub fn document_blocks_from_sections(sections: &[Section]) -> Vec<&Block> {
    let mut output = Vec::new();
    for section in sections {
        collect_blocks(&section.blocks, &mut output);
        output.extend(document_blocks_from_sections(&section.children));
    }
    output
}

fn collect_blocks<'a>(blocks: &'a [Block], output: &mut Vec<&'a Block>) {
    for block in blocks {
        output.push(block);
        match block {
            Block::List { items, .. } => {
                for item in items {
                    collect_blocks(&item.blocks, output);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    collect_blocks(&item.description, output);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    collect_blocks(&cell.blocks, output);
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Definition list helpers
// ---------------------------------------------------------------------------

pub fn definition_items(section: &Section) -> Vec<&mant_ast::DefinitionItem> {
    section
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items.iter()),
            _ => None,
        })
        .flatten()
        .collect()
}

pub fn nested_definition_items(section: &Section) -> Vec<&mant_ast::DefinitionItem> {
    document_blocks_from_sections(std::slice::from_ref(section))
        .into_iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items.iter()),
            _ => None,
        })
        .flatten()
        .collect()
}

pub fn semantic_definition_items(document: &MantDocument) -> Vec<&mant_ast::DefinitionItem> {
    document_blocks(document)
        .into_iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items.iter()),
            _ => None,
        })
        .flatten()
        .filter(|item| item.identity.is_some())
        .collect()
}

// ---------------------------------------------------------------------------
// Outline helpers
// ---------------------------------------------------------------------------

pub fn find_outline_entry<'a>(nodes: &'a [OutlineNode], name: &str) -> Option<&'a OutlineNode> {
    for node in nodes {
        if matches!(node, OutlineNode::ManualEntry { names, .. } if names.iter().any(|value| value == name))
        {
            return Some(node);
        }
        if let Some(found) = find_outline_entry(node.children(), name) {
            return Some(found);
        }
    }
    None
}

pub fn count_outline_entries(nodes: &[OutlineNode]) -> usize {
    nodes
        .iter()
        .map(|node| {
            usize::from(matches!(node, OutlineNode::ManualEntry { .. }))
                + count_outline_entries(node.children())
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Vertical spacing assertions
// ---------------------------------------------------------------------------

pub fn assert_no_duplicate_vertical_spacing(sections: &[Section], fixture: &str) {
    for section in sections {
        assert_block_spacing_is_normalized(&section.blocks, fixture, &section.title);
        assert_no_duplicate_vertical_spacing(&section.children, fixture);
    }
}

fn assert_block_spacing_is_normalized(blocks: &[Block], fixture: &str, section_name: &str) {
    for pair in blocks.windows(2) {
        if matches!(pair[0], Block::VerticalSpace { .. }) {
            assert_eq!(
                block_spacing_before(&pair[1]),
                0,
                "fixture {fixture} section {section_name} stores one roff gap twice",
            );
        }
    }
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    assert_block_spacing_is_normalized(&item.blocks, fixture, section_name);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    assert_block_spacing_is_normalized(&item.description, fixture, section_name);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    assert_block_spacing_is_normalized(&cell.blocks, fixture, section_name);
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn block_spacing_before(block: &Block) -> u16 {
    match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => layout.spacing_before_lines,
        Block::VerticalSpace { .. } => 0,
    }
}

// ---------------------------------------------------------------------------
// Markup / control-character leak assertions
// ---------------------------------------------------------------------------

pub fn assert_document_has_no_source_markup(name: &str, document: &MantDocument) {
    for block in document_blocks(document) {
        visit_block_inlines(block, &mut |inline| {
            let value = match inline {
                Inline::Text { value } | Inline::Code { value } => value,
                Inline::Strong { .. }
                | Inline::Emphasis { .. }
                | Inline::ExternalLink { .. }
                | Inline::EmailLink { .. }
                | Inline::ManualReference { .. }
                | Inline::SectionReference { .. }
                | Inline::Anchor { .. }
                | Inline::LineBreak => return,
            };
            assert!(
                !value.contains("\\f")
                    && !value.contains("\\(")
                    && !value.contains("\\*")
                    && !value.contains("<br")
                    && !value.contains("<b>")
                    && !value.contains("<i>")
                    && !value.contains(['\u{1d}', '\u{1e}', '\u{1f}']),
                "fixture {name} leaked source markup: {value:?}",
            );
        });
    }
}

pub fn assert_anchor_ids_are_clean(name: &str, document: &MantDocument) {
    for block in document_blocks(document) {
        visit_block_inlines(block, &mut |inline| {
            if let Inline::Anchor { id } = inline {
                assert!(
                    !id.contains(['\u{1d}', '\u{1e}', '\u{1f}']),
                    "{name} anchor ID {id:?} bytes {:02x?} leaks roff escape",
                    id.as_bytes(),
                );
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Preformatted block helpers
// ---------------------------------------------------------------------------

pub fn as_preformatted(block: &Block) -> Option<&[Inline]> {
    match block {
        Block::Preformatted { children, .. } => Some(children),
        _ => None,
    }
}

pub fn assert_preformatted(section: &Section, needle: &str, expected_indent: u16) {
    let (children, indent) = find_preformatted(&section.blocks, needle, 0)
        .unwrap_or_else(|| panic!("missing preformatted text {needle:?} in {}", section.title));
    assert!(inline_text(children).contains(needle));
    assert_eq!(indent, expected_indent);
}

fn find_preformatted<'a>(
    blocks: &'a [Block],
    needle: &str,
    base_indent: u16,
) -> Option<(&'a [Inline], u16)> {
    for block in blocks {
        match block {
            Block::Preformatted {
                children, layout, ..
            } if inline_text(children).contains(needle) => {
                return Some((children, base_indent + layout.indent_columns));
            }
            Block::List { items, layout, .. } => {
                for item in items {
                    if let Some(found) =
                        find_preformatted(&item.blocks, needle, base_indent + layout.indent_columns)
                    {
                        return Some(found);
                    }
                }
            }
            Block::DefinitionList { items, layout, .. } => {
                for item in items {
                    if let Some(found) = find_preformatted(
                        &item.description,
                        needle,
                        base_indent + layout.indent_columns + 4,
                    ) {
                        return Some(found);
                    }
                }
            }
            Block::Table { rows, layout, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    if let Some(found) =
                        find_preformatted(&cell.blocks, needle, base_indent + layout.indent_columns)
                    {
                        return Some(found);
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::Unsupported { .. } => {}
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Inline style helpers
// ---------------------------------------------------------------------------

pub fn contains_strong(children: &[Inline], expected: &str) -> bool {
    children.iter().any(|inline| match inline {
        Inline::Strong { children } => inline_text(children) == expected,
        Inline::Emphasis { children }
        | Inline::ExternalLink { children, .. }
        | Inline::EmailLink { children, .. }
        | Inline::ManualReference { children, .. }
        | Inline::SectionReference { children, .. } => contains_strong(children, expected),
        Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } | Inline::LineBreak => {
            false
        }
    })
}

pub fn contains_emphasis(children: &[Inline], expected: &str) -> bool {
    children.iter().any(|inline| match inline {
        Inline::Emphasis { children } => inline_text(children) == expected,
        Inline::Strong { children }
        | Inline::ExternalLink { children, .. }
        | Inline::EmailLink { children, .. }
        | Inline::ManualReference { children, .. }
        | Inline::SectionReference { children, .. } => contains_emphasis(children, expected),
        Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } | Inline::LineBreak => {
            false
        }
    })
}

pub fn count_line_breaks(children: &[Inline]) -> usize {
    children
        .iter()
        .map(|inline| match inline {
            Inline::LineBreak => 1,
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => count_line_breaks(children),
            Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } => 0,
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Text extraction
// ---------------------------------------------------------------------------

pub fn inline_text(children: &[Inline]) -> String {
    children
        .iter()
        .map(|inline| match inline {
            Inline::Text { value } | Inline::Code { value } => value.clone(),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => inline_text(children),
            Inline::Anchor { .. } => String::new(),
            Inline::LineBreak => "\n".to_owned(),
        })
        .collect()
}

pub fn block_slice_text(blocks: &[Block]) -> String {
    blocks.iter().map(block_text).collect::<Vec<_>>().join("\n")
}

fn block_text(block: &Block) -> String {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            inline_text(children)
        }
        Block::List { items, .. } => items
            .iter()
            .map(|item| block_slice_text(&item.blocks))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::DefinitionList { items, .. } => items
            .iter()
            .map(|item| {
                format!(
                    "{} {}",
                    item.terms
                        .iter()
                        .map(|term| inline_text(term))
                        .collect::<Vec<_>>()
                        .join(" "),
                    block_slice_text(&item.description),
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Table { rows, .. } => rows
            .iter()
            .flat_map(|row| &row.cells)
            .map(|cell| block_slice_text(&cell.blocks))
            .collect::<Vec<_>>()
            .join(" "),
        Block::Equation { value, .. } => value.clone(),
        Block::Unsupported { text, .. } => text.clone(),
        Block::VerticalSpace { .. } => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Inline visitor
// ---------------------------------------------------------------------------

pub fn visit_block_inlines(block: &Block, visitor: &mut impl FnMut(&Inline)) {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            visit_inlines(children, visitor);
        }
        Block::List { items, .. } => {
            for item in items {
                for block in &item.blocks {
                    visit_block_inlines(block, visitor);
                }
            }
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                for term in &item.terms {
                    visit_inlines(term, visitor);
                }
                for block in &item.description {
                    visit_block_inlines(block, visitor);
                }
            }
        }
        Block::Table { rows, .. } => {
            for cell in rows.iter().flat_map(|row| &row.cells) {
                for block in &cell.blocks {
                    visit_block_inlines(block, visitor);
                }
            }
        }
        Block::Equation { .. } | Block::VerticalSpace { .. } | Block::Unsupported { .. } => {}
    }
}

fn visit_inlines(children: &[Inline], visitor: &mut impl FnMut(&Inline)) {
    for inline in children {
        visitor(inline);
        match inline {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => visit_inlines(children, visitor),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Anchor { .. }
            | Inline::LineBreak => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Section-topology assertion (reusable pattern across distributions)
// ---------------------------------------------------------------------------

pub fn assert_section_topology(name: &str, document: &MantDocument, expected_titles: &[&str]) {
    assert_eq!(document.source.format, SourceFormat::Man, "fixture {name}");
    assert!(
        document
            .source
            .path
            .as_deref()
            .is_some_and(|path| path.contains(name)),
        "fixture {name} must retain its source location",
    );

    let section_titles: Vec<&str> = document
        .sections
        .iter()
        .map(|section| section.title.as_str())
        .collect();
    assert_eq!(section_titles, expected_titles, "fixture {name}");

    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    assert!(
        sections
            .iter()
            .all(|section| !section.blocks.is_empty() || !section.children.is_empty()),
        "fixture {name} contains an empty section",
    );
    let ids: HashSet<&str> = sections.iter().map(|section| section.id.as_str()).collect();
    assert_eq!(ids.len(), sections.len(), "fixture {name} section IDs");
}
