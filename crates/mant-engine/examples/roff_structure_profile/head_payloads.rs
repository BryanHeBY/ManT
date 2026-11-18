//! Observe formatter payloads executed within their definition term owner.

use libmandoc_rs::{Node, NodeKind, TableCellKind, TableRowKind};
use mant_ir::{DefinitionItem, Document, Inline, inline_plain_text, visit::Visit};
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
pub(super) struct HeadPayloads {
    pub(super) row_ids: HashSet<u32>,
    equation_ids: HashSet<u32>,
    owners: BTreeMap<Location, Vec<TermPayload>>,
}

#[derive(Default)]
struct TermPayload {
    table_words: Vec<String>,
    equations: Vec<String>,
}

impl HeadPayloads {
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
            let mut payload = TermPayload::default();
            for head in node
                .children
                .iter()
                .filter(|part| part.kind == NodeKind::Head)
            {
                self.collect_term(head, &mut payload);
            }
            self.owners
                .entry((node.line, node.column))
                .or_default()
                .push(payload);
        }
        for child in &node.children {
            self.walk(child, definitions);
        }
    }

    fn collect_term(&mut self, node: &Node, payload: &mut TermPayload) {
        if node.flags.no_print {
            return;
        }
        if node.kind == NodeKind::Table {
            self.row_ids.insert(node.id);
            if node.table_row_kind == Some(TableRowKind::Data) {
                // tbl_term.c supplies these exact plain cells to term_word.
                // Nontrivial escape and device processing is outside this
                // independent lexical witness.
                payload.table_words.extend(
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
        } else if node.kind == NodeKind::Equation {
            self.equation_ids.insert(node.id);
            if let Some(value) = node
                .equation
                .as_ref()
                .map(libmandoc_rs::EquationBox::readable_text)
                && !value.is_empty()
            {
                payload
                    .equations
                    .push(super::native::equation_visible_text(&value));
            }
        } else {
            for child in &node.children {
                self.collect_term(child, payload);
            }
        }
    }

    pub(super) fn contains_equation(&self, node: &Node) -> bool {
        self.equation_ids.contains(&node.id)
    }

    pub(super) fn compare(&self, document: &Document, violations: &mut Vec<String>) {
        if self.row_ids.is_empty() && self.equation_ids.is_empty() {
            return;
        }
        let mut observed = Terms::default();
        observed.visit_document(document);
        for (&(line, column), groups) in &self.owners {
            for (occurrence, expected) in groups.iter().enumerate().filter(|(_, payload)| {
                !payload.table_words.is_empty() || !payload.equations.is_empty()
            }) {
                let terms = observed
                    .0
                    .get(&(line, column))
                    .and_then(|terms| terms.get(occurrence));
                let terms = terms.copied().unwrap_or_default();
                if let Some(missing) = missing_table_word(&expected.table_words, terms) {
                    violations.push(format!(
                        "definition-head-table at {line}:{column}, occurrence {}: missing cell word {missing:?} in term",
                        occurrence + 1
                    ));
                }
                if !expected.equations.is_empty() {
                    let mut observed = TermEquations::default();
                    for inline in terms.iter().flat_map(|term| &term.content) {
                        observed.visit_inline(inline);
                    }
                    if observed.0 != expected.equations {
                        violations.push(format!(
                            "definition-head-equation at {line}:{column}, occurrence {}: expected {:?} in term, observed {:?}",
                            occurrence + 1,
                            expected.equations,
                            observed.0,
                        ));
                    }
                }
            }
        }
    }
}

fn missing_table_word<'words>(
    words: &'words [String],
    terms: &[mant_ir::DefinitionTerm],
) -> Option<&'words str> {
    if words.is_empty() {
        return None;
    }
    let text = terms
        .iter()
        .map(|term| inline_plain_text(term))
        .collect::<Vec<_>>()
        .join("\n");
    let mut tokens = text.split_ascii_whitespace();
    words
        .iter()
        .find(|word| !tokens.by_ref().any(|candidate| candidate == word.as_str()))
        .map(String::as_str)
}

#[derive(Default)]
struct TermEquations<'ir>(Vec<&'ir str>);

impl<'ir> Visit<'ir> for TermEquations<'ir> {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Equation { value, .. } = inline {
            self.0.push(value);
        }
        mant_ir::visit::walk_inline(self, inline);
    }
}
