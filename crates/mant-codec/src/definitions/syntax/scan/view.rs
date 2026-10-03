//! Accepted visible bytes, effective fonts and actual native operand receipts.
use super::{Inline, NativeOperand, NativeOperandRole, Range};

#[derive(Clone, Copy, Default)]
pub(super) struct Style {
    pub(super) strong: bool,
    pub(super) emphasis: bool,
}

struct Run {
    bytes: Range<usize>,
    style: Style,
}

pub(super) struct HeadView<'a> {
    pub(super) text: String,
    runs: Vec<Run>,
    operands: &'a [NativeOperand],
    literal_starts: Vec<usize>,
}

impl<'a> HeadView<'a> {
    pub(super) fn new(nodes: &[Inline], operands: &'a [NativeOperand]) -> Self {
        let mut view = Self {
            text: String::new(),
            runs: Vec::new(),
            operands,
            literal_starts: Vec::new(),
        };
        view.append(nodes, Style::default());
        // An authored leading blank belongs to the operand; its first name
        // starts later. Compute that point once instead of rescanning prefixes.
        view.literal_starts = operands
            .iter()
            .filter_map(|operand| {
                if operand.role == NativeOperandRole::Argument {
                    return None;
                }
                let text = view.text.get(operand.bytes.clone())?;
                let name = text.trim_start();
                (!name.is_empty()).then_some(operand.bytes.start + text.len() - name.len())
            })
            .collect();
        view
    }

    fn append(&mut self, nodes: &[Inline], style: Style) {
        for node in nodes {
            match node {
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    let start = self.text.len();
                    self.text.push_str(value);
                    self.runs.push(Run {
                        bytes: start..self.text.len(),
                        style,
                    });
                }
                Inline::Strong { children } => self.append(
                    children,
                    Style {
                        strong: true,
                        ..style
                    },
                ),
                Inline::Emphasis { children } => {
                    self.append(
                        children,
                        Style {
                            emphasis: true,
                            ..style
                        },
                    );
                }
                Inline::Link { children, .. } => self.append(children, style),
                Inline::LineBreak { .. } => {
                    let start = self.text.len();
                    self.text.push('\n');
                    self.runs.push(Run {
                        bytes: start..start + 1,
                        style,
                    });
                }
                Inline::Anchor { .. } => {}
            }
        }
    }

    pub(super) fn style(&self, offset: usize) -> Style {
        let index = self.runs.partition_point(|run| run.bytes.end <= offset);
        self.runs
            .get(index)
            .map_or(Style::default(), |run| run.style)
    }

    pub(super) fn operand(&self, offset: usize) -> Option<&NativeOperand> {
        let index = self
            .operands
            .partition_point(|operand| operand.bytes.end <= offset);
        self.operands
            .get(index)
            .filter(|operand| operand.bytes.contains(&offset))
    }

    pub(super) fn is_parameter(&self, offset: usize) -> bool {
        if let Some(operand) = self.operand(offset) {
            match operand.role {
                NativeOperandRole::Argument => return true,
                NativeOperandRole::ExplicitOption => return false,
                NativeOperandRole::Literal => {}
            }
        }
        let style = self.style(offset);
        style.emphasis && !style.strong
    }

    pub(super) fn literal_operand_starts(&self, offset: usize) -> bool {
        self.literal_starts.binary_search(&offset).is_ok()
    }

    pub(super) fn explicit_argument(&self, offset: usize) -> bool {
        self.operand(offset)
            .is_some_and(|operand| operand.role == NativeOperandRole::Argument)
    }

    pub(super) fn explicit_option(&self, offset: usize) -> bool {
        self.operand(offset)
            .is_some_and(|operand| operand.role == NativeOperandRole::ExplicitOption)
    }
}
