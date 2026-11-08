//! Conversion state belongs to one rendering call, not IR or thread-local state.
use super::{
    MAX_MARKDOWN_MATH_FORMULAS, MAX_MARKDOWN_MATH_INPUT_BYTES, MAX_MARKDOWN_MATH_ISSUES,
    MAX_MARKDOWN_MATH_NODES, MAX_MARKDOWN_MATH_OUTPUT_BYTES, MarkdownMathError, MarkdownMathIssue,
    MarkdownMathIssueKind, MarkdownMathLimit, MarkdownMathSummary,
};
use crate::encode::MarkdownOptions;
use crate::math::{MathFormat, MathLoss, MathOptions, MathPolicy, encode_equation};
use mant_ir::{Block, EquationExpression, EquationValidationLimits, Inline};
use std::{cell::RefCell, collections::HashMap};

#[derive(Clone, Copy, Default)]
pub(in crate::encode) struct RenderOptions<'ctx> {
    pub(in crate::encode) options: MarkdownOptions,
    pub(in crate::encode) math: Option<&'ctx MathContext>,
}

impl RenderOptions<'_> {
    pub(in crate::encode) fn portable(options: MarkdownOptions) -> Self {
        Self {
            options,
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
    latex: Option<String>,
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
    policy: MathPolicy,
    state: RefCell<State>,
}

impl MathContext {
    pub(in crate::encode) fn new(policy: MathPolicy) -> Self {
        Self {
            policy,
            state: RefCell::default(),
        }
    }

    fn register(&self, expression: &EquationExpression, value: &str) -> Option<usize> {
        let key = FormulaKey::Expression(std::ptr::from_ref(expression));
        let mut state = self.state.borrow_mut();
        if state.error.is_some() {
            return None;
        }
        if let Some(&formula) = state.keys.get(&key) {
            return Some(formula);
        }
        let formula = state.formulas.len();
        if formula >= MAX_MARKDOWN_MATH_FORMULAS {
            state.error = Some(limit(MarkdownMathLimit::Formulas));
            return None;
        }
        // Remaining aggregate and per-expression encoding budgets precede
        // every projection. An exhausted operation never starts a large
        // default-budget readable fallback merely to check its cache.
        let limits = EquationValidationLimits {
            max_nodes: MAX_MARKDOWN_MATH_NODES
                .saturating_sub(state.nodes)
                .min(crate::math::MAX_MATH_NODES),
            max_depth: crate::math::MAX_MATH_DEPTH,
            max_text_bytes: MAX_MARKDOWN_MATH_INPUT_BYTES
                .saturating_sub(state.input_bytes)
                .min(crate::math::MAX_MATH_INPUT_BYTES),
        };
        if let Err(error) = expression.validate(limits) {
            state.error = Some(match error {
                mant_ir::EquationValidationError::InvalidNode(_) => {
                    MarkdownMathError::InvalidEquation {
                        formula,
                        message: error.to_string(),
                    }
                }
                mant_ir::EquationValidationError::TextByteLimit => {
                    limit(MarkdownMathLimit::InputBytes)
                }
                mant_ir::EquationValidationError::DepthLimit => limit(MarkdownMathLimit::Depth),
                mant_ir::EquationValidationError::NodeLimit => limit(MarkdownMathLimit::Nodes),
            });
            return None;
        }
        if expression.readable_text() != value {
            state.error = Some(MarkdownMathError::InvalidEquation {
                formula,
                message: "equation text does not match its owned expression".into(),
            });
            return None;
        }
        let mut stack = vec![expression];
        while let Some(node) = stack.pop() {
            state.nodes += 1;
            for value in [&node.text, &node.left, &node.right, &node.top, &node.bottom]
                .into_iter()
                .flatten()
            {
                state.input_bytes += value.len();
            }
            stack.extend(node.children.iter().rev());
        }
        state.keys.insert(key, formula);
        state.formulas.push(Formula {
            latex: None,
            losses: Vec::new(),
            first_loss: None,
            loss_count: 0,
            fallback: None,
            encoded: false,
            readable_bytes: value.len(),
            fallback_reserved: false,
        });
        Some(formula)
    }

    pub(in crate::encode) fn latex(
        &self,
        expression: &EquationExpression,
        value: &str,
    ) -> Option<String> {
        let formula = self.register(expression, value)?;
        let mut state = self.state.borrow_mut();
        if state.formulas[formula].latex.is_none() {
            // Strictness is applied to the final accepted branch at finish.
            // A rejected style/link candidate must not poison a later safe
            // mathematical candidate or report losses for bytes never used.
            let result = encode_equation(
                expression,
                MathOptions {
                    format: MathFormat::Latex,
                    display: false,
                    policy: MathPolicy::BestEffort,
                },
            );
            let artifact = match result {
                Ok(artifact) => artifact,
                Err(error) => {
                    state.error = Some(MarkdownMathError::Encoding { formula, error });
                    return None;
                }
            };
            let framed_bytes = artifact.text.len().saturating_add(16);
            if framed_bytes > MAX_MARKDOWN_MATH_OUTPUT_BYTES.saturating_sub(state.output_bytes) {
                state.error = Some(limit(MarkdownMathLimit::OutputBytes));
                return None;
            }
            state.output_bytes += framed_bytes;
            let available = MAX_MARKDOWN_MATH_ISSUES.saturating_sub(state.details);
            let loss_count = artifact.losses.len();
            let first_loss = artifact.losses.first().cloned();
            let losses: Vec<_> = artifact.losses.into_iter().take(available).collect();
            state.details += losses.len();
            state.formulas[formula].latex = Some(artifact.text);
            state.formulas[formula].loss_count = loss_count;
            state.formulas[formula].first_loss = first_loss;
            state.formulas[formula].losses = losses;
        }
        state.formulas[formula].encoded = true;
        state.formulas[formula].fallback = None;
        state.formulas[formula].latex.clone()
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
        if state.error.is_some() {
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
                state.error = Some(limit(MarkdownMathLimit::OutputBytes));
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
        if state.error.is_some() {
            return;
        }
        if let Some(&formula) = state.keys.get(&key) {
            drop(state);
            self.mark_reading(formula, MarkdownMathIssueKind::MissingStructure);
            return;
        }
        let formula = state.formulas.len();
        if formula >= MAX_MARKDOWN_MATH_FORMULAS {
            state.error = Some(limit(MarkdownMathLimit::Formulas));
            return;
        }
        let Block::Equation { value, .. } = owner else {
            return;
        };
        if value.len() > MAX_MARKDOWN_MATH_INPUT_BYTES.saturating_sub(state.input_bytes) {
            state.error = Some(limit(MarkdownMathLimit::InputBytes));
            return;
        }
        state.input_bytes += value.len();
        state.keys.insert(key, formula);
        state.formulas.push(Formula {
            latex: None,
            losses: Vec::new(),
            first_loss: None,
            loss_count: 0,
            fallback: None,
            encoded: false,
            readable_bytes: value.len(),
            fallback_reserved: false,
        });
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
        if depth > crate::math::MAX_MATH_DEPTH {
            state.error = Some(limit(MarkdownMathLimit::Depth));
            return false;
        }
        if state.context_work >= MAX_MARKDOWN_MATH_NODES {
            state.error = Some(limit(MarkdownMathLimit::Nodes));
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

fn limit(limit: MarkdownMathLimit) -> MarkdownMathError {
    MarkdownMathError::LimitExceeded { limit }
}
