//! Collects target occurrences from the lowered renderer-neutral IR.

use mant_ir::{Block, Document, FragmentAlias, Inline, Section};

use super::{ObservedRole, ObservedTarget, ObservedTargets, SectionPosition};

struct ObservationLocation {
    role: ObservedRole,
    container: &'static str,
    section: SectionPosition,
    owner_source_line: u32,
    owner_path: String,
    ir_path: String,
}

pub(super) fn observed_targets(document: &Document) -> ObservedTargets {
    let mut observed = ObservedTargets::default();
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
        &document.blocks,
        &mut observed,
        root_position,
        "document",
        "content",
        None,
        0,
    );
    let mut next_section_ordinal = 0;
    for (index, section) in document.sections.iter().enumerate() {
        collect_section(
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
        &section.blocks,
        observed,
        section_position,
        path,
        "content",
        None,
        0,
    );
    for (index, child) in section.children.iter().enumerate() {
        collect_section(
            child,
            observed,
            &format!("{path}/section[{index}]"),
            next_section_ordinal,
        );
    }
}

fn collect_blocks(
    blocks: &[Block],
    observed: &mut ObservedTargets,
    section: SectionPosition,
    parent_path: &str,
    owner_container: &'static str,
    owner_path: Option<&str>,
    owner_source_line: u32,
) {
    for (block_index, block) in blocks.iter().enumerate() {
        let path = format!("{parent_path}/block[{block_index}]");
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                let container = if owner_container == "content" {
                    match block {
                        Block::Paragraph { .. } => "paragraph",
                        Block::Preformatted { .. } => "preformatted",
                        _ => unreachable!(),
                    }
                } else {
                    owner_container
                };
                let block_source_line = block_source_line(block);
                collect_inlines(
                    children,
                    observed,
                    section,
                    &path,
                    container,
                    owner_path.unwrap_or(&path),
                    if owner_source_line == 0 {
                        block_source_line
                    } else {
                        owner_source_line
                    },
                );
            }
            Block::List { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    let item_path = format!("{path}/item[{item_index}]");
                    collect_blocks(
                        &item.blocks,
                        observed,
                        section,
                        &item_path,
                        "list-item",
                        Some(&item_path),
                        first_block_source_line(&item.blocks),
                    );
                }
            }
            Block::DefinitionList { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    let item_path = format!("{path}/definition[{item_index}]");
                    if let Some(identity) = &item.identity {
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
                            term,
                            observed,
                            section,
                            &format!("{item_path}/term[{term_index}]"),
                            "definition",
                            &item_path,
                            first_block_source_line(&item.description),
                        );
                    }
                    collect_blocks(
                        &item.description,
                        observed,
                        section,
                        &format!("{item_path}/description"),
                        "definition",
                        Some(&item_path),
                        first_block_source_line(&item.description),
                    );
                }
            }
            Block::Table { rows, .. } => {
                collect_table_cells(rows, observed, section, &path);
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn collect_table_cells(
    rows: &[mant_ir::TableRow],
    observed: &mut ObservedTargets,
    section: SectionPosition,
    path: &str,
) {
    for (row_index, row) in rows.iter().enumerate() {
        for (cell_index, cell) in row.cells.iter().enumerate() {
            let cell_path = format!("{path}/row[{row_index}]/cell[{cell_index}]");
            collect_blocks(
                &cell.blocks,
                observed,
                section,
                &cell_path,
                "table-cell",
                Some(&cell_path),
                first_block_source_line(&cell.blocks),
            );
        }
    }
}

fn collect_inlines(
    nodes: &[Inline],
    observed: &mut ObservedTargets,
    section: SectionPosition,
    parent_path: &str,
    container: &'static str,
    owner_path: &str,
    owner_source_line: u32,
) {
    for (index, node) in nodes.iter().enumerate() {
        let path = format!("{parent_path}/inline[{index}]");
        match node {
            Inline::Anchor {
                id,
                fragment_aliases,
                owner_source,
            } => {
                record_observed(
                    observed,
                    id.as_str(),
                    fragment_aliases,
                    ObservationLocation {
                        role: ObservedRole::Anchor,
                        container,
                        section,
                        owner_source_line: owner_source
                            .map_or(owner_source_line, |source| source.line),
                        owner_path: owner_path.to_owned(),
                        ir_path: path,
                    },
                );
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                if let Inline::Link {
                    target: mant_ir::LinkTarget::Section { id },
                    ..
                } = node
                {
                    observed.section_links.insert(id.to_string());
                }
                collect_inlines(
                    children,
                    observed,
                    section,
                    &path,
                    container,
                    owner_path,
                    owner_source_line,
                );
            }
            Inline::Text { .. } | Inline::Code { .. } | Inline::LineBreak => {}
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
