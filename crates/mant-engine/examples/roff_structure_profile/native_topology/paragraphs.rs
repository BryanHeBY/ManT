//! Local evidence for inline equations in prose, definitions and no-fill flows.

use super::{
    DefinitionOccurrence, EquationOwner, FlowOccurrence, Node, NodeKind, SourceOccurrence,
    TopologyOwners, equation_has_payload, ordinal_labels::controls::source_control,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub(super) enum PrecedingDefinition<'source> {
    Known(DefinitionOccurrence),
    Inferred(&'source Node),
    Unproved,
}

#[derive(Clone, Copy, Default)]
enum LiteralWitness {
    #[default]
    Unproved,
    Empty,
    Started(FlowOccurrence),
}

impl PrecedingDefinition<'_> {
    fn resolve(self, topology: &mut TopologyOwners) -> Option<DefinitionOccurrence> {
        match self {
            Self::Known(owner) => Some(owner),
            Self::Inferred(head) => Some(next_definition(head, topology)),
            Self::Unproved => None,
        }
    }
}

/// Local provenance within a section or a separately identified definition.
pub(super) struct ParagraphWitness {
    section: SourceOccurrence,
    definition: Option<DefinitionOccurrence>,
    exact_owner: bool,
    simple: bool,
    owner: Option<SourceOccurrence>,
    occurrences: BTreeMap<u32, usize>,
    unproved_lines: BTreeSet<u32>,
    uncertain_start: Option<u32>,
    literal: LiteralWitness,
}

impl ParagraphWitness {
    pub(super) fn new(section: SourceOccurrence, definition: Option<DefinitionOccurrence>) -> Self {
        Self {
            section,
            definition,
            exact_owner: true,
            simple: true,
            owner: None,
            occurrences: BTreeMap::new(),
            unproved_lines: BTreeSet::new(),
            uncertain_start: None,
            literal: LiteralWitness::default(),
        }
    }

