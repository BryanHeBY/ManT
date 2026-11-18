//! Compare readable table cells with the definition term that owns the span.

use libmandoc_rs::{Node, NodeKind, TableCellKind, TableRowKind};
use mant_ir::{DefinitionItem, Document, inline_plain_text, visit::Visit};
use std::collections::{BTreeMap, HashMap, HashSet};

type Location = (u32, u32);

#[derive(Default)]
struct Terms<'a>(HashMap<Location, Vec<&'a [mant_ir::DefinitionTerm]>>);

impl<'a> Visit<'a> for Terms<'a> {
    fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
        if let Some(source) = item.source {
            self.0
                .entry((source.line, source.column))
                .or_default()
                .push(&item.terms);
        }
        mant_ir::visit::walk_definition_item(self, item);
    }
}

#[derive(Default)]
pub(super) struct HeadTables {
    pub(super) row_ids: HashSet<u32>,
    owners: BTreeMap<Location, Vec<Vec<String>>>,
}

impl HeadTables {
    pub(super) fn collect(root: &Node) -> Self {
        let mut result = Self::default();
        result.walk(root, false);
        result
    }

    fn walk(&mut self, node: &Node, definitions: bool) {
        if node.flags.no_print {
            return;
        }
        let definitions = if node.kind == NodeKind::Block
            && let Some(kind) = super::native_topology::mdoc_list_topology_kind(node)
        {
            kind == super::native_topology::MdocContainerKind::Definition
        } else {
            definitions
        };
        if definitions && node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("It")
        {
            let mut rows = Vec::new();
            for head in node
                .children
                .iter()
                .filter(|part| part.kind == NodeKind::Head)
            {
                self.collect_rows(head, &mut rows);
            }
            self.owners
                .entry((node.line, node.column))
                .or_default()
                .push(rows);
        }
        for child in &node.children {
            self.walk(child, definitions);
        }
    }

    fn collect_rows(&mut self, node: &Node, words: &mut Vec<String>) {
        if node.flags.no_print {
            return;
        }
        if node.kind == NodeKind::Table {
            self.row_ids.insert(node.id);
            if node.table_row_kind == Some(TableRowKind::Data) {
                // tbl_term.c supplies these exact plain cells to term_word.
                // Nontrivial escape and device processing is outside this
                // independent lexical witness.
                words.extend(
                    node.table_cells
                        .iter()
                        .filter(|cell| {
                            cell.kind == TableCellKind::Text && !cell.vertical_continuation
                        })
                        .filter_map(|cell| cell.decoder_text())
                        .filter(|value| {
                            value.is_ascii()
                                && !value.contains('\\')
                                && value.bytes().all(|byte| {
                                    byte.is_ascii_graphic() || byte.is_ascii_whitespace()
                                })
                        })
                        .flat_map(str::split_ascii_whitespace)
                        .map(str::to_owned),
                );
            }
        } else {
            for child in &node.children {
                self.collect_rows(child, words);
            }
        }
    }

    pub(super) fn compare(&self, document: &Document, violations: &mut Vec<String>) {
        if self.row_ids.is_empty() {
            return;
        }
        let mut observed = Terms::default();
        observed.visit_document(document);
        for (&(line, column), groups) in &self.owners {
            for (occurrence, expected) in groups
                .iter()
                .enumerate()
                .filter(|(_, words)| !words.is_empty())
            {
                let terms = observed
                    .0
                    .get(&(line, column))
                    .and_then(|terms| terms.get(occurrence));
                let text = terms
                    .iter()
                    .flat_map(|terms| terms.iter())
                    .map(|term| inline_plain_text(term))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut tokens = text.split_ascii_whitespace();
                if let Some(missing) = expected
                    .iter()
                    .find(|word| !tokens.by_ref().any(|candidate| candidate == word.as_str()))
                {
                    violations.push(format!(
                        "definition-head-table at {line}:{column}, occurrence {}: missing cell word {missing:?} in term",
                        occurrence + 1
                    ));
                }
            }
        }
    }
}
