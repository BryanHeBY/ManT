//! Conservative proof of exact Flow fragment origins in a Markdown render.
//!
//! A source claim is made only for an untransformed, whole-root renderer
//! placement. Escapes, entity decoding, style delimiters and split placements
//! retain visible text but fall back to `render-derived` rather than guessing.

use std::collections::HashMap;
use std::ops::Range;

use mant_codec::encode::MarkdownArtifact;
use mant_ir::{
    Block, ContentBlockStep, ContentInlineRoot, ContentLocation, ContentRootKey, Document, Inline,
    Section,
};

#[derive(Default)]
pub(super) struct FlowOrigins {
    records: Vec<Origin>,
    roots: HashMap<ContentRootKey, (ContentLocation, String)>,
}

struct Origin {
    markdown: Range<usize>,
    root: ContentRootKey,
}

impl FlowOrigins {
    pub(super) fn new(document: Option<&Document>, artifact: &MarkdownArtifact<'_>) -> Self {
        let Some(document) = document else {
            return Self {
                records: Vec::new(),
                roots: HashMap::new(),
            };
        };
        let Some(flow) = document.flow() else {
            return Self {
                records: Vec::new(),
                roots: HashMap::new(),
            };
        };
        let mut locations = HashMap::new();
        let content = document.content();
        if let Some(heading) = &flow.heading {
            insert_inline_location(
                &heading.content,
                ContentLocation::DocumentHeading { path: Vec::new() },
                content,
                &mut locations,
            );
        }
        collect_blocks(&flow.blocks, &[], &[], content, &mut locations);
        collect_sections(&flow.sections, &[], content, &mut locations);
        let mut roots = HashMap::new();
        let mut records = Vec::new();
        for (root, markdown) in artifact.rendered_root_ranges() {
            let Some(location) = locations.get(&root) else {
                continue;
            };
            if let std::collections::hash_map::Entry::Vacant(entry) = roots.entry(root) {
                let Some(text) = content.root_logical_text(root) else {
                    continue;
                };
                entry.insert((location.clone(), text));
            }
            let Some((_, text)) = roots.get(&root) else {
                continue;
            };
            if artifact.text().get(markdown.clone()) == Some(text.as_str()) {
                records.push(Origin { markdown, root });
            }
        }
        records.sort_by_key(|record| record.markdown.start);
        Self { records, roots }
    }

    pub(super) fn locate(
        &self,
        markdown: Range<usize>,
        visible: &str,
    ) -> Option<(ContentLocation, u64, u64)> {
        let index = self
            .records
            .partition_point(|record| record.markdown.start <= markdown.start);
        let record = self.records.get(index.checked_sub(1)?)?;
        if markdown.end > record.markdown.end {
            return None;
        }
        let (location, text) = self.roots.get(&record.root)?;
        let start = markdown.start.checked_sub(record.markdown.start)?;
        let end = markdown.end.checked_sub(record.markdown.start)?;
        if text.get(start..end)? != visible {
            return None;
        }
        Some((
            location.clone(),
            u64::try_from(start).ok()?,
            u64::try_from(end).ok()?,
        ))
    }
}

fn insert_inline_location(
    inlines: &[Inline],
    location: ContentLocation,
    content: mant_ir::ContentContext<'_>,
    locations: &mut HashMap<ContentRootKey, ContentLocation>,
) {
    if let Some(root) = first_root(inlines, content) {
        locations.entry(root).or_insert(location);
    }
}

fn first_root(inlines: &[Inline], content: mant_ir::ContentContext<'_>) -> Option<ContentRootKey> {
    for inline in inlines {
        let root = match inline {
            Inline::Text { content: value } | Inline::Code { content: value } => {
                content.atom(value.atom).map(|atom| atom.root)
            }
            Inline::LineBreak { atom } => content.atom(*atom).map(|atom| atom.root),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => first_root(children, content),
            Inline::Anchor { .. } => None,
        };
        if root.is_some() {
            return root;
        }
    }
    None
}

fn collect_sections(
    sections: &[Section],
    parents: &[u32],
    content: mant_ir::ContentContext<'_>,
    locations: &mut HashMap<ContentRootKey, ContentLocation>,
) {
    for (index, section) in sections.iter().enumerate() {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        let mut path = parents.to_vec();
        path.push(index);
        insert_inline_location(
            &section.heading.content,
            ContentLocation::SectionHeading {
                sections: path.clone(),
                path: Vec::new(),
            },
            content,
            locations,
        );
        collect_blocks(&section.blocks, &path, &[], content, locations);
        collect_sections(&section.children, &path, content, locations);
    }
}

fn collect_blocks(
    blocks: &[Block],
    sections: &[u32],
    parents: &[ContentBlockStep],
    content: mant_ir::ContentContext<'_>,
    locations: &mut HashMap<ContentRootKey, ContentLocation>,
) {
    for (index, block) in blocks.iter().enumerate() {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        let mut path = parents.to_vec();
        path.push(ContentBlockStep::Block { index });
        match block {
            Block::Paragraph { children, .. }
            | Block::Preformatted { children, .. }
            | Block::FixedDisplay { children, .. } => {
                insert_inline_location(
                    children,
                    ContentLocation::Content {
                        sections: sections.to_vec(),
                        blocks: path,
                        root: ContentInlineRoot::Inlines,
                        path: Vec::new(),
                    },
                    content,
                    locations,
                );
            }
            Block::List { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    let Ok(item_index) = u32::try_from(item_index) else {
                        continue;
                    };
                    let mut nested = path.clone();
                    nested.push(ContentBlockStep::ListItem { index: item_index });
                    collect_blocks(&item.blocks, sections, &nested, content, locations);
                }
            }
            Block::DefinitionList { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    let Ok(item_index) = u32::try_from(item_index) else {
                        continue;
                    };
                    for (term_index, term) in item.terms.iter().enumerate() {
                        let Ok(term_index) = u32::try_from(term_index) else {
                            continue;
                        };
                        insert_inline_location(
                            term,
                            ContentLocation::Content {
                                sections: sections.to_vec(),
                                blocks: path.clone(),
                                root: ContentInlineRoot::DefinitionTerm {
                                    item_index,
                                    term_index,
                                },
                                path: Vec::new(),
                            },
                            content,
                            locations,
                        );
                    }
                    let mut nested = path.clone();
                    nested.push(ContentBlockStep::DefinitionItem { index: item_index });
                    collect_blocks(&item.description, sections, &nested, content, locations);
                }
            }
            Block::Table { rows, .. } => {
                for (row, table_row) in rows.iter().enumerate() {
                    let Ok(row) = u32::try_from(row) else {
                        continue;
                    };
                    for (column, cell) in table_row.cells.iter().enumerate() {
                        let Ok(column) = u32::try_from(column) else {
                            continue;
                        };
                        let mut nested = path.clone();
                        nested.push(ContentBlockStep::TableCell { row, column });
                        collect_blocks(&cell.blocks, sections, &nested, content, locations);
                    }
                }
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}
