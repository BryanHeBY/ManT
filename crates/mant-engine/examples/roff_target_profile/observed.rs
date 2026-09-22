//! Collects target occurrences from the lowered renderer-neutral IR.

use mant_ir::{Block, ContentContext, Document, FragmentAlias, Inline, Provenance, Section};

use super::{ObservedRole, ObservedTarget, ObservedTargets, SectionPosition};

struct ObservationLocation {
    role: ObservedRole,
    container: &'static str,
    section: SectionPosition,
    owner_source_line: u32,
    owner_path: String,
    ir_path: String,
}

#[derive(Clone, Copy)]
struct OwnerContext<'a> {
    container: &'static str,
    path: Option<&'a str>,
    source_line: u32,
}

pub(super) fn observed_targets(document: &Document) -> ObservedTargets {
    let mut observed = ObservedTargets::default();
    let content = document.content();
    let root_position = SectionPosition {
        ordinal: 0,
        source_line: 0,
    };
    record_observed(
        &mut observed,
        "document",
        &document.fragment_aliases,
        ObservationLocation {
            role: ObservedRole::Document,
            container: "document",
            section: root_position,
            owner_source_line: 0,
            owner_path: "document".to_owned(),
            ir_path: "document".to_owned(),
        },
    );
    collect_blocks(
        content,
        &document.blocks,
        &mut observed,
        root_position,
        "document",
        OwnerContext {
            container: "content",
            path: None,
            source_line: 0,
        },
    );
    let mut next_section_ordinal = 0;
    for (index, section) in document.sections.iter().enumerate() {
        collect_section(
            content,
            section,
            &mut observed,
            &format!("section[{index}]"),
            &mut next_section_ordinal,
        );
    }
    observed
}

fn record_observed(
    observed: &mut ObservedTargets,
    identity: &str,
    fragment_aliases: &[FragmentAlias],
    location: ObservationLocation,
) {
    let fragment_aliases = fragment_aliases
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    observed.identities.insert(identity.to_owned());
    observed.fragments.extend(fragment_aliases.iter().cloned());
    match location.role {
        ObservedRole::Section => {
            observed.sections.insert(identity.to_owned());
        }
        ObservedRole::Entry => {
            observed.entries.insert(identity.to_owned());
        }
        ObservedRole::Anchor => {
            observed.anchors.insert(identity.to_owned());
        }
        ObservedRole::Document => {}
    }
    observed.occurrences.push(ObservedTarget {
        identity: identity.to_owned(),
        fragment_aliases,
        role: location.role,
        container: location.container,
        section_ordinal: location.section.ordinal,
        section_source_line: location.section.source_line,
        owner_source_line: location.owner_source_line,
        owner_path: location.owner_path,
        ir_path: location.ir_path,
    });
}

fn collect_section(
    content: ContentContext<'_>,
    section: &Section,
    observed: &mut ObservedTargets,
    path: &str,
    next_section_ordinal: &mut usize,
) {
    *next_section_ordinal += 1;
    let section_position = SectionPosition {
        ordinal: *next_section_ordinal,
        source_line: section.source.map_or(0, |source| source.line),
    };
    record_observed(
        observed,
        section.id.as_str(),
        &section.fragment_aliases,
        ObservationLocation {
            role: ObservedRole::Section,
            container: "section",
            section: section_position,
            owner_source_line: section_position.source_line,
            owner_path: path.to_owned(),
            ir_path: path.to_owned(),
        },
    );
    observed.identities.insert(section.id.to_string());
    collect_blocks(
        content,
        &section.blocks,
        observed,
        section_position,
        path,
        OwnerContext {
            container: "content",
            path: None,
            source_line: 0,
        },
    );
    for (index, child) in section.children.iter().enumerate() {
        collect_section(
            content,
            child,
            observed,
            &format!("{path}/section[{index}]"),
            next_section_ordinal,
        );
    }
}

