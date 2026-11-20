//! Prove the retained payload of structures executed in definition terms.

use libmandoc_rs::{DefinitionListStyle, Node, NodeKind, TableCellKind, TableRowKind};
use mant_codec::{RoffWordEffect, RoffWordEvidence};
use mant_ir::{DefinitionItem, DefinitionTerm, Document, Inline, visit::Visit};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::{
    native_topology::{MdocContainerKind, mdoc_list_items, mdoc_list_topology_kind},
    ordinal_labels::{
        self, OrdinalDisposition, SourceContext, SourceDestination,
        controls::{SourceControl, source_control},
        description::source_evidence,
    },
};

type Location = (u32, u32);

#[derive(Default)]
struct Terms<'a>(HashMap<Location, Vec<&'a [DefinitionTerm]>>);

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
    owners: BTreeMap<Location, Vec<TermPayload>>,
}

#[derive(Clone, Copy, Default)]
enum Origin {
    #[default]
    Term,
    List,
    Table,
    Display,
    Equation,
}

impl Origin {
    fn label(self) -> &'static str {
        match self {
            Self::Term => "text",
            Self::List => "list",
            Self::Table => "table",
            Self::Display => "literal",
            Self::Equation => "equation",
        }
    }
}

struct ExpectedAtom {
    value: String,
    origin: Origin,
    /// Rows proved from line-start words within one no-fill display.
    literal_row: Option<usize>,
}

#[derive(Default)]
struct TermPayload {
    atoms: Vec<ExpectedAtom>,
    structured: bool,
    partial: bool,
    literal_rows: usize,
}

impl TermPayload {
    fn record(&mut self, origin: Origin, word: &RoffWordEvidence, starts_literal_row: bool) {
        if [
            RoffWordEffect::Unclassified,
            RoffWordEffect::Continues,
            RoffWordEffect::WordEndBreak,
            RoffWordEffect::ZeroAdvance,
        ]
        .iter()
        .any(|&effect| word.has(effect))
        {
            self.partial = true;
            return;
        }
        if starts_literal_row {
            self.literal_rows += 1;
        }
        self.atoms.extend(
            word.text
                .split_ascii_whitespace()
                .map(|token| ExpectedAtom {
                    value: token.to_owned(),
                    origin,
                    literal_row: starts_literal_row.then_some(self.literal_rows),
                }),
        );
    }

    fn compare(
        &self,
        terms: &[DefinitionTerm],
        (line, column): Location,
        occurrence: usize,
        violations: &mut Vec<String>,
    ) {
        // On complete coverage, compare all occurrences of the proved
        // vocabulary. Other words can be generated list decorations.
        // Partial coverage still proves a monotonic subsequence.
        let vocabulary: HashSet<_> = self
            .atoms
            .iter()
            .filter(|item| !matches!(item.origin, Origin::Equation))
            .map(|item| item.value.as_str())
            .collect();
        let mut observation = TermObservation::default();
        for term in terms {
            observation.visit_inlines(&term.content);
            observation.end_line();
        }
        let mut observed = observation
            .atoms
            .iter()
            .filter(|item| item.equation || vocabulary.contains(item.value.as_str()));
        let mut previous_row = None;
        for expected in &self.atoms {
            let matches = |item: &ObservedAtom| {
                item.equation == matches!(expected.origin, Origin::Equation)
                    && item.value == expected.value
            };
            let found = if self.partial {
                observed.find(|item| matches(item))
            } else {
                observed.next().filter(|item| matches(item))
            };
            let Some(found) = found else {
                violations.push(format!(
                    "definition-head-{} at {line}:{column}, occurrence {occurrence}: missing or out-of-order {:?} in term",
                    expected.origin.label(), expected.value
                ));
                return;
            };
            if let Some(row) = expected.literal_row {
                if previous_row
                    .is_some_and(|(source, projected)| source < row && found.row <= projected)
                {
                    violations.push(format!(
                        "definition-head-literal at {line}:{column}, occurrence {occurrence}: missing hard line before {:?}",
                        expected.value
                    ));
                    return;
                }
                previous_row = Some((row, found.row));
            }
        }
        if !self.partial
            && let Some(extra) = observed.next()
        {
            violations.push(format!(
                "definition-head-content at {line}:{column}, occurrence {occurrence}: unexpected repeated {:?} in term",
                extra.value
            ));
        }
    }
}

