//! Conversion state belongs to one rendering call, not IR or thread-local state.
use super::{
    MAX_MARKDOWN_MATH_FORMULAS, MAX_MARKDOWN_MATH_INPUT_BYTES, MAX_MARKDOWN_MATH_ISSUES,
    MAX_MARKDOWN_MATH_NODES, MAX_MARKDOWN_MATH_OUTPUT_BYTES, MarkdownMathError, MarkdownMathIssue,
    MarkdownMathIssueKind, MarkdownMathLimit, MarkdownMathSummary,
};
use crate::encode::{MarkdownMath, MarkdownOptions};
use crate::math::{MathFormat, MathLoss, MathOptions, MathPolicy, encode_equation};
use mant_ir::{
    Block, EquationExpression, EquationValidationError, EquationValidationLimits, Inline,
};
use std::{cell::RefCell, collections::HashMap};

#[cfg(test)]
#[path = "context/tests.rs"]
mod tests;

#[derive(Clone, Copy, Default)]
pub(in crate::encode) struct RenderOptions<'ctx> {
    pub(in crate::encode) options: MarkdownOptions,
    pub(in crate::encode) math: Option<&'ctx MathContext>,
}

impl RenderOptions<'_> {
    pub(in crate::encode) fn portable(options: MarkdownOptions) -> Self {
        Self {
            options: MarkdownOptions {
                math: crate::encode::MarkdownMath::Readable,
                ..options
            },
            math: None,
        }
    }
}

impl From<MarkdownOptions> for RenderOptions<'_> {
    fn from(options: MarkdownOptions) -> Self {
        Self::portable(options)
    }
}

impl std::ops::Deref for RenderOptions<'_> {
    type Target = MarkdownOptions;
    fn deref(&self) -> &Self::Target {
        &self.options
    }
}

impl std::ops::DerefMut for RenderOptions<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.options
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum FormulaKey {
    Expression(*const EquationExpression),
    Missing(*const Block),
}

struct Formula {
    blocked: bool,
    markup: [Option<String>; 2],
    losses: Vec<MathLoss>,
    first_loss: Option<MathLoss>,
    loss_count: usize,
    fallback: Option<MarkdownMathIssueKind>,
    encoded: bool,
    readable_bytes: usize,
    fallback_reserved: bool,
}

#[derive(Default)]
struct State {
    exhausted: bool,
    keys: HashMap<FormulaKey, usize>,
    formulas: Vec<Formula>,
    nodes: usize,
    input_bytes: usize,
    output_bytes: usize,
    details: usize,
    context_work: usize,
    error: Option<MarkdownMathError>,
}

pub(in crate::encode) struct MathContext {
    mode: MarkdownMath,
    policy: MathPolicy,
    recover_errors: bool,
    memoize: bool,
    state: RefCell<State>,
}

impl MathContext {
    pub(in crate::encode) fn new(policy: MathPolicy, mode: MarkdownMath) -> Self {
        Self {
            mode,
            policy,
            recover_errors: false,
            memoize: true,
            state: RefCell::default(),
        }
    }

    pub(in crate::encode) fn export(memoize: bool, mode: MarkdownMath) -> Self {
        Self {
            mode,
            policy: MathPolicy::BestEffort,
            recover_errors: true,
            memoize,
            state: RefCell::default(),
        }
    }

    fn begin(&self, state: &mut State, key: FormulaKey, bytes: usize) -> Option<usize> {
        if self.memoize
            && let Some(&formula) = state.keys.get(&key)
        {
            return Some(formula);
        }
        if state.error.is_some() || state.exhausted {
            return None;
        }
        let formula = state.formulas.len();
        if formula >= MAX_MARKDOWN_MATH_FORMULAS {
            self.reject(state, None, limit(MarkdownMathLimit::Formulas));
            return None;
        }
        if self.memoize {
            state.keys.insert(key, formula);
        }
        state.formulas.push(Formula {
            blocked: false,
            markup: [None, None],
            losses: Vec::new(),
            first_loss: None,
            loss_count: 0,
            fallback: None,
            encoded: false,
            readable_bytes: bytes,
            fallback_reserved: false,
        });
        Some(formula)
    }

    fn reject(&self, state: &mut State, formula: Option<usize>, error: MarkdownMathError) {
        if !self.recover_errors {
            state.error = Some(error);
            return;
        }
        let kind = match error {
            MarkdownMathError::LimitExceeded { limit } => {
                state.exhausted = true;
                MarkdownMathIssueKind::BudgetLimit(limit)
            }
            MarkdownMathError::Encoding {
                error: crate::math::MathError::LimitExceeded { limit },
                ..
            } => MarkdownMathIssueKind::EncodingLimit(limit),
            _ => MarkdownMathIssueKind::InvalidEquation,
        };
        if let Some(formula) = formula {
            let entry = &mut state.formulas[formula];
            entry.blocked = true;
            entry.encoded = false;
            entry.fallback = Some(kind);
        }
    }

