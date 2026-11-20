//! Independent source-side witness for the mdoc ordinal recovery audit.

use std::collections::{HashMap, HashSet};

use libmandoc_rs::{Node, NodeKind, NormalizedListKind, TableCellKind};

#[path = "ordinal_labels/controls.rs"]
pub(crate) mod controls;
#[path = "ordinal_labels/description.rs"]
pub(crate) mod description;
use controls::{SourceControl, source_control};
use description::{Payload, description};
use mant_codec::{RoffWordEffect as WordEffect, RoffWordEvidence as NativeWord};

pub(crate) fn skips_source_children(node: &Node) -> bool {
    // print_mdoc_node() ignores these nodes or never walks their operands.
    // In particular, obsolete Db retains its operands for diagnostics.
    node.flags.no_print
        || node.kind == NodeKind::Comment
        || source_control(node) == Some(SourceControl::Skip)
}

#[cfg(test)]
#[path = "ordinal_label_cases.rs"]
pub(crate) mod cases;

pub(crate) struct NativeOrdinalLabels {
    pub(crate) terms: Vec<String>,
    disposition: OrdinalDisposition,
}

impl NativeOrdinalLabels {
    pub(crate) fn disposition(&self) -> OrdinalDisposition {
        if self.terms.len() >= 2 {
            self.disposition
        } else {
            OrdinalDisposition::Retained
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OrdinalDisposition {
    Recovered,
    Retained,
    Uncertain,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum FontEvidence {
    #[default]
    Plain,
    Unproved,
}

#[derive(Default)]
pub(crate) struct SourceContext {
    destinations: HashMap<u32, SourceDestination>,
    no_space_nodes: HashSet<u32>,
    unproved_font_owners: HashSet<u32>,
    joined_flag_dashes: HashSet<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceDestination {
    Blocks,
    Term,
}

#[derive(Default)]
struct SourceState {
    no_space: bool,
    font: FontEvidence,
}

impl SourceContext {
    pub(crate) fn collect(root: &Node) -> Self {
        let mut spacing = Self::default();
        spacing.walk(
            root,
            &mut SourceState::default(),
            None,
            SourceDestination::Blocks,
        );
        spacing
    }

    pub(crate) fn no_space_at(&self, node: &Node) -> bool {
        self.no_space_nodes.contains(&node.id)
    }

    pub(crate) fn destination(&self, node: &Node) -> Option<SourceDestination> {
        self.destinations.get(&node.id).copied()
    }

    pub(crate) fn font_at(&self, node: &Node) -> FontEvidence {
        if self.unproved_font_owners.contains(&node.id) {
            FontEvidence::Unproved
        } else {
            FontEvidence::Plain
        }
    }

    pub(crate) fn joins_flag_operand(&self, node: &Node) -> bool {
        self.joined_flag_dashes.contains(&node.id)
    }

    fn walk(
        &mut self,
        node: &Node,
        state: &mut SourceState,
        list: Option<NormalizedListKind>,
        mut destination: SourceDestination,
    ) {
        if skips_source_children(node) {
            return;
        }
        // mdoc_term.c::termp_it_pre() walks only definition HEADs. Other
        // lists can retain skipped HEAD arguments for validator messages.
        if node.kind == NodeKind::Head && node.macro_token.as_deref() == Some("It") {
            if list != Some(NormalizedListKind::Definition) {
                return;
            }
            destination = SourceDestination::Term;
        }
        self.destinations.insert(node.id, destination);
        if state.no_space {
            self.no_space_nodes.insert(node.id);
        }
        match (node.kind, node.macro_token.as_deref()) {
            (NodeKind::Element, Some("Sm")) => {
                state.no_space = spacing_mode(node, state.no_space);
                return;
            }
            (_, Some("ft")) => {
                state.font = font_request(node, state.font);
                return;
            }
            (NodeKind::Head | NodeKind::Body, Some("It"))
            | (NodeKind::Block, Some("D1" | "Dl" | "Bd")) => {
                if state.font == FontEvidence::Unproved {
                    self.unproved_font_owners.insert(node.id);
                }
            }
            (NodeKind::Table, _) => {
                // tbl_term.c::term_tbl() clears NONOSPACE after each span.
                state.no_space = false;
                return;
            }
            (NodeKind::Text, _)
                if node
                    .text
                    .as_deref()
                    .is_some_and(|text| text.contains(r"\f")) =>
            {
                state.font = FontEvidence::Unproved;
            }
            _ => {}
        }
        let previous_font = state.font;
        let scoped_font = source_font_scope(node);
        if scoped_font {
            state.font = FontEvidence::Unproved;
        }
        self.observe_flag_spacing(&node.children);
        let list = node.list_kind.or(list);
        for child in &node.children {
            self.walk(child, state, list, destination);
        }
        // print_mdoc_node() restores the font stack depth after a macro
        // push. An ordinary word or roff ft can replace the active slot.
        if scoped_font {
            state.font = previous_font;
        }
    }

    fn observe_flag_spacing(&mut self, nodes: &[Node]) {
        let mut next = None::<&Node>;
        for node in nodes.iter().rev() {
            if roff_transparent(node) {
                continue;
            }
            // termp_fl_pre() glues a bare dash before the next macro
            // on the same execution line, using roff_node_next().
            if node.kind == NodeKind::Element
                && node.macro_token.as_deref() == Some("Fl")
                && (!node.children.is_empty()
                    || next
                        .is_some_and(|next| next.kind != NodeKind::Text && !next.flags.line_start))
            {
                self.joined_flag_dashes.insert(node.id);
            }
            next = Some(node);
        }
    }
}

fn roff_transparent(node: &Node) -> bool {
    skips_source_children(node)
        || matches!(
            node.macro_token.as_deref(),
            Some(
                "ft" | "ll"
                    | "mc"
                    | "po"
                    | "ta"
                    | "Db"
                    | "Es"
                    | "Sm"
                    | "Tg"
                    | "DT"
                    | "UC"
                    | "PD"
                    | "AT"
            )
        )
}

fn font_request(node: &Node, previous: FontEvidence) -> FontEvidence {
    match node.children.first().and_then(|arg| arg.text.as_deref()) {
        Some("1" | "R" | "CR" | "CW") => FontEvidence::Plain,
        Some("" | "B" | "3" | "I" | "2" | "BI" | "4" | "P" | "CB" | "CI") => FontEvidence::Unproved,
        _ => previous,
    }
}

fn source_font_scope(node: &Node) -> bool {
    matches!(
        (node.kind, node.macro_token.as_deref()),
        (NodeKind::Head, Some("Sh" | "Ss"))
            | (NodeKind::Body, Some("Bf"))
            | (
                NodeKind::Element,
                Some("Ar" | "Em" | "Sy" | "Cm" | "Ic" | "Li" | "Fl" | "Lk" | "In" | "Fn")
            )
    )
}

fn spacing_mode(node: &Node, previous: bool) -> bool {
    // mdoc_term.c::termp_sm_pre() uses one persistent mode for all words.
    node.children
        .first()
        .and_then(|child| child.text.as_deref())
        .map_or(!previous, |option| option != "on")
}

pub(crate) fn observe<'a>(
    items: impl Iterator<Item = &'a Node>,
    spacing: &SourceContext,
) -> Option<NativeOrdinalLabels> {
    let mut terms = Vec::new();
    let mut has_candidate = false;
    let mut disposition = OrdinalDisposition::Recovered;
    let mut previous = None;
    for item in items {
        let mut label = NativeLabel::default();
        if let Some(head) = part(item, NodeKind::Head) {
            label.no_space_mode = spacing.no_space_at(head);
            label.walk(head, None);
        }
        let text = label.evidence.text.as_str();
        let marker = split_ordinal(text.trim()).and_then(|(digits, punctuation)| {
            let mut words = digits.split_whitespace().peekable();
            has_candidate |= words.peek().is_some()
                && words.all(|word| word.trim_start_matches('+').parse::<u64>().is_ok());
            let number: u64 = digits.parse().ok()?;
            // Displayed source punctuation is checked separately from the
            // exact decimal spelling generated by an ordinary list marker.
            (text == text.trim()
                && punctuation != Punctuation::Brackets
                && number.to_string() == digits)
                .then_some((number, punctuation))
        });
        let follows = marker.is_some_and(|(number, punctuation)| {
            previous.is_none_or(|(prior, mark): (u64, Punctuation)| {
                prior.checked_add(1) == Some(number) && mark == punctuation
            })
        });
        let body = part(item, NodeKind::Body).map_or_else(Payload::default, |body| {
            description(body, label.continues, spacing)
        });
        if (!follows && label.effect != LabelEffect::Uncertain)
            || label.effect == LabelEffect::Retained
            || body.absent()
        {
            disposition = OrdinalDisposition::Retained;
        } else if disposition != OrdinalDisposition::Retained
            && (label.effect == LabelEffect::Uncertain || body.needs_review())
        {
            disposition = OrdinalDisposition::Uncertain;
        }
        previous = marker;
        terms.push(text.to_owned());
    }
    has_candidate.then_some(NativeOrdinalLabels { terms, disposition })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Punctuation {
    Period,
    Close,
    Paired,
    Brackets,
}

fn split_ordinal(text: &str) -> Option<(&str, Punctuation)> {
    if let Some(digits) = text.strip_suffix('.') {
        Some((digits, Punctuation::Period))
    } else if let Some(digits) = text.strip_suffix(')') {
        Some(match digits.strip_prefix('(') {
            Some(digits) => (digits, Punctuation::Paired),
            None => (digits, Punctuation::Close),
        })
    } else {
        let digits = text.strip_prefix('[')?.strip_suffix(']')?;
        Some((digits, Punctuation::Brackets))
    }
}

fn part(node: &Node, kind: NodeKind) -> Option<&Node> {
    node.children.iter().find(|child| child.kind == kind)
}

#[derive(Default)]
struct NativeLabel {
    evidence: NativeWord,
    join_next: bool,
    no_space_mode: bool,
    effect: LabelEffect,
    continues: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum LabelEffect {
    #[default]
    Replaceable,
    Uncertain,
    Retained,
}

impl NativeLabel {
    fn walk(&mut self, node: &Node, next: Option<&Node>) {
        if skips_source_children(node) {
            return;
        }
        if node.kind == NodeKind::Text {
            // CVS print_mdoc_node() sets NOSPACE for delimiter TEXT around
            // term_word(). Styling wrappers alone do not suppress spaces.
            if node.flags.no_fill
                && node.flags.line_start
                && !self.continues
                && !self.evidence.text.is_empty()
            {
                self.evidence.text.push('\n');
            }
            self.word(
                node.text.as_deref().unwrap_or_default(),
                node.flags.delimiter_close,
                node.flags.delimiter_open,
            );
            return;
        }
        if node.kind == NodeKind::Equation {
            // Eqn glyphs may also spell an ordinal, but the expression is
            // independent of its reading text and must stay in its term.
            self.effect = LabelEffect::Retained;
            if let Some(equation) = &node.equation {
                self.word(&equation.readable_text(), false, false);
            }
            return;
        }
        if node.kind == NodeKind::Table {
            // A native data span includes independent row and cell owners,
            // even when its only cell has the spelling of a list marker.
            self.effect = LabelEffect::Retained;
            for cell in &node.table_cells {
                if cell.kind == TableCellKind::Text
                    && !cell.vertical_continuation
                    && let Some(text) = cell.text.as_deref()
                {
                    self.word(text, false, false);
                }
            }
            self.no_space_mode = false;
            self.join_next = true;
            return;
        }
        if let Some(control) = source_control(node) {
            self.visit_control(node, control);
            return;
        }
        let token = node.macro_token.as_deref();
        match (node.kind, token) {
            (_, Some("Ns")) => {
                self.join_next |= !node.flags.line_start;
                return;
            }
            (_, Some("Pf")) => {
                self.walk_children(node);
                // CVS termp_pf_post() selects NOSPACE only before the next
                // sibling, and only if that sibling does not start a line.
                self.join_next |= next.is_some_and(|next| !next.flags.line_start);
                return;
            }
            (NodeKind::Block, Some("Pq" | "Po" | "Bq" | "Bo")) => {
                let (open, close) = if matches!(token, Some("Pq" | "Po")) {
                    ("(", ")")
                } else {
                    ("[", "]")
                };
                self.word(open, node.flags.delimiter_close, true);
                if let Some(body) = part(node, NodeKind::Body) {
                    self.walk(body, None);
                }
                self.word(close, true, node.flags.delimiter_open);
                return;
            }
            (NodeKind::Block, Some("En")) => {
                self.custom_enclosure(node);
                return;
            }

            // Paired enclosure parts carry positions but their delimiters
            // are already emitted around the corresponding block body.
            (
                _,
                Some(
                    "It" | "Xo" | "Bf" | "Bk" | "Sy" | "Em" | "Li" | "No" | "En" | "Pq" | "Po"
                    | "Pc" | "Bq" | "Bo" | "Bc",
                ),
            ) => {}
            // A native link or real boundary remains meaningful even
            // when its own displayed words resemble an ordinal marker.
            (_, Some("Sx" | "Xr" | "Lk" | "Mt")) => {
                self.effect = LabelEffect::Retained;
            }
            // An unmodeled node can expose a candidate spelling without
            // proving whether the complete label can be replaced.
            _ => self.effect = self.effect.max(LabelEffect::Uncertain),
        }
        self.walk_children(node);
    }

    fn visit_control(&mut self, node: &Node, control: SourceControl) {
        match control {
            SourceControl::SpacingMode => {
                self.no_space_mode = spacing_mode(node, self.no_space_mode);
                if !self.no_space_mode && !self.evidence.text.is_empty() {
                    self.join_next = false;
                }
            }
            SourceControl::LineBoundary | SourceControl::VerticalSpace => {
                self.effect = LabelEffect::Retained;
            }
            SourceControl::AlignedRows => {
                self.effect = LabelEffect::Retained;
                for (index, child) in node.children.iter().enumerate().skip(1) {
                    self.walk(child, node.children.get(index + 1));
                }
            }
            SourceControl::Margin => {
                // roff_term_pre_mc() flushes a nonempty buffer with
                // NOBREAK and clears NOSPACE. Its operand belongs to the
                // margin, so it contributes no label text.
                if !self.evidence.text.is_empty() || self.evidence.has(WordEffect::Buffered) {
                    self.join_next = self.no_space_mode;
                }
            }
            SourceControl::State | SourceControl::Skip => {}
        }
    }

    fn custom_enclosure(&mut self, node: &Node) {
        // termp_quote_pre/post execute normalized Es words in the same
        // stream as the enclosed BODY, including numeric glyphs.
        if let Some(enclosure) = &node.enclosure {
            self.word(&enclosure.opening, node.flags.delimiter_close, true);
            if let Some(body) = part(node, NodeKind::Body) {
                self.walk(body, None);
            }
            if let Some(closing) = &enclosure.closing {
                self.word(closing, true, node.flags.delimiter_open);
            } else {
                self.join_next = self.no_space_mode;
            }
        } else {
            self.effect = self.effect.max(LabelEffect::Uncertain);
            self.walk_children(node);
        }
    }

    fn walk_children(&mut self, node: &Node) {
        for (index, child) in node.children.iter().enumerate() {
            self.walk(child, node.children.get(index + 1));
        }
    }

    fn word(&mut self, word: &str, closes: bool, opens: bool) {
        if !self.evidence.text.is_empty() && !self.join_next && !closes {
            self.observe_word(" ");
        }
        self.observe_word(word);
        self.join_next = self.no_space_mode || opens || self.continues;
    }

    fn observe_word(&mut self, source: &str) {
        self.evidence.extend(source);
        if self.evidence.has(WordEffect::EmittedBreak) {
            self.effect = LabelEffect::Retained;
        } else if self.evidence.has(WordEffect::Unclassified) {
            self.effect = self.effect.max(LabelEffect::Uncertain);
        }
        self.continues = self.evidence.has(WordEffect::Continues);
    }
}
