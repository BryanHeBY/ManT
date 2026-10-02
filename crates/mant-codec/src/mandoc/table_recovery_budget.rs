//! Operation-local allowance for optional source-backed tbl enhancement.
//!
//! This is not a native parser/owned-tree safety limit. Refusal keeps the
//! finalized cell payload; speculative formatter state and diagnostics remain
//! governed by `tables::recovery::CellCandidate`. Work already charged is never
//! returned when a candidate declines, fails, or is rolled back.

use mant_ir::{EquationExpression, Inline, LinkTarget};

pub(super) const MAX_FRAGMENT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug)]
pub(super) struct Limits {
    pub(super) scan: usize,
    pub(super) input: usize,
    pub(super) attempts: usize,
    pub(super) output_nodes: usize,
    pub(super) output_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            scan: 4 * 1024 * 1024,
            input: 4 * 1024 * 1024,
            attempts: 256,
            output_nodes: 65_536,
            output_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Exhaustion {
    Scan,
    Input,
    Attempts,
    OutputNodes,
    OutputBytes,
}

#[derive(Debug, Default)]
pub(super) struct TableRecoveryBudget {
    limits: Limits,
    scan: usize,
    input: usize,
    attempts: usize,
    output_nodes: usize,
    output_bytes: usize,
    refused_fragments: usize,
    exhaustion: Option<Exhaustion>,
}

impl TableRecoveryBudget {
    #[cfg(test)]
    pub(super) fn with_limits(limits: Limits) -> Self {
        Self {
            limits,
            ..Self::default()
        }
    }

    pub(super) fn charge_scan(&mut self, units: usize) -> bool {
        self.charge(units, Exhaustion::Scan)
    }

    pub(super) fn charge_input(&mut self, bytes: usize) -> bool {
        self.charge(bytes, Exhaustion::Input)
    }

    /// Debit before preparing or cloning a speculative candidate. Raw replay
    /// is another attempt, including when a semantic parse already declined.
    pub(super) fn begin_candidate(&mut self, source_bytes: usize) -> bool {
        if !self.charge(1, Exhaustion::Attempts) {
            return false;
        }
        if source_bytes > MAX_FRAGMENT_BYTES {
            self.reject_fragment();
            return false;
        }
        self.charge_input(source_bytes)
    }

    pub(super) fn reject_fragment(&mut self) {
        // A local size refusal spends prior work but does not exhaust the
        // page's remaining allowance for another, smaller safe cell.
        self.refused_fragments = self.refused_fragments.saturating_add(1);
    }

    pub(super) const fn refused_fragments(&self) -> usize {
        self.refused_fragments
    }

    pub(super) const fn exhaustion(&self) -> Option<Exhaustion> {
        self.exhaustion
    }

    pub(super) const fn attempts(&self) -> usize {
        self.attempts
    }

    /// Bound admission of already generated candidate output. The finite
    /// source/parse limits bound the transient candidate; this visit does not
    /// claim to prevent every allocation made by the native parser.
    #[cfg(test)]
    pub(super) fn charge_output(&mut self, nodes: &[Inline]) -> bool {
        self.charge_output_with_text_bytes(nodes).is_some()
    }

    /// The accepted visible byte count also admits a later ownership-copy
    /// without rewalking the candidate just to calculate its allocation.
    pub(super) fn charge_output_with_text_bytes(&mut self, nodes: &[Inline]) -> Option<usize> {
        let mut text_bytes = 0;
        self.charge_inlines(nodes, &mut text_bytes)
            .then_some(text_bytes)
    }

    fn charge_inlines(&mut self, nodes: &[Inline], text_bytes: &mut usize) -> bool {
        for node in nodes {
            if !self.charge(1, Exhaustion::OutputNodes) {
                return false;
            }
            let accepted = match node {
                Inline::Text { value } | Inline::Code { value } => {
                    *text_bytes = text_bytes.saturating_add(value.len());
                    self.charge_output_text(value)
                }
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    self.charge_inlines(children, text_bytes)
                }
                Inline::Link {
                    target,
                    title,
                    children,
                } => {
                    self.charge_target(target)
                        && self.charge_optional_output(title.as_deref())
                        && self.charge_inlines(children, text_bytes)
                }
                Inline::Equation { value, expression } => {
                    *text_bytes = text_bytes.saturating_add(value.len());
                    self.charge_output_text(value) && self.charge_equation(expression)
                }
                Inline::Anchor {
                    id,
                    fragment_aliases,
                    ..
                } => {
                    self.charge_output_text(id)
                        && fragment_aliases
                            .iter()
                            .all(|alias| self.charge_output_text(alias.as_str()))
                }
                Inline::LineBreak { .. } => {
                    *text_bytes = text_bytes.saturating_add(1);
                    true
                }
            };
            if !accepted {
                return false;
            }
        }
        true
    }

    fn charge_target(&mut self, target: &LinkTarget) -> bool {
        match target {
            LinkTarget::External { uri } => self.charge_output_text(uri),
            LinkTarget::Email { address } => self.charge_output_text(address),
            LinkTarget::Document { name, fragment } => {
                self.charge_output_text(name) && self.charge_optional_output(fragment.as_deref())
            }
            LinkTarget::Manual {
                name,
                manual_section,
            } => {
                self.charge_output_text(name)
                    && self.charge_optional_output(manual_section.as_deref())
            }
            LinkTarget::Section { id } => self.charge_output_text(id),
        }
    }

    fn charge_equation(&mut self, equation: &EquationExpression) -> bool {
        self.charge(1, Exhaustion::OutputNodes)
            && [
                equation.text.as_deref(),
                equation.left.as_deref(),
                equation.right.as_deref(),
                equation.top.as_deref(),
                equation.bottom.as_deref(),
            ]
            .into_iter()
            .all(|text| self.charge_optional_output(text))
            && equation
                .children
                .iter()
                .all(|child| self.charge_equation(child))
    }

    fn charge_optional_output(&mut self, text: Option<&str>) -> bool {
        text.is_none_or(|text| self.charge_output_text(text))
    }

    fn charge_output_text(&mut self, text: &str) -> bool {
        self.charge(text.len(), Exhaustion::OutputBytes)
    }

    fn charge(&mut self, units: usize, reason: Exhaustion) -> bool {
        if self.exhaustion.is_some() {
            return false;
        }
        let (used, maximum) = match reason {
            Exhaustion::Scan => (&mut self.scan, self.limits.scan),
            Exhaustion::Input => (&mut self.input, self.limits.input),
            Exhaustion::Attempts => (&mut self.attempts, self.limits.attempts),
            Exhaustion::OutputNodes => (&mut self.output_nodes, self.limits.output_nodes),
            Exhaustion::OutputBytes => (&mut self.output_bytes, self.limits.output_bytes),
        };
        *used = used.saturating_add(units);
        if *used > maximum {
            self.exhaustion = Some(reason);
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests;