    fn register(&self, expression: &EquationExpression, value: &str) -> Option<usize> {
        let key = FormulaKey::Expression(std::ptr::from_ref(expression));
        let mut state = self.state.borrow_mut();
        if self.memoize
            && let Some(&formula) = state.keys.get(&key)
        {
            return Some(formula);
        }
        let formula = self.begin(&mut state, key, value.len())?;
        // Remaining aggregate and per-expression encoding budgets precede
        // every projection. An exhausted operation never starts a large
        // default-budget readable fallback merely to check its cache.
        let remaining_nodes = MAX_MARKDOWN_MATH_NODES.saturating_sub(state.nodes);
        let remaining_bytes = MAX_MARKDOWN_MATH_INPUT_BYTES.saturating_sub(state.input_bytes);
        let limits = EquationValidationLimits {
            max_nodes: remaining_nodes.min(crate::math::MAX_MATH_NODES),
            max_depth: crate::math::MAX_MATH_DEPTH,
            max_text_bytes: remaining_bytes.min(crate::math::MAX_MATH_INPUT_BYTES),
        };
        let validation = expression.validate_with_report(limits);
        // Failed validation and cache checks still consume the traversed
        // prefix. Candidate presentation changes never refund this work.
        state.nodes = state.nodes.saturating_add(validation.visited_nodes);
        state.input_bytes = state.input_bytes.saturating_add(validation.text_bytes);
        if let Err(error) = validation.result {
            let error = validation_error(
                formula,
                error,
                remaining_nodes,
                remaining_bytes,
                validation.text_bytes,
            );
            self.reject(&mut state, Some(formula), error);
            return Some(formula);
        }
        if expression.readable_text() != value {
            self.reject(
                &mut state,
                Some(formula),
                MarkdownMathError::InvalidEquation {
                    formula,
                    message: "equation text does not match its owned expression".into(),
                },
            );
            return Some(formula);
        }
        Some(formula)
    }

    #[cfg(test)]
    pub(super) fn latex(&self, expression: &EquationExpression, value: &str) -> Option<String> {
        self.encoded(expression, value, false)
    }

    pub(in crate::encode) fn encoded(
        &self,
        expression: &EquationExpression,
        value: &str,
        display: bool,
    ) -> Option<String> {
        let format = match self.mode {
            MarkdownMath::Latex => MathFormat::Latex,
            MarkdownMath::Mathml => MathFormat::Mathml,
            MarkdownMath::Readable => return None,
        };
        let slot = usize::from(format == MathFormat::Mathml && display);
        let formula = self.register(expression, value)?;
        let mut state = self.state.borrow_mut();
        if state.error.is_some() || state.formulas[formula].blocked {
            return None;
        }
        if state.formulas[formula].markup[slot].is_none() {
            // Strictness is applied to the final accepted branch at finish.
            // A rejected style/link candidate must not poison a later safe
            // mathematical candidate or report losses for bytes never used.
            let result = encode_equation(
                expression,
                MathOptions {
                    format,
                    display: format == MathFormat::Mathml && display,
                    policy: MathPolicy::BestEffort,
                },
            );
            let artifact = match result {
                Ok(artifact) => artifact,
                Err(error) => {
                    self.reject(
                        &mut state,
                        Some(formula),
                        MarkdownMathError::Encoding { formula, error },
                    );
                    return None;
                }
            };
            let text = if format == MathFormat::Mathml {
                match super::mathml::markdown_fragment(&artifact.text) {
                    Ok(text) => text,
                    Err(error) => {
                        self.reject(
                            &mut state,
                            Some(formula),
                            MarkdownMathError::Encoding { formula, error },
                        );
                        return None;
                    }
                }
            } else {
                artifact.text
            };
            let framed_bytes = text.len().saturating_add(16);
            if framed_bytes > MAX_MARKDOWN_MATH_OUTPUT_BYTES.saturating_sub(state.output_bytes) {
                self.reject(
                    &mut state,
                    Some(formula),
                    limit(MarkdownMathLimit::OutputBytes),
                );
                return None;
            }
            state.output_bytes += framed_bytes;
            // Inline/display variants have the same tree losses. Charge details
            // once for this owner while charging both retained markup strings.
            if state.formulas[formula].markup.iter().all(Option::is_none) {
                let available = MAX_MARKDOWN_MATH_ISSUES.saturating_sub(state.details);
                let loss_count = artifact.losses.len();
                let first_loss = artifact.losses.first().cloned();
                let losses: Vec<_> = artifact.losses.into_iter().take(available).collect();
                state.details += losses.len();
                state.formulas[formula].loss_count = loss_count;
                state.formulas[formula].first_loss = first_loss;
                state.formulas[formula].losses = losses;
            }
            state.formulas[formula].markup[slot] = Some(text);
        }
        state.formulas[formula].encoded = true;
        state.formulas[formula].fallback = None;
        state.formulas[formula].markup[slot].clone()
    }

