//! Root-vector boundary receipts collected while projecting, never by diffing text.

use super::padding::ProjectedNode;
use super::{Inline, NEXT_ROW, OutputNodeEdit, materialize_origins};

#[derive(Default)]
pub(in crate::mandoc::inline::flow) struct OutputNodeEdits {
    old_start: usize,
    boundaries: Vec<usize>,
    prefix: Option<OutputNodeEdit>,
}

impl OutputNodeEdits {
    pub(in crate::mandoc::inline::flow) fn remap(&self, position: &mut usize) {
        if self.boundaries.is_empty() {
            return;
        }
        if *position < self.old_start {
            if let Some(prefix) = self.prefix {
                prefix.remap(position);
            }
            return;
        }
        let relative = *position - self.old_start;
        let old_end = self.boundaries.len() - 1;
        *position =
            self.boundaries[relative.min(old_end)].saturating_add(relative.saturating_sub(old_end));
    }
}

pub(super) struct RootProjection {
    nodes: Vec<(Inline, Option<usize>)>,
    old_start: usize,
    old_len: usize,
}

impl RootProjection {
    pub(super) fn new(old_start: usize, old_len: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(old_len),
            old_start,
            old_len,
        }
    }

    pub(super) fn append(&mut self, pieces: &mut Vec<ProjectedNode>, root: Option<usize>) {
        let mut boundary = root;
        for piece in pieces.drain(..) {
            let source = piece.is_source().then(|| boundary.take()).flatten();
            self.nodes.push((piece.materialize(), source));
        }
    }

    pub(super) fn prepend(&mut self, node: Inline) {
        self.nodes.insert(0, (node, None));
    }

    /// Root carriers are edited in this existing reverse traversal. Nested
    /// carriers change their annotation's children, never a root boundary.
    pub(super) fn materialize_origins(&mut self, origin: &mut Option<u16>) {
        let mut reversed = Vec::with_capacity(self.nodes.len());
        for (mut node, source) in std::mem::take(&mut self.nodes).into_iter().rev() {
            if let Some(columns) = super::super::native_row_origin(&node) {
                *origin = Some(u16::try_from(columns).unwrap_or(u16::MAX));
                continue;
            }
            match &mut node {
                Inline::LineBreak {} => {
                    if let Some(columns) = origin.take() {
                        reversed.push((node, source));
                        reversed.push((Inline::anchor(format!("{NEXT_ROW}{columns}")), None));
                        continue;
                    }
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => materialize_origins(children, origin),
                Inline::Text { value } | Inline::Code { value } if !value.is_empty() => {
                    *origin = None;
                }
                _ => {}
            }
            reversed.push((node, source));
        }
        reversed.reverse();
        self.nodes = reversed;
    }

    pub(super) fn finish(
        self,
        output: &mut Vec<Inline>,
        prefix: Option<OutputNodeEdit>,
    ) -> OutputNodeEdits {
        let mut boundaries = vec![None; self.old_len + 1];
        for (node, source) in self.nodes {
            if let Some(source) = source {
                boundaries[source] = Some(output.len());
            }
            output.push(node);
        }
        // A removed private carrier points to the next surviving source
        // boundary. End checkpoints remain after the complete projected tail.
        let mut next = output.len();
        let mut changed = prefix.is_some();
        let boundaries = boundaries
            .into_iter()
            .enumerate()
            .rev()
            .map(|(index, position)| {
                #[cfg(test)]
                super::ROOT_BOUNDARIES_PROJECTED
                    .with(|work| work.set(work.get().saturating_add(1)));
                next = position.unwrap_or(next);
                changed |= next != self.old_start + index;
                next
            })
            .collect::<Vec<_>>();
        if !changed {
            return OutputNodeEdits::default();
        }
        OutputNodeEdits {
            old_start: self.old_start,
            boundaries: boundaries.into_iter().rev().collect(),
            prefix,
        }
    }
}
