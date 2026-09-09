//! Identify definitions in explicit preparation, counting and allocation passes.
//! Source-neutral topology is stable before identities are allocated.

mod binding;
mod context;
mod diagnostics;
mod evidence;
mod groups;
mod identity;
mod normalize;
pub(crate) use normalize::normalize_definition_nesting;
mod preparation;
mod recognized;
mod syntax;

use context::DefinitionContext;
pub(crate) use diagnostics::manual_discovery_diagnostics;
pub(crate) use evidence::{NativeHeadEvidence, NativeHeadRole};
pub(crate) use identity::document_id_slug;
use identity::{document_anchor_ids, identify_item, identify_list_item};
use mant_ir::{Block, Section};
pub(crate) use mant_ir::{ContentEntry, content_entries, content_entry_locations};
pub(crate) use recognized::RecognizedName;
use std::collections::{HashMap, HashSet};
pub(crate) use syntax::{
    environment_variable_alias, option_names_from_terms, option_occurrences_from_terms,
    option_prefix, slash_option_forms,
};
#[cfg(test)]
use syntax::{is_value_name, option_names};

/// Annotate reliably recognizable command-line options and return every
/// inline anchor that the navigation resolver must retain.
pub(crate) fn identify_definitions(
    blocks: &mut Vec<Block>,
    sections: &mut [Section],
    reserved_targets: &HashSet<String>,
    document_name: Option<&str>,
) -> HashSet<String> {
    identify_definitions_with_evidence(
        blocks,
        sections,
        reserved_targets,
        document_name,
        &NativeHeadEvidence::default(),
    )
}

pub(crate) fn identify_definitions_with_evidence(
    blocks: &mut Vec<Block>,
    sections: &mut [Section],
    reserved_targets: &HashSet<String>,
    document_name: Option<&str>,
    evidence: &NativeHeadEvidence,
) -> HashSet<String> {
    let root_context = document_name.map_or(DefinitionContext::Generic, |name| {
        let name = name.to_ascii_lowercase();
        if name.ends_with("_config") || name.ends_with("-config") {
            DefinitionContext::ConfigurationKeys
        } else {
            DefinitionContext::Generic
        }
    });
    let prepared = preparation::prepare(blocks, sections, root_context, evidence);

    let used = document_anchor_ids(blocks, sections);
    let mut discovery = DefinitionDiscovery {
        retained: used.clone(),
        used,
        reserved: reserved_targets,
        preferred_counts: &prepared.preferred_counts,
        plans: prepared.plans.into_iter(),
    };
    discovery.identify_blocks(blocks);
    discovery.identify_sections(sections);
    assert!(
        discovery.plans.next().is_none(),
        "all prepared owners were allocated"
    );
    discovery.retained
}

struct DefinitionDiscovery<'a> {
    plans: std::vec::IntoIter<preparation::PreparedDefinition>,
    used: HashSet<String>,
    reserved: &'a HashSet<String>,
    retained: HashSet<String>,
    preferred_counts: &'a HashMap<String, usize>,
}

impl DefinitionDiscovery<'_> {
    fn identify_sections(&mut self, sections: &mut [Section]) {
        for section in sections {
            self.identify_blocks(&mut section.blocks);
            self.identify_sections(&mut section.children);
        }
    }

    fn identify_blocks(&mut self, blocks: &mut [Block]) {
        for block in blocks {
            match block {
                Block::List { items, .. } => {
                    for item in items {
                        identify_list_item(
                            item,
                            &mut self.used,
                            self.reserved,
                            &mut self.retained,
                            self.preferred_counts,
                        );
                        self.identify_blocks(&mut item.blocks);
                    }
                }
                Block::DefinitionList { items, .. } => {
                    for item in items {
                        let plan = self
                            .plans
                            .next()
                            .expect("every final definition was prepared")
                            .for_item(item);
                        identify_item(
                            item,
                            plan,
                            &mut self.used,
                            self.reserved,
                            &mut self.retained,
                            self.preferred_counts,
                        );
                        self.identify_blocks(&mut item.description);
                    }
                }
                Block::Table { rows, .. } => {
                    for row in rows {
                        for cell in &mut row.cells {
                            self.identify_blocks(&mut cell.blocks);
                        }
                    }
                }
                Block::Paragraph { .. }
                | Block::Preformatted { .. }
                | Block::Equation { .. }
                | Block::VerticalSpace { .. }
                | Block::ThematicBreak { .. }
                | Block::Unsupported { .. } => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;