    pub(in crate::encode) fn reading(
        &self,
        expression: &EquationExpression,
        value: &str,
        kind: MarkdownMathIssueKind,
    ) {
        if let Some(formula) = self.register(expression, value) {
            self.mark_reading(formula, kind);
        }
    }

    fn mark_reading(&self, formula: usize, kind: MarkdownMathIssueKind) {
        let mut state = self.state.borrow_mut();
        if state.error.is_some() || state.formulas[formula].blocked {
            return;
        }
        if self.recover_errors {
            state.formulas[formula].encoded = false;
            state.formulas[formula].fallback = Some(kind);
            return;
        }
        if !state.formulas[formula].fallback_reserved {
            // Code rows may each require backtick padding and hard-break
            // framing. Reserve eight payload bytes plus fixed delimiters
            // per input byte, covering even alternating backticks and LF.
            // Charge it once even if rejected candidates revisit the owner.
            let bytes = state.formulas[formula]
                .readable_bytes
                .saturating_mul(8)
                .saturating_add(32);
            if bytes > MAX_MARKDOWN_MATH_OUTPUT_BYTES.saturating_sub(state.output_bytes) {
                self.reject(
                    &mut state,
                    Some(formula),
                    limit(MarkdownMathLimit::OutputBytes),
                );
                return;
            }
            state.output_bytes += bytes;
            state.formulas[formula].fallback_reserved = true;
        }
        state.formulas[formula].encoded = false;
        state.formulas[formula].fallback = Some(kind);
    }

    pub(in crate::encode) fn missing(&self, owner: &Block) {
        let key = FormulaKey::Missing(std::ptr::from_ref(owner));
        let mut state = self.state.borrow_mut();
        if self.memoize
            && let Some(&formula) = state.keys.get(&key)
        {
            drop(state);
            self.mark_reading(formula, MarkdownMathIssueKind::MissingStructure);
            return;
        }
        let Block::Equation { value, .. } = owner else {
            return;
        };
        let Some(formula) = self.begin(&mut state, key, value.len()) else {
            return;
        };
        if value.len() > MAX_MARKDOWN_MATH_INPUT_BYTES.saturating_sub(state.input_bytes) {
            self.reject(
                &mut state,
                Some(formula),
                limit(MarkdownMathLimit::InputBytes),
            );
            return;
        }
        state.input_bytes += value.len();
        drop(state);
        self.mark_reading(formula, MarkdownMathIssueKind::MissingStructure);
    }

    pub(in crate::encode) fn reading_inlines(&self, nodes: &[Inline], kind: MarkdownMathIssueKind) {
        self.reading_inlines_depth(nodes, kind, 0);
    }

    fn reading_inlines_depth(&self, nodes: &[Inline], kind: MarkdownMathIssueKind, depth: usize) {
        for node in nodes {
            if !self.visit_context(depth) {
                break;
            }
            match node {
                Inline::Equation { value, expression } => self.reading(expression, value, kind),
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => {
                    self.reading_inlines_depth(children, kind, depth + 1);
                }
                _ => {}
            }
        }
    }

    pub(in crate::encode) fn reading_blocks(&self, blocks: &[Block], kind: MarkdownMathIssueKind) {
        self.reading_blocks_depth(blocks, kind, 0);
    }

