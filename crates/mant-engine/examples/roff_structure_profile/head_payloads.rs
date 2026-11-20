//! Observe formatter payloads executed within their definition term owner.

use libmandoc_rs::{Node, NodeKind, TableCellKind, TableRowKind};
use mant_ir::{DefinitionItem, Document, Inline, inline_plain_text, visit::Visit};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::{
    native_topology::{MdocContainerKind, mdoc_list_items, mdoc_list_topology_kind},
    ordinal_labels::{SourceContext, SourceDestination, description::source_evidence},
};

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
    list_words: Vec<String>,
    equations: Vec<String>,
}

impl TermPayload {
    fn is_empty(&self) -> bool {
        self.table_words.is_empty() && self.list_words.is_empty() && self.equations.is_empty()
    }
}

impl HeadPayloads {
    pub(super) fn collect(root: &Node, source: &SourceContext) -> Self {
        let mut result = Self::default();
        result.walk(root, source);
        result
    }

    fn walk(&mut self, node: &Node, source: &SourceContext) {
        if source.destination(node) != Some(SourceDestination::Blocks) {
            return;
        }
        if node.kind == NodeKind::Block
            && node.macro_token.as_deref() == Some("It")
            && let Some(head) = node.children.iter().find(|part| {
                part.kind == NodeKind::Head
                    && source.destination(part) == Some(SourceDestination::Term)
            })
        {
            let mut payload = TermPayload::default();
            self.collect_term(head, source, &mut payload);
            self.owners
                .entry((node.line, node.column))
                .or_default()
                .push(payload);
        }
        for child in &node.children {
            self.walk(child, source);
        }
    }

    fn collect_term(&mut self, node: &Node, source: &SourceContext, payload: &mut TermPayload) {
        if source.destination(node) != Some(SourceDestination::Term) {
            return;
        }
        if node.kind == NodeKind::Block
            && let Some(kind) = mdoc_list_topology_kind(node)
        {
            let words = if kind == MdocContainerKind::Table {
                &mut payload.table_words
            } else {
                &mut payload.list_words
            };
            for item in mdoc_list_items(node) {
                for part in item
                    .children
                    .iter()
                    .filter(|part| source.destination(part) == Some(SourceDestination::Term))
                {
                    collect_part_words(part, source, words);
                }
            }
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
                self.collect_term(child, source, payload);
            }
        }
    }

    pub(super) fn contains_equation(&self, node: &Node) -> bool {
        self.equation_ids.contains(&node.id)
    }

    pub(super) fn compare(&self, document: &Document, violations: &mut Vec<String>) {
        if self.owners.values().flatten().all(TermPayload::is_empty) {
            return;
        }
        let mut observed = Terms::default();
        observed.visit_document(document);
        for (&(line, column), groups) in &self.owners {
            for (occurrence, expected) in groups
                .iter()
                .enumerate()
                .filter(|(_, payload)| !payload.is_empty())
            {
                let terms = observed
                    .0
                    .get(&(line, column))
                    .and_then(|terms| terms.get(occurrence));
                let terms = terms.copied().unwrap_or_default();
                for (kind, witness, words) in [
                    ("table", "cell", &expected.table_words),
                    ("list", "item", &expected.list_words),
                ] {
                    if let Some(missing) = missing_term_word(words, terms) {
                        violations.push(format!(
                            "definition-head-{kind} at {line}:{column}, occurrence {}: missing {witness} word {missing:?} in term",
                            occurrence + 1
                        ));
                    }
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

fn collect_part_words(part: &Node, source: &SourceContext, words: &mut Vec<String>) {
    // Nested blocks supply their own obligations. A mixed part cannot
    // prove an exact sequence by concatenating its lexical descendants.
    if part
        .children
        .iter()
        .any(|child| matches!(child.kind, NodeKind::Table | NodeKind::Block))
    {
        return;
    }
    let (_, word) = source_evidence(&part.children, part, false, source, true);
    if let Some(word) = word
        && !word.has(mant_codec::RoffWordEffect::Unclassified)
    {
        words.extend(word.text.split_whitespace().map(str::to_owned));
    }
}

fn missing_term_word<'words>(
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