#[derive(Default)]
struct TermObservation {
    atoms: Vec<ObservedAtom>,
    text: String,
    row: usize,
}

struct ObservedAtom {
    value: String,
    row: usize,
    equation: bool,
}

impl TermObservation {
    fn end_run(&mut self) {
        self.atoms
            .extend(self.text.split_ascii_whitespace().map(|word| ObservedAtom {
                value: word.to_owned(),
                row: self.row,
                equation: false,
            }));
        self.text.clear();
    }

    fn end_line(&mut self) {
        self.end_run();
        self.row += 1;
    }

    fn visit_inlines(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Text { value } | Inline::Code { value } => self.text.push_str(value),
                Inline::LineBreak {} => self.end_line(),
                Inline::Equation { value, .. } => {
                    self.end_run();
                    self.atoms.push(ObservedAtom {
                        value: value.clone(),
                        row: self.row,
                        equation: true,
                    });
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => {
                    self.visit_inlines(children);
                }
                Inline::Anchor { .. } => {}
            }
        }
    }
}

#[derive(serde::Serialize)]
pub(super) struct TermReview {
    line: u32,
    column: u32,
    reason: &'static str,
}

impl HeadPayloads {
    pub(super) fn collect(root: &Node, source: &SourceContext) -> Self {
        let mut result = Self::default();
        result.walk(root, source, &mut HashSet::new());
        result
    }

    fn walk(&mut self, node: &Node, source: &SourceContext, recovered: &mut HashSet<u32>) {
        if source.destination(node) != Some(SourceDestination::Blocks) {
            return;
        }
        if node.kind == NodeKind::Block {
            if node.macro_token.as_deref() == Some("Bl")
                && node.definition_list_style == Some(DefinitionListStyle::Tag)
                && ordinal_labels::observe(mdoc_list_items(node), source)
                    .is_some_and(|labels| labels.disposition() == OrdinalDisposition::Recovered)
            {
                // These authored numeric heads become ListItems. They
                // cannot occupy an occurrence of a definition-term owner.
                recovered.extend(mdoc_list_items(node).map(|item| item.id));
            }
            if node.macro_token.as_deref() == Some("It")
                && !recovered.contains(&node.id)
                && let Some(head) = node.children.iter().find(|part| {
                    part.kind == NodeKind::Head
                        && source.destination(part) == Some(SourceDestination::Term)
                })
            {
                let mut payload = TermPayload::default();
                if contains_structure(head, source) {
                    payload.structured = true;
                    Self::walk_part(head, source, Origin::Term, &mut payload);
                }
                self.owners
                    .entry((node.line, node.column))
                    .or_default()
                    .push(payload);
            }
        }
        for child in &node.children {
            self.walk(child, source, recovered);
        }
    }

    fn walk_part(node: &Node, source: &SourceContext, origin: Origin, payload: &mut TermPayload) {
        let mut start = 0;
        for (index, child) in node.children.iter().enumerate() {
            let structural = matches!(
                child.kind,
                NodeKind::Block | NodeKind::Table | NodeKind::Equation
            );
            let control = source_control(child);
            let closes_field = matches!(
                control,
                Some(
                    SourceControl::LineBoundary
                        | SourceControl::VerticalSpace
                        | SourceControl::Margin
                        | SourceControl::AlignedRows
                )
            );
            if structural || closes_field || child.flags.no_fill && child.flags.line_start {
                record_words(node, &node.children[start..index], source, origin, payload);
                start = index;
            }
            if structural {
                Self::collect_structure(child, source, origin, payload);
            } else if control == Some(SourceControl::AlignedRows) {
                // The generated rows have their own field retirement.
                // Retain both neighbors and disclose the unproved interval.
                payload.partial = true;
            }
            if structural || closes_field {
                start = index + 1;
            }
        }
        record_words(node, &node.children[start..], source, origin, payload);
    }