#[allow(clippy::too_many_lines)]
fn collect_blocks(
    content: ContentContext<'_>,
    blocks: &[Block],
    observed: &mut ObservedTargets,
    section: SectionPosition,
    parent_path: &str,
    owner: OwnerContext<'_>,
) {
    for (block_index, block) in blocks.iter().enumerate() {
        let path = format!("{parent_path}/block[{block_index}]");
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                let container = if owner.container == "content" {
                    match block {
                        Block::Paragraph { .. } => "paragraph",
                        Block::Preformatted { .. } => "preformatted",
                        _ => unreachable!(),
                    }
                } else {
                    owner.container
                };
                let block_source_line = block_source_line(block);
                collect_inlines(
                    content,
                    children,
                    observed,
                    section,
                    &path,
                    OwnerContext {
                        container,
                        path: Some(owner.path.unwrap_or(&path)),
                        source_line: if owner.source_line == 0 {
                            block_source_line
                        } else {
                            owner.source_line
                        },
                    },
                );
            }
            Block::List { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    let item_path = format!("{path}/item[{item_index}]");
                    collect_blocks(
                        content,
                        &item.blocks,
                        observed,
                        section,
                        &item_path,
                        OwnerContext {
                            container: "list-item",
                            path: Some(&item_path),
                            source_line: first_block_source_line(&item.blocks),
                        },
                    );
                }
            }
            Block::DefinitionList { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    let item_path = format!("{path}/definition[{item_index}]");
                    if let Some(identity) = &item.entry {
                        record_observed(
                            observed,
                            identity.id.as_str(),
                            &[],
                            ObservationLocation {
                                role: ObservedRole::Entry,
                                container: "definition",
                                section,
                                owner_source_line: first_block_source_line(&item.description),
                                owner_path: item_path.clone(),
                                ir_path: item_path.clone(),
                            },
                        );
                    }
                    for (term_index, term) in item.terms.iter().enumerate() {
                        collect_inlines(
                            content,
                            term,
                            observed,
                            section,
                            &format!("{item_path}/term[{term_index}]"),
                            OwnerContext {
                                container: "definition",
                                path: Some(&item_path),
                                source_line: first_block_source_line(&item.description),
                            },
                        );
                    }
                    collect_blocks(
                        content,
                        &item.description,
                        observed,
                        section,
                        &format!("{item_path}/description"),
                        OwnerContext {
                            container: "definition",
                            path: Some(&item_path),
                            source_line: first_block_source_line(&item.description),
                        },
                    );
                }
            }
            Block::Table { rows, .. } => {
                collect_table_cells(content, rows, observed, section, &path);
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn collect_table_cells(
    content: ContentContext<'_>,
    rows: &[mant_ir::TableRow],
    observed: &mut ObservedTargets,
    section: SectionPosition,
    path: &str,
) {
    for (row_index, row) in rows.iter().enumerate() {
        for (cell_index, cell) in row.cells.iter().enumerate() {
            let cell_path = format!("{path}/row[{row_index}]/cell[{cell_index}]");
            collect_blocks(
                content,
                &cell.blocks,
                observed,
                section,
                &cell_path,
                OwnerContext {
                    container: "table-cell",
                    path: Some(&cell_path),
                    source_line: first_block_source_line(&cell.blocks),
                },
            );
        }
    }
}

fn collect_inlines(
    content: ContentContext<'_>,
    nodes: &[Inline],
    observed: &mut ObservedTargets,
    section: SectionPosition,
    parent_path: &str,
    owner: OwnerContext<'_>,
) {
    for (index, node) in nodes.iter().enumerate() {
        let path = format!("{parent_path}/inline[{index}]");
        match node {
            Inline::Anchor {
                id,
                fragment_aliases,
                point,
            } => {
                let source = match content
                    .point(*point)
                    .expect("valid profile point")
                    .provenance
                {
                    Provenance::Authored { span } => Some(span),
                    Provenance::Generated { trigger } => trigger,
                    Provenance::Unknown => None,
                };
                record_observed(
                    observed,
                    id.as_str(),
                    fragment_aliases,
                    ObservationLocation {
                        role: ObservedRole::Anchor,
                        container: owner.container,
                        section,
                        owner_source_line: source.map_or(owner.source_line, |source| source.line),
                        owner_path: owner.path.unwrap_or(parent_path).to_owned(),
                        ir_path: path,
                    },
                );
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                if let Inline::Link { occurrence, .. } = node
                    && let mant_ir::LinkTarget::Section { id } = &content
                        .occurrence(*occurrence)
                        .expect("valid profile occurrence")
                        .target
                {
                    observed.section_links.insert(id.to_string());
                }
                collect_inlines(content, children, observed, section, &path, owner);
            }
            Inline::Text { .. } | Inline::Code { .. } | Inline::LineBreak { .. } => {}
        }
    }
}

fn block_source_line(block: &Block) -> u32 {
    match block {
        Block::Paragraph { source, .. }
        | Block::Preformatted { source, .. }
        | Block::List { source, .. }
        | Block::DefinitionList { source, .. }
        | Block::Table { source, .. }
        | Block::Equation { source, .. }
        | Block::Unsupported { source, .. } => source.map_or(0, |source| source.line),
        Block::VerticalSpace { source, .. } | Block::ThematicBreak { source, .. } => {
            source.map_or(0, |source| source.line)
        }
    }
}

fn first_block_source_line(blocks: &[Block]) -> u32 {
    blocks
        .iter()
        .map(block_source_line)
        .find(|line| *line > 0)
        .unwrap_or_default()
}