    pub(super) fn observe<'source>(
        &mut self,
        children: &'source [Node],
        topology: &mut TopologyOwners,
    ) -> Option<PrecedingDefinition<'source>> {
        let mut preceding_definition = None;
        for child in children {
            let token = child.macro_token.as_deref();
            let preceding = preceding_definition.take();
            if child.kind == NodeKind::Block && matches!(token, Some("TP" | "IP")) {
                self.begin_paragraph(child.line);
                if has_simple_man_tag(child) {
                    let definition = next_definition(child, topology);
                    if let Some(body) = block_body(child) {
                        let _ = Self::new(self.section, Some(definition))
                            .observe(&body.children, topology);
                    }
                    preceding_definition = Some(PrecedingDefinition::Known(definition));
                } else {
                    preceding_definition = Some(PrecedingDefinition::Unproved);
                }
                continue;
            }
            if child.kind == NodeKind::Block && token == Some("RS") {
                preceding_definition = self.observe_relative_scope(child, preceding, topology);
                continue;
            }
            if matches!(token, Some("Pp" | "PP" | "P" | "LP")) {
                self.begin_paragraph(child.line);
                if let Some(body) = block_body(child) {
                    preceding_definition = self.observe(&body.children, topology);
                    self.owner = None;
                }
                continue;
            }
            if child.kind == NodeKind::Block
                && token == Some("Bd")
                && matches!(
                    child.display_kind,
                    Some(libmandoc_rs::DisplayKind::Literal | libmandoc_rs::DisplayKind::Unfilled)
                )
                && let Some(body) = block_body(child)
            {
                // The display BODY has its own literal run. Its parser
                // flags still govern internal .fi/.nf.
                let mut display = Self::new(self.section, self.definition);
                display.exact_owner = self.exact_owner;
                display.literal = LiteralWitness::Empty;
                let _ = display.observe(&body.children, topology);
                self.begin_paragraph(child.line);
                continue;
            }
            if token == Some("Tg") {
                // mdoc_validate.c::post_tg either keeps the target here
                // or installs it on a later node. A retained target can
                // give the current IR paragraph its first source owner.
                if child.flags.deep_link_target {
                    if child.flags.no_fill {
                        self.literal = LiteralWitness::Unproved;
                    } else if self.simple {
                        self.start(child.line);
                    }
                }
                continue;
            }
            if child.flags.no_print || token == Some("Sm") {
                preceding_definition = preceding;
                continue;
            }
            if preserves_declaration_predecessor(child) {
                // These handlers change row/word formatting without
                // inserting a competing declaration or prose owner.
                preceding_definition = preceding;
            } else if preceding.is_some() && source_control(child).is_some() {
                preceding_definition = Some(PrecedingDefinition::Unproved);
            } else if self.owner.is_none() {
                preceding_definition = inferred_owner_candidate(child);
            }
            self.observe_inline(child, topology);
        }
        preceding_definition
    }

    fn observe_relative_scope<'source>(
        &mut self,
        node: &Node,
        preceding: Option<PrecedingDefinition<'source>>,
        topology: &mut TopologyOwners,
    ) -> Option<PrecedingDefinition<'source>> {
        self.begin_paragraph(node.line);
        let body = block_body(node)?;
        // pre_RS/post_RS close the current line and delimit an indented
        // scope. A proved positive run can extend a preceding definition.
        let next = match (preceding, relative_indent(node)) {
            (None, _) | (_, RelativeIndent::Nonpositive) => {
                let _ = self.observe(&body.children, topology);
                None
            }
            (Some(preceding), offset) => {
                let definition = match offset {
                    RelativeIndent::Positive => preceding.resolve(topology),
                    _ => None,
                };
                let mut continuation = Self::new(self.section, definition);
                continuation.exact_owner = definition.is_some();
                let _ = continuation.observe(&body.children, topology);
                Some(definition.map_or(PrecedingDefinition::Unproved, PrecedingDefinition::Known))
            }
        };
        self.owner = None;
        next
    }

    fn observe_inline(&mut self, node: &Node, topology: &mut TopologyOwners) {
        let token = node.macro_token.as_deref();
        let prose = !node.flags.no_fill
            && (node.kind == NodeKind::Text && starts_prose(node)
                || node.kind == NodeKind::Element
                    && (matches!(
                        token,
                        Some("No" | "Em" | "Sy" | "Li" | "B" | "I" | "SB" | "SM")
                    ) && matches!(node.children.as_slice(), [text] if starts_prose(text))
                        // termp_an_pre() and termp_lk_pre() can supply
                        // the first printed operand of this paragraph.
                        || matches!(token, Some("An" | "Lk"))
                            && node.children.first().is_some_and(starts_prose)));
        let has_equation = node.equation.as_ref().is_some_and(equation_has_payload);
        self.observe_literal(node, has_equation, topology);
        if prose && self.simple {
            if self.owner.is_none() && matches!(token, Some("An" | "Lk")) {
                // When an expanded macro also begins a paragraph at
                // these coordinates, the source line no longer proves
                // which occurrence owns the equation.
                self.uncertain_start = Some(node.line);
            }
            self.start(node.line);
        } else if node.kind == NodeKind::Equation {
            match node.equation_inline {
                Some(false) if has_equation => {
                    self.begin_paragraph(node.line);
                }
                Some(true) if has_equation && node.flags.no_fill => {
                    let location = match self.literal {
                        LiteralWitness::Started(owner) => Some(owner),
                        _ => None,
                    };
                    topology
                        .inline_owners
                        .insert(node.id, EquationOwner::Literal(location));
                }
                Some(true) if has_equation => {
                    if self.simple {
                        self.start(node.line);
                    }
                    if self.exact_owner
                        && let Some(owner) = self.owner
                        && !self.unproved_lines.contains(&owner.line)
                    {
                        // An inline equation can be authored on a
                        // later line than its active paragraph.
                        topology.inline_owners.insert(
                            node.id,
                            EquationOwner::Paragraph(FlowOccurrence {
                                definition: self.definition,
                                source: owner,
                            }),
                        );
                    }
                }
                _ => {}
            }
        } else if node.kind != NodeKind::Text || node.flags.line_start {
            if self.owner.is_none() && node.flags.line_start {
                self.uncertain_start = Some(node.line);
            }
            self.simple = false;
            self.owner = None;
        }
    }

    fn begin_paragraph(&mut self, line: u32) {
        if self.uncertain_start.take() == Some(line) {
            // An unseen predecessor at this coordinate can shift the
            // paragraph occurrence after either kind of content boundary.
            self.unproved_lines.insert(line);
        }
        self.simple = true;
        self.owner = None;
        self.literal = LiteralWitness::Unproved;
    }

    fn observe_literal(&mut self, node: &Node, has_equation: bool, topology: &mut TopologyOwners) {
        if matches!(node.macro_token.as_deref(), Some("nf" | "EX")) {
            // roff_term_pre_br()/pre_literal() close the prior row at
            // these transitions into a fresh no-fill owner.
            self.literal = LiteralWitness::Empty;
        } else if node.flags.no_fill
            && (node.kind == NodeKind::Text || node.equation_inline == Some(true) && has_equation)
        {
            if matches!(self.literal, LiteralWitness::Empty) {
                self.literal = if self.exact_owner && (starts_prose(node) || has_equation) {
                    LiteralWitness::Started(FlowOccurrence {
                        definition: self.definition,
                        source: SourceOccurrence::next(
                            topology
                                .literal_occurrences
                                .entry((self.section, self.definition))
                                .or_default(),
                            node.line,
                        ),
                    })
                } else {
                    LiteralWitness::Unproved
                };
            }
        } else {
            // Do not guess which literal block an intervening request or
            // a semantic owner produced. Keep the local review for that
            // occurrence while still requiring a literal where it is proved.
            self.literal = LiteralWitness::Unproved;
        }
    }

    fn start(&mut self, line: u32) {
        self.owner
            .get_or_insert_with(|| SourceOccurrence::next(&mut self.occurrences, line));
    }
}