    fn reading_blocks_depth(&self, blocks: &[Block], kind: MarkdownMathIssueKind, depth: usize) {
        for block in blocks {
            if !self.visit_context(depth) {
                break;
            }
            match block {
                Block::Equation {
                    value,
                    expression: Some(expression),
                    ..
                } => self.reading(expression, value, kind),
                Block::Equation {
                    expression: None, ..
                } => self.missing(block),
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    self.reading_inlines_depth(children, kind, depth + 1);
                }
                Block::List { items, .. } => {
                    for item in items {
                        if !self.visit_context(depth + 1) {
                            return;
                        }
                        self.reading_blocks_depth(&item.blocks, kind, depth + 1);
                    }
                }
                Block::DefinitionList { items, .. } => {
                    for item in items {
                        if !self.visit_context(depth + 1) {
                            return;
                        }
                        for term in &item.terms {
                            if !self.visit_context(depth + 1) {
                                return;
                            }
                            self.reading_inlines_depth(&term.content, kind, depth + 1);
                        }
                        self.reading_blocks_depth(&item.description, kind, depth + 1);
                    }
                }
                Block::Table { rows, .. } => {
                    for row in rows {
                        if !self.visit_context(depth + 1) {
                            return;
                        }
                        for cell in &row.cells {
                            if !self.visit_context(depth + 1) {
                                return;
                            }
                            self.reading_blocks_depth(&cell.blocks, kind, depth + 2);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn visit_context(&self, depth: usize) -> bool {
        let mut state = self.state.borrow_mut();
        if state.error.is_some() {
            return false;
        }
        if self.recover_errors {
            if depth > EquationValidationLimits::default().max_depth {
                self.reject(&mut state, None, limit(MarkdownMathLimit::Depth));
                return false;
            }
            return true;
        }
        if depth > crate::math::MAX_MATH_DEPTH {
            self.reject(&mut state, None, limit(MarkdownMathLimit::Depth));
            return false;
        }
        if state.context_work >= MAX_MARKDOWN_MATH_NODES {
            self.reject(&mut state, None, limit(MarkdownMathLimit::Nodes));
            return false;
        }
        state.context_work += 1;
        true
    }

    pub(in crate::encode) fn failed(&self) -> bool {
        self.state.borrow().error.is_some()
    }

    pub(in crate::encode) fn finish(
        self,
    ) -> Result<(Vec<MarkdownMathIssue>, MarkdownMathSummary), MarkdownMathError> {
        let state = self.state.into_inner();
        if let Some(error) = state.error {
            return Err(error);
        }
        let mut issues = Vec::new();
        let mut summary = MarkdownMathSummary {
            formulas: state.formulas.len(),
            budget_exhausted: state.exhausted,
            ..Default::default()
        };
        for (formula, entry) in state.formulas.into_iter().enumerate() {
            if let Some(kind) = entry.fallback {
                if self.policy == MathPolicy::Strict {
                    return Err(MarkdownMathError::UnsupportedContext { formula, kind });
                }
                summary.fallback_formulas += 1;
                summary.issues += 1;
                if issues.len() < MAX_MARKDOWN_MATH_ISSUES {
                    issues.push(MarkdownMathIssue {
                        formula,
                        path: Vec::new(),
                        kind,
                    });
                }
            } else if entry.encoded {
                if self.policy == MathPolicy::Strict
                    && let Some(loss) = entry.first_loss
                {
                    return Err(MarkdownMathError::Encoding {
                        formula,
                        error: crate::math::MathError::Loss(loss),
                    });
                }
                summary.encoded_formulas += 1;
                summary.issues += entry.loss_count;
                for loss in entry.losses {
                    if issues.len() >= MAX_MARKDOWN_MATH_ISSUES {
                        break;
                    }
                    issues.push(MarkdownMathIssue {
                        formula,
                        path: loss.path,
                        kind: MarkdownMathIssueKind::Encoding(loss.kind),
                    });
                }
            }
        }
        summary.details_omitted = summary.issues.saturating_sub(issues.len());
        Ok((issues, summary))
    }
}

fn validation_error(
    formula: usize,
    error: EquationValidationError,
    remaining_nodes: usize,
    remaining_bytes: usize,
    visited_bytes: usize,
) -> MarkdownMathError {
    match error {
        EquationValidationError::InvalidNode(_) => MarkdownMathError::InvalidEquation {
            formula,
            message: error.to_string(),
        },
        EquationValidationError::TextByteLimit
            if remaining_bytes <= crate::math::MAX_MATH_INPUT_BYTES
                || visited_bytes > remaining_bytes =>
        {
            limit(MarkdownMathLimit::InputBytes)
        }
        EquationValidationError::NodeLimit if remaining_nodes <= crate::math::MAX_MATH_NODES => {
            limit(MarkdownMathLimit::Nodes)
        }
        EquationValidationError::NodeLimit => MarkdownMathError::Encoding {
            formula,
            error: crate::math::MathError::LimitExceeded {
                limit: crate::math::MathLimit::Nodes,
            },
        },
        EquationValidationError::DepthLimit => MarkdownMathError::Encoding {
            formula,
            error: crate::math::MathError::LimitExceeded {
                limit: crate::math::MathLimit::Depth,
            },
        },
        EquationValidationError::TextByteLimit => MarkdownMathError::Encoding {
            formula,
            error: crate::math::MathError::LimitExceeded {
                limit: crate::math::MathLimit::InputBytes,
            },
        },
    }
}

fn limit(limit: MarkdownMathLimit) -> MarkdownMathError {
    MarkdownMathError::LimitExceeded { limit }
}