    fn collect_structure(
        node: &Node,
        source: &SourceContext,
        origin: Origin,
        payload: &mut TermPayload,
    ) {
        if source.destination(node) != Some(SourceDestination::Term) {
            return;
        }
        match node.kind {
            NodeKind::Table => {
                if node.table_row_kind == Some(TableRowKind::Data) {
                    // tbl_term.c feeds these cells to term_word() in row
                    // order. Vertical continuations do not print text.
                    for value in node
                        .table_cells
                        .iter()
                        .filter(|cell| {
                            cell.kind == TableCellKind::Text && !cell.vertical_continuation
                        })
                        .filter_map(|cell| cell.decoder_text())
                    {
                        payload.record(Origin::Table, &RoffWordEvidence::read(value), false);
                    }
                }
            }
            NodeKind::Equation => {
                if let Some(value) = node
                    .equation
                    .as_ref()
                    .map(libmandoc_rs::EquationBox::readable_text)
                    && !value.is_empty()
                {
                    payload.atoms.push(ExpectedAtom {
                        value: super::native::equation_visible_text(&value),
                        origin: Origin::Equation,
                        literal_row: None,
                    });
                }
            }
            NodeKind::Block => {
                let token = node.macro_token.as_deref();
                if let Some(kind) = mdoc_list_topology_kind(node) {
                    let origin = if kind == MdocContainerKind::Table {
                        Origin::Table
                    } else {
                        Origin::List
                    };
                    for item in mdoc_list_items(node) {
                        for part in item.children.iter().filter(|part| {
                            source.destination(part) == Some(SourceDestination::Term)
                        }) {
                            Self::walk_part(part, source, origin, payload);
                        }
                    }
                } else if matches!(token, Some("Bd" | "D1" | "Dl" | "Xo")) {
                    let display = token != Some("Xo");
                    let origin = if display { Origin::Display } else { origin };
                    for part in node
                        .children
                        .iter()
                        .filter(|part| part.kind == NodeKind::Body)
                    {
                        Self::walk_part(part, source, origin, payload);
                    }
                } else {
                    // Generated enclosures and complex formatting are
                    // outside the simple lexical witness for this interval.
                    payload.partial = true;
                }
            }
            _ => unreachable!("only structural nodes enter the term walker"),
        }
    }

    pub(super) fn compare(
        &self,
        document: &Document,
        violations: &mut Vec<String>,
    ) -> Vec<TermReview> {
        let mut observed = Terms::default();
        let mut reviews = Vec::new();
        if self
            .owners
            .values()
            .flatten()
            .any(|payload| payload.structured)
        {
            observed.visit_document(document);
        }
        for (&(line, column), groups) in &self.owners {
            if !groups.iter().any(|payload| payload.structured) {
                continue;
            }
            let terms = observed
                .0
                .get(&(line, column))
                .map_or(&[][..], Vec::as_slice);
            if terms.len() != groups.len() && (terms.len() > 1 || groups.len() > 1) {
                reviews.push(TermReview {
                    line,
                    column,
                    reason: "ambiguous-owner",
                });
                continue;
            }
            if groups
                .iter()
                .any(|payload| payload.structured && payload.partial)
            {
                reviews.push(TermReview {
                    line,
                    column,
                    reason: "partial-payload",
                });
            }
            for (index, payload) in groups
                .iter()
                .enumerate()
                .filter(|(_, payload)| payload.structured)
            {
                let terms = terms.get(index).copied().unwrap_or_default();
                payload.compare(terms, (line, column), index + 1, violations);
            }
        }
        reviews
    }
}

fn contains_structure(node: &Node, source: &SourceContext) -> bool {
    source.destination(node) == Some(SourceDestination::Term)
        && (matches!(node.kind, NodeKind::Table | NodeKind::Equation)
            || node.kind == NodeKind::Block
                && matches!(node.macro_token.as_deref(), Some("Bl" | "Bd" | "D1" | "Dl"))
            || node
                .children
                .iter()
                .any(|child| contains_structure(child, source)))
}

fn record_words(
    owner: &Node,
    words: &[Node],
    source: &SourceContext,
    origin: Origin,
    payload: &mut TermPayload,
) {
    let Some(first) = words.first() else { return };
    let (closed, word) = source_evidence(words, first, false, source, true);
    payload.partial |= closed.has_text() || closed.needs_review();
    if let Some(word) = word {
        let literal_row = matches!(origin, Origin::Display)
            && first.flags.no_fill
            && first.flags.line_start
            && owner.macro_token.as_deref() != Some("D1");
        payload.record(origin, &word, literal_row);
    }
}
