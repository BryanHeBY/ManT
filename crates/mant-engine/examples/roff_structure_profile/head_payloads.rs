//! Prove the retained payload of structures executed in definition terms.

use libmandoc_rs::{
    DefinitionListStyle, Node, NodeKind, NormalizedListKind, TableCellKind, TableRowKind,
};
use mant_codec::{RoffWordEffect, RoffWordEvidence};
use mant_ir::{DefinitionItem, DefinitionTerm, Document, Inline, visit::Visit};
use std::collections::{BTreeMap, HashMap, HashSet};

#[path = "head_payloads/literal_rows.rs"]
mod literal_rows;

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
    ListMarker,
}

impl Origin {
    fn label(self) -> &'static str {
        match self {
            Self::Term => "text",
            Self::List => "list",
            Self::Table => "table",
            Self::Display => "literal",
            Self::Equation => "equation",
            Self::ListMarker => "list-marker",
        }
    }
}

struct ExpectedAtom {
    value: String,
    origin: Origin,
    row: Option<DisplayRow>,
}

#[derive(Clone, Copy)]
struct DisplayRow {
    display: usize,
    within: usize,
}

#[derive(Default)]
struct TermPayload {
    atoms: Vec<ExpectedAtom>,
    structured: bool,
    partial: bool,
    opaque_words: bool,
    displays: usize,
}

impl TermPayload {
    fn record(&mut self, origin: Origin, word: &RoffWordEvidence, row: Option<DisplayRow>) {
        if [
            RoffWordEffect::Unclassified,
            RoffWordEffect::WordEndBreak,
            RoffWordEffect::ZeroAdvance,
        ]
        .iter()
        .any(|&effect| word.has(effect))
        {
            self.leave_unproved_words();
            return;
        }
        self.atoms.extend(
            word.text
                .split_ascii_whitespace()
                .map(|token| ExpectedAtom {
                    value: token.to_owned(),
                    origin,
                    row,
                }),
        );
    }

