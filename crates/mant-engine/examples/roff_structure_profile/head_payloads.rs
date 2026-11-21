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
    wrappers: (String, String),
}

#[derive(Clone, Copy)]
struct DisplayRow {
    display: usize,
    within: usize,
    ended: bool,
}

#[derive(Default)]
struct TermPayload {
    atoms: Vec<ExpectedAtom>,
    structured: bool,
    partial: bool,
    opaque_words: bool,
    displays: usize,
    field: literal_rows::Field,
    short_rows: Option<literal_rows::Rows>,
    joins: HashMap<usize, &'static str>,
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
                    wrappers: Default::default(),
                }),
        );
    }

    fn leave_unproved_words(&mut self) {
        self.partial = true;
        self.opaque_words = true;
    }

    fn compound_spellings(&self) -> HashMap<usize, HashMap<String, usize>> {
        let mut compounds = HashMap::new();
        for &index in self.joins.keys() {
            let first = &self.atoms[index];
            let mut stem = first.value.clone();
            // Each edge has one source-proved separator. Generate only
            // consecutive prefixes of that chain, not a product of wrappers.
            for end in (index + 1..self.atoms.len()).take(32) {
                let Some(separator) = self.joins.get(&(end - 1)) else {
                    break;
                };
                let last = &self.atoms[end];
                stem.push_str(separator);
                for spelling in last.spellings() {
                    for left in ["", first.wrappers.0.as_str()] {
                        compounds
                            .entry(index)
                            .or_insert_with(HashMap::new)
                            .insert(format!("{left}{stem}{spelling}"), end - index);
                    }
                }
                stem.push_str(&last.value);
            }
        }
        compounds
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
        let compound = self.compound_spellings();
        let vocabulary: HashSet<_> = self
            .atoms
            .iter()
            .filter(|item| !matches!(item.origin, Origin::Equation))
            .flat_map(ExpectedAtom::spellings)
            .chain(compound.values().flat_map(|words| words.keys().cloned()))
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
        let mut witnesses = self.atoms.iter().enumerate();
        while let Some((index, expected)) = witnesses.next() {
            let joined = |item: &ObservedAtom| {
                (!item.equation)
                    .then(|| compound.get(&index)?.get(&item.value))
                    .flatten()
            };
            let matches = |item: &ObservedAtom| {
                item.equation == matches!(expected.origin, Origin::Equation)
                    && expected.spellings().any(|word| word == item.value)
                    || joined(item).is_some()
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
            if let Some(&extra) = joined(found) {
                witnesses.nth(extra - 1);
            }
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
                    } else if source.ended && found.row == projected {
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

impl ExpectedAtom {
    fn spellings(&self) -> impl Iterator<Item = String> + '_ {
        let (left, right) = &self.wrappers;
        ["", left.as_str()].into_iter().flat_map(move |left| {
            right
                .char_indices()
                .map(|(end, _)| &right[..end])
                .chain(std::iter::once(right.as_str()))
                .map(move |right| format!("{left}{}{right}", self.value))
        })
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
        result.walk(root, source, &mut HashSet::new(), None);
        result
    }

    fn walk(
        &mut self,
        node: &Node,
        source: &SourceContext,
        recovered: &mut HashSet<u32>,
        list: Option<&Node>,
    ) {
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
                let mut payload = TermPayload {
                    field: literal_rows::Field::for_item(list, node),
                    ..TermPayload::default()
                };
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
        let list = if node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("Bl") {
            Some(node)
        } else {
            list
        };
        for child in &node.children {
            self.walk(child, source, recovered, list);
        }
    }

    fn walk_part(node: &Node, source: &SourceContext, origin: Origin, payload: &mut TermPayload) {
        Self::walk_part_slice(&node.children, source, origin, payload);
    }

    fn walk_part_slice(
        children: &[Node],
        source: &SourceContext,
        origin: Origin,
        payload: &mut TermPayload,
    ) {
        let mut start = 0;
        for (index, child) in children.iter().enumerate() {
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
            let short_display = child.kind == NodeKind::Block
                && matches!(child.macro_token.as_deref(), Some("D1" | "Dl"));
            if !short_display {
                payload.short_rows = None;
            }
            if structural || closes_field || child.flags.no_fill && child.flags.line_start {
                record_words(&children[start..index], source, origin, None, payload);
                start = index;
            }
            if structural {
                Self::collect_structure(child, source, origin, payload);
                if !short_display {
                    payload.short_rows = None;
                }
            } else if control == Some(SourceControl::AlignedRows) {
                // The generated rows have their own field retirement.
                // Retain both neighbors and disclose the unproved interval.
                payload.leave_unproved_words();
            }
            if structural || closes_field {
                start = index + 1;
            }
        }
        record_words(&children[start..], source, origin, None, payload);
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
                        wrappers: Default::default(),
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
                    literal_rows::record_short(node, source, payload);
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
                    wrappers: Default::default(),
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
        let start = payload.atoms.len();
        for part in node
            .children
            .iter()
            .filter(|part| source.destination(part) == Some(SourceDestination::Term))
        {
            let from = payload.atoms.len();
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
            if part.kind == NodeKind::Body
                && let Some((left, right)) = macro_delimiters(node)
            {
                // Fo writes its opening after executing HEAD; enclosure
                // macros write theirs before executing BODY.
                if node.macro_token.as_deref() == Some("Fo") && from > start {
                    payload.atoms[from - 1].wrappers.1.push_str(&left);
                    if let Some(first) = payload.atoms.get(from)
                        && (first.row.is_some() || matches!(first.origin, Origin::Equation))
                        && !first.wrappers.0.is_empty()
                    {
                        // A nested enclosure can emit its opener before
                        // the display flushes the function-name row.
                        let opener = first.wrappers.0.clone();
                        payload.atoms[from - 1].wrappers.1.push_str(&opener);
                    }
                    // termp_fo_pre() writes '(' then suppresses the next
                    // word's separator. A compound spelling must still
                    // prove both independently authored words.
                    if !part.flags.no_fill
                        && payload.atoms.get(from).is_some_and(|first| {
                            first.row.is_none() && !matches!(first.origin, Origin::Equation)
                        })
                    {
                        payload.joins.insert(from - 1, "(");
                    }
                } else if let Some(first) = payload.atoms.get_mut(from) {
                    first.wrappers.0.insert_str(0, &left);
                }
                if payload.atoms.len() > from
                    && let Some(last) = payload.atoms.last_mut()
                    && !matches!(last.origin, Origin::Equation)
                {
                    last.wrappers.1.push_str(&right);
                }
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

fn macro_delimiters(node: &Node) -> Option<(String, String)> {
    let (open, close) = match node.enclosure.as_ref() {
        Some(enclosure) if node.macro_token.as_deref() == Some("En") => (
            enclosure.opening.as_str(),
            enclosure.closing.as_deref().unwrap_or_default(),
        ),
        _ => ordinal_labels::description::scope_delimiters(node.macro_token.as_deref())?,
    };
    Some((
        RoffWordEvidence::read(open).text,
        RoffWordEvidence::read(close).text,
    ))
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