fn preserves_declaration_predecessor(node: &Node) -> bool {
    // The origin and source ownership of a prior declaration survive
    // ordinary formatting before RS. Treat other controls separately
    // because they can change mode, margin or the first row's origin.
    matches!(
        node.macro_token.as_deref(),
        Some("sp" | "br" | "PD" | "ft" | "ta")
    )
}

fn block_body(node: &Node) -> Option<&Node> {
    (node.kind == NodeKind::Block)
        .then(|| {
            node.children
                .iter()
                .find(|part| part.kind == NodeKind::Body)
        })
        .flatten()
}

fn next_definition(node: &Node, topology: &mut TopologyOwners) -> DefinitionOccurrence {
    DefinitionOccurrence::next(&mut topology.definitions, node.line, node.column)
}

enum RelativeIndent {
    Positive,
    Nonpositive,
    Unproved,
}

fn relative_indent(node: &Node) -> RelativeIndent {
    let [argument] = super::super::native::node_part_children(node, NodeKind::Head) else {
        // pre_RS uses the active lmargin when the operand is missing. It
        // does not prove that the run stays with ordinary section prose.
        return RelativeIndent::Unproved;
    };
    let Some(text) = argument.text.as_deref() else {
        return RelativeIndent::Unproved;
    };
    let (value, cell) = if let Some(value) = text.strip_suffix('i') {
        (value, 0.1)
    } else {
        (
            text.strip_suffix('n')
                .or_else(|| text.strip_suffix('m'))
                .unwrap_or(text),
            1.0,
        )
    };
    // term_ascii.c::ascii_hspan uses 240 units per inch and 24 per en.
    // A fractional cell can be absorbed at the active origin.
    match value.parse::<f64>() {
        Ok(distance) if distance.is_finite() && distance <= 0.0 => RelativeIndent::Nonpositive,
        Ok(distance) if (cell..4096.0 * cell).contains(&distance) => RelativeIndent::Positive,
        _ => RelativeIndent::Unproved,
    }
}

fn has_simple_man_tag(node: &Node) -> bool {
    let children = super::super::native::node_part_children(node, NodeKind::Head);
    // A simple named tag does not take the ordered/bullet projection.
    // More complex heads retain value/section checking without asserting
    // an exact definition address.
    match children {
        [text, ..] if node.macro_token.as_deref() == Some("IP") => simple_tag(text),
        [text] => {
            simple_tag(text)
                || matches!(text.macro_token.as_deref(), Some("B" | "I"))
                    && matches!(text.children.as_slice(), [operand] if simple_tag(operand))
        }
        _ => false,
    }
}

fn simple_tag(node: &Node) -> bool {
    node.text
        .as_deref()
        .map(|text| text.trim_start_matches(r"\-").trim_start_matches('-'))
        .is_some_and(|word| {
            word.starts_with(|ch: char| ch.is_ascii_alphabetic())
                && word
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        })
}

fn inferred_owner_candidate(node: &Node) -> Option<PrecedingDefinition<'_>> {
    if !matches!(node.macro_token.as_deref(), Some("B" | "I")) || node.children.is_empty() {
        return None;
    }
    // Recognize only a standalone, styled, literal option head. Other
    // styled lines can be inferred definitions depending on context.
    let option = node.macro_token.as_deref() == Some("B")
        && matches!(node.children.as_slice(), [text] if simple_tag(text)
            && text.text.as_deref().is_some_and(|word| word.starts_with(r"\-") || word.starts_with('-')));
    Some(if option {
        PrecedingDefinition::Inferred(node)
    } else {
        PrecedingDefinition::Unproved
    })
}

fn starts_prose(node: &Node) -> bool {
    node.kind == NodeKind::Text
        && node.text.as_deref().is_some_and(|text| {
            !text.trim_start().is_empty() && !text.trim_start().starts_with('\\')
        })
}