    fn leave_unproved_words(&mut self) {
        self.partial = true;
        self.opaque_words = true;
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
        for (index, term) in terms.iter().enumerate() {
            observation.term = index;
            observation.row = 0;
            observation.visit_inlines(&term.content);
            observation.end_run();
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
            let found = if self.opaque_words {
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
            if let Some(row) = expected.row {
                if let Some((source, projected, term)) = previous_row {
                    let source: DisplayRow = source;
                    let problem = if found.term != term {
                        Some("hard line replaced by a separate term")
                    } else if source.display == row.display {
                        let required = row.within - source.within;
                        match found.row.saturating_sub(projected).cmp(&required) {
                            std::cmp::Ordering::Less => Some("missing hard line"),
                            std::cmp::Ordering::Greater => Some("unexpected hard line"),
                            std::cmp::Ordering::Equal => None,
                        }
                    } else if found.row == projected {
                        Some("missing hard line")
                    } else {
                        None
                    };
                    if let Some(problem) = problem {
                        violations.push(format!(
                            "definition-head-literal at {line}:{column}, occurrence {occurrence}: {problem} before {:?}", expected.value
                    ));
                        return;
                    }
                }
                previous_row = Some((row, found.row, found.term));
            } else {
                previous_row = None;
            }
        }
        if !self.opaque_words
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
    term: usize,
}

struct ObservedAtom {
    value: String,
    row: usize,
    term: usize,
    equation: bool,
}

impl TermObservation {
    fn end_run(&mut self) {
        self.atoms
            .extend(self.text.split_ascii_whitespace().map(|word| ObservedAtom {
                value: word.to_owned(),
                row: self.row,
                term: self.term,
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
                        term: self.term,
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
                record_words(&node.children[start..index], source, origin, None, payload);
                start = index;
            }
            if structural {
                Self::collect_structure(child, source, origin, payload);
            } else if control == Some(SourceControl::AlignedRows) {
                // The generated rows have their own field retirement.
                // Retain both neighbors and disclose the unproved interval.
                payload.leave_unproved_words();
            }
            if structural || closes_field {
                start = index + 1;
            }
        }
        record_words(&node.children[start..], source, origin, None, payload);
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
                        payload.record(Origin::Table, &RoffWordEvidence::read(value), None);
                    }
                }
            }
            NodeKind::Equation => {
                if let Some(equation) = node
                    .equation
                    .as_ref()
                    .filter(|equation| super::native::equation_has_payload(equation))
                {
                    payload.atoms.push(ExpectedAtom {
                        value: super::native::equation_visible_text(&equation.readable_text()),
                        origin: Origin::Equation,
                        row: None,
                    });
                }
            }
            NodeKind::Block => {
                let token = node.macro_token.as_deref();
                if let Some(kind) = mdoc_list_topology_kind(node) {
                    Self::walk_list(node, kind, source, payload);
                } else if matches!(token, Some("D1" | "Dl")) {
                    // blk_part_imp puts boundary punctuation beside BODY.
                    // termp_d1_pre and its post-handler enclose all those
                    // siblings in one executed field.
                    payload.displays += 1;
                    let row = DisplayRow {
                        display: payload.displays,
                        within: 0,
                    };
                    record_words(&node.children, source, Origin::Display, Some(row), payload);
                } else if matches!(token, Some("Bd" | "Xo")) {
                    for part in node
                        .children
                        .iter()
                        .filter(|part| part.kind == NodeKind::Body)
                    {
                        if token == Some("Xo") {
                            Self::walk_part(part, source, origin, payload);
                        } else if !literal_rows::record(node, part, source, payload) {
                            Self::walk_part(part, source, Origin::Display, payload);
                        }
                    }
                } else {
                    Self::walk_wrapper(node, source, origin, payload);
                }
            }
            _ => unreachable!("only structural nodes enter the term walker"),
        }
    }

    fn walk_list(
        node: &Node,
        kind: MdocContainerKind,
        source: &SourceContext,
        payload: &mut TermPayload,
    ) {
        let origin = if kind == MdocContainerKind::Table {
            Origin::Table
        } else {
            Origin::List
        };
        for (index, item) in mdoc_list_items(node).enumerate() {
            // termp_it_pre() writes one generated HEAD word before
            // the author's BODY, even when the two spellings agree.
            let marker = match node.list_kind {
                Some(NormalizedListKind::Ordered) => Some(format!("{}.", index + 1)),
                Some(NormalizedListKind::Bullet) => Some("•".into()),
                Some(NormalizedListKind::Dash) => Some("-".into()),
                _ => None,
            };
            if let Some(value) = marker {
                payload.atoms.push(ExpectedAtom {
                    value,
                    origin: Origin::ListMarker,
                    row: None,
                });
            }
            for part in item
                .children
                .iter()
                .filter(|part| source.destination(part) == Some(SourceDestination::Term))
            {
                Self::walk_part(part, source, origin, payload);
            }
        }
    }

    fn walk_wrapper(
        node: &Node,
        source: &SourceContext,
        origin: Origin,
        payload: &mut TermPayload,
    ) {
        // The wrapper's generated layout remains under review;
        // independently executed descendants still have witnesses.
        payload.partial = true;
        for part in node
            .children
            .iter()
            .filter(|part| source.destination(part) == Some(SourceDestination::Term))
        {
            if matches!(part.kind, NodeKind::Head | NodeKind::Body | NodeKind::Tail)
                && contains_structure(part, source)
            {
                Self::walk_part(part, source, origin, payload);
            } else if matches!(
                part.kind,
                NodeKind::Block | NodeKind::Table | NodeKind::Equation
            ) {
                Self::collect_structure(part, source, origin, payload);
            } else {
                record_words(std::slice::from_ref(part), source, origin, None, payload);
            }
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
    words: &[Node],
    source: &SourceContext,
    origin: Origin,
    row: Option<DisplayRow>,
    payload: &mut TermPayload,
) {
    let Some(first) = words.first() else { return };
    let (closed, word) = source_evidence(words, first, false, source, true);
    if closed.has_text() || closed.needs_review() {
        payload.leave_unproved_words();
    }
    if let Some(word) = word {
        payload.record(origin, &word, row);
    }
}
