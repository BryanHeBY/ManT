//! Collects target occurrences from the lowered renderer-neutral IR.

use mant_ir::{Block, Document, FragmentAlias, Inline, Section};

use super::{ObservedRole, ObservedTarget, ObservedTargets, SectionPosition};

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
        ObservedRole::Document,
        "document",
        root_position,
        "document".to_owned(),
    );
    collect_blocks(
        &document.blocks,
        &mut observed,
        root_position,
        "document",
        "content",
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
    role: ObservedRole,
    container: &'static str,
    section: SectionPosition,
    ir_path: String,
) {
    let fragment_aliases = fragment_aliases
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    observed.identities.insert(identity.to_owned());
    observed.fragments.extend(fragment_aliases.iter().cloned());
    match role {
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
        role,
        container,
        section_ordinal: section.ordinal,
        section_source_line: section.source_line,
        ir_path,
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
        ObservedRole::Section,
        "section",
        section_position,
        path.to_owned(),
    );
    observed.identities.insert(section.id.to_string());
    collect_blocks(&section.blocks, observed, section_position, path, "content");
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
                collect_inlines(children, observed, section, &path, container);
            }
            Block::List { items, .. } => {
                for (item_index, item) in items.iter().enumerate() {
                    collect_blocks(
                        &item.blocks,
                        observed,
                        section,
                        &format!("{path}/item[{item_index}]"),
                        "list-item",
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
                            ObservedRole::Entry,
                            "definition",
                            section,
                            item_path.clone(),
                        );
                    }
                    for (term_index, term) in item.terms.iter().enumerate() {
                        collect_inlines(
                            term,
                            observed,
                            section,
                            &format!("{item_path}/term[{term_index}]"),
                            "definition",
                        );
                    }
                    collect_blocks(
                        &item.description,
                        observed,
                        section,
                        &format!("{item_path}/description"),
                        "definition",
                    );
                }
            }
            Block::Table { rows, .. } => {
                for (row_index, row) in rows.iter().enumerate() {
                    for (cell_index, cell) in row.cells.iter().enumerate() {
                        collect_blocks(
                            &cell.blocks,
                            observed,
                            section,
                            &format!("{path}/row[{row_index}]/cell[{cell_index}]"),
                            "table-cell",
                        );
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

fn collect_inlines(
    nodes: &[Inline],
    observed: &mut ObservedTargets,
    section: SectionPosition,
    parent_path: &str,
    container: &'static str,
) {
    for (index, node) in nodes.iter().enumerate() {
        let path = format!("{parent_path}/inline[{index}]");
        match node {
            Inline::Anchor {
                id,
                fragment_aliases,
            } => {
                record_observed(
                    observed,
                    id.as_str(),
                    fragment_aliases,
                    ObservedRole::Anchor,
                    container,
                    section,
                    path,
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
                collect_inlines(children, observed, section, &path, container);
            }
            Inline::Text { .. } | Inline::Code { .. } | Inline::LineBreak => {}
        }
    }
}
