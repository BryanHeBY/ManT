//! Publish mathematical structure only after its executed words are accepted.
use mant_ir::{
    Diagnostic, DiagnosticImpact, DiagnosticLevel,
    visit::{self, Visit},
};

use super::{
    AnnotationKey, AnnotationPool, Block, DestinationPayload, Destinations, EquationExpression,
    HashMap, HashSet, Inline, LinkTarget, Section, SourceSpan,
};

/// One native occurrence owns one tree. Its private carriers retain only a key
/// while the ordinary word receipts decide which projected cells survive.
#[derive(Debug)]
pub(super) struct EquationDestination {
    expression: Option<EquationExpression>,
    expected: String,
    source: Option<SourceSpan>,
    chunks: usize,
    complete: bool,
    reported: bool,
}

impl EquationDestination {
    pub(super) fn new(expression: EquationExpression, source: Option<SourceSpan>) -> Self {
        Self {
            expression: Some(expression),
            expected: String::new(),
            source,
            chunks: 0,
            complete: true,
            reported: false,
        }
    }

    pub(super) fn record(&mut self, children: &[Inline], complete: bool) {
        self.chunks += 1;
        self.complete &= complete && self.chunks == 1;
        self.complete &= append_projection(children, &mut self.expected);
    }

    pub(super) fn mark_incomplete(&mut self) {
        self.complete = false;
    }

    pub(super) fn record_deferred(&mut self, children: &[Inline]) {
        // The shared zero-advance machine previews only this occurrence's
        // delayed tail. Its later release is still part of the same word
        // owner, rather than a second executed equation projection chunk.
        self.complete &= append_projection(children, &mut self.expected);
    }

    fn publish(&mut self, children: &[Inline]) -> Option<Inline> {
        let mut accepted = String::new();
        let mut ended = false;
        self.complete &= self.chunks == 1
            && append_accepted_projection(children, &mut accepted, &mut ended)
            && accepted == self.expected;
        let expression = self.expression.take()?;
        if !self.complete {
            return None;
        }
        // The native linear spelling is an execution proof, not the shared
        // mathematical reading. Derive the cache from the retained structure.
        Some(Inline::Equation {
            value: expression.readable_text(),
            expression,
        })
    }

    fn take_diagnostic(&mut self) -> Option<Diagnostic> {
        // Complete rejection can remove every carrier before publication.
        // An unrecorded configuration-only root never owned a formula word.
        let reduced = !self.complete || self.chunks > 0 && self.expression.is_some();
        if self.reported || !reduced {
            return None;
        }
        self.reported = true;
        Some(Diagnostic {
            impact: DiagnosticImpact::SemanticCoverage,
            level: DiagnosticLevel::Warning,
            code: Some("manual.equation-execution-coverage".into()),
            message: "native word execution changed or split an equation's mathematical owner. The accepted text and original annotations were retained".into(),
            source: self.source,
        })
    }
}

/// Styles do not change membership. A hard line or another semantic owner
/// cannot be hidden by turning the whole accepted interval into mathematics.
fn append_projection(nodes: &[Inline], output: &mut String) -> bool {
    let mut complete = true;
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => {
                complete &= !value.contains('\n');
                output.push_str(value);
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                complete &= append_projection(children, output);
            }
            Inline::Anchor { id, .. } => complete &= id.as_str().starts_with("\0mant:"),
            Inline::LineBreak { .. } | Inline::Link { .. } | Inline::Equation { .. } => {
                complete = false;
            }
        }
    }
    complete
}

/// A retired native pass may close its final word inside that word's private
/// carrier. This is the boundary after the mathematical owner, whereas a
/// boundary followed by more operand content still splits the equation.
fn append_accepted_projection(nodes: &[Inline], output: &mut String, ended: &mut bool) -> bool {
    let mut complete = true;
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => {
                complete &= !*ended && !value.contains('\n');
                output.push_str(value);
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                complete &= append_accepted_projection(children, output, ended);
            }
            Inline::Anchor { id, .. } => complete &= id.as_str().starts_with("\0mant:"),
            Inline::LineBreak { .. } => *ended = true,
            Inline::Link { .. } | Inline::Equation { .. } => complete = false,
        }
    }
    complete
}

/// Called only after the complete accepted mathematical prefix was proved.
/// Consume original wrappers instead of cloning glyphs or replaying layout.
fn take_trailing_boundaries(mut nodes: Vec<Inline>) -> (Vec<Inline>, bool) {
    let mut trailing = Vec::new();
    let mut complete = true;
    while let Some(node) = nodes.pop() {
        match node {
            node @ Inline::LineBreak { .. } => trailing.push(node),
            node @ Inline::Anchor { .. } if matches!(&node, Inline::Anchor { id, .. } if id.as_str().starts_with("\0mant:")) =>
            {
                trailing.push(node);
            }
            Inline::Strong { children } => {
                let (children, whole) = take_trailing_boundaries(children);
                if !children.is_empty() {
                    trailing.push(Inline::Strong { children });
                }
                if !whole {
                    complete = false;
                    break;
                }
            }
            Inline::Emphasis { children } => {
                let (children, whole) = take_trailing_boundaries(children);
                if !children.is_empty() {
                    trailing.push(Inline::Emphasis { children });
                }
                if !whole {
                    complete = false;
                    break;
                }
            }
            _ => {
                complete = false;
                break;
            }
        }
    }
    trailing.reverse();
    (trailing, complete)
}

struct EquationCarriers<'pool> {
    destinations: &'pool Destinations,
    counts: HashMap<AnnotationKey, CarrierCount>,
}

#[derive(Default)]
struct CarrierCount {
    empty: usize,
    content: usize,
}

impl<'ir> Visit<'ir> for EquationCarriers<'_> {
    fn visit_inline(&mut self, node: &'ir Inline) {
        if let Inline::Link {
            target: LinkTarget::External { uri },
            children,
            ..
        } = node
            && let Some(destination) = self.destinations.targets.get(uri)
            && matches!(destination.payload, DestinationPayload::Equation(_))
        {
            let count = self.counts.entry(destination.key).or_default();
            if super::has_scalar(children) {
                count.content += 1;
            } else {
                count.empty += 1;
            }
        }
        visit::walk_inline(self, node);
    }
}

impl AnnotationPool {
    pub(super) fn check_local_equation_carriers(&self, nodes: &[Inline]) {
        let destinations = self.0.borrow();
        let mut carriers = EquationCarriers {
            destinations: &destinations,
            counts: HashMap::new(),
        };
        for node in nodes {
            carriers.visit_inline(node);
        }
        let counts = carriers.counts;
        drop(destinations);
        self.mark_split_equations(&counts);
    }

    pub(super) fn check_document_equation_carriers(&self, blocks: &[Block], sections: &[Section]) {
        let destinations = self.0.borrow();
        let mut carriers = EquationCarriers {
            destinations: &destinations,
            counts: HashMap::new(),
        };
        for block in blocks {
            carriers.visit_block(block);
        }
        for section in sections {
            carriers.visit_section(section);
        }
        let counts = carriers.counts;
        drop(destinations);
        self.mark_split_equations(&counts);
    }

    fn mark_split_equations(&self, counts: &HashMap<AnnotationKey, CarrierCount>) {
        let mut destinations = self.0.borrow_mut();
        for (&key, count) in counts {
            // Publication already prunes an empty provisional identity when
            // the same occurrence has a later accepted content carrier.
            let count = if count.content > 0 {
                count.content
            } else {
                count.empty
            };
            if count > 1
                && let Some(destination) = destinations
                    .targets
                    .get_mut(&super::markers::target_key(key))
                && let DestinationPayload::Equation(equation) = &mut destination.payload
            {
                equation.mark_incomplete();
            }
        }
    }

    pub(super) fn publish_equation(
        &self,
        uri: &str,
        children: &mut Vec<Inline>,
        output: &mut Vec<Inline>,
        nonempty: &HashSet<String>,
    ) -> bool {
        let equation = {
            let mut destinations = self.0.borrow_mut();
            let Some(destination) = destinations.targets.get_mut(uri) else {
                return false;
            };
            let DestinationPayload::Equation(equation) = &mut destination.payload else {
                return false;
            };
            equation.publish(children)
        };
        if let Some(equation) = equation {
            output.push(equation);
            let (mut trailing, _) = take_trailing_boundaries(std::mem::take(children));
            self.publish(&mut trailing, false, nonempty);
            output.extend(trailing);
        } else {
            // Recovery keeps the actual accepted source owners and hard rows.
            // A suffix which overwrote a mathematical glyph is still prose.
            self.publish(children, true, nonempty);
            output.append(children);
        }
        true
    }

    pub(super) fn take_equation_diagnostics(&self, output: &mut Vec<Diagnostic>) {
        let mut destinations = self.0.borrow_mut();
        let mut equations: Vec<_> = destinations
            .targets
            .values_mut()
            .filter_map(|destination| match &mut destination.payload {
                DestinationPayload::Equation(equation) => Some((destination.key, equation)),
                DestinationPayload::Link(_) | DestinationPayload::Code => None,
            })
            .collect();
        equations.sort_unstable_by_key(|(key, _)| key.0);
        output.extend(
            equations
                .into_iter()
                .filter_map(|(_, equation)| equation.take_diagnostic()),
        );
    }
}
