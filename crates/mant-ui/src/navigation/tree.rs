//! Width-independent topology of the final sidebar forest, never inferred from
//! folding, the viewport, or builder-time `is_last` flags.
use crate::{NavKind, NavNode};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Default)]
struct Branch {
    parent: Option<usize>,
    next_sibling: bool,
}

pub(crate) struct TreePlan<'a> {
    pub(super) nodes: &'a [NavNode],
    branches: Vec<Branch>,
}

pub(super) struct Prefixes {
    pub(super) first: String,
    pub(super) continuation: String,
    #[cfg(test)]
    ancestor_visits: usize,
}

impl<'a> TreePlan<'a> {
    pub(crate) fn new(nodes: &'a [NavNode]) -> Self {
        #[cfg(test)]
        TRACE.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                trace.0 += 1;
            }
        });
        let mut branches = vec![Branch::default(); nodes.len()];
        let mut last_child = vec![None; nodes.len()];
        let mut last_root = None;
        let mut ancestors: Vec<usize> = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            while ancestors
                .last()
                .is_some_and(|parent| nodes[*parent].depth >= node.depth)
            {
                ancestors.pop();
            }
            let parent = ancestors.last().copied().filter(|parent| {
                nodes[*parent].depth.checked_add(1) == Some(node.depth)
                    && node.parent_id.as_deref() == Some(nodes[*parent].id.as_str())
            });
            branches[index].parent = parent;
            // Malformed/orphan nodes keep their existing identity and visibility
            // policy. Do not fabricate a parent or join them to unrelated roots.
            let previous = if let Some(parent) = parent {
                last_child[parent].replace(index)
            } else if node.depth == 0 && node.parent_id.is_none() {
                last_root.replace(index)
            } else {
                None
            };
            if let Some(previous) = previous {
                branches[previous].next_sibling = true;
            }
            ancestors.push(index);
        }
        Self { nodes, branches }
    }

    #[cfg(test)]
    pub(super) fn record_layout(&self, width: usize) {
        TRACE.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                trace.1.push((std::ptr::from_ref(self) as usize, width));
            }
        });
    }

    pub(super) fn prefixes(
        &self,
        index: usize,
        selected: bool,
        expanded: bool,
        width: usize,
    ) -> Prefixes {
        let node = &self.nodes[index];
        let branch = self.branches[index];
        // At most this many two-cell slots can survive suffix clipping. Collect
        // nearest ancestors first, without ever materializing depth-sized text.
        let budget = width.saturating_sub(1);
        let slots = budget.div_ceil(2);
        let mut ancestors = Vec::new();
        let mut parent = branch.parent;
        while ancestors.len() < slots {
            let Some(index) = parent else { break };
            // The direct parent owns the current branch's column; earlier
            // ancestors supply the guide columns to its left.
            let Some(grandparent) = self.branches[index].parent else {
                break;
            };
            ancestors.push(self.branches[index].next_sibling);
            parent = Some(grandparent);
        }
        let mut first = String::from(if selected { " › " } else { "   " });
        let mut continuation = String::from("   ");
        for &continues in ancestors.iter().rev() {
            let slot = if continues { "│ " } else { "  " };
            first.push_str(slot);
            continuation.push_str(slot);
        }
        if branch.parent.is_some() {
            first.push_str(if branch.next_sibling {
                "├─"
            } else {
                "╰─"
            });
            continuation.push_str(if branch.next_sibling { "│ " } else { "  " });
        }
        first.push_str(if node.kind == NavKind::Tldr {
            "◆ "
        } else if node.has_children {
            if expanded { "▾ " } else { "▸ " }
        } else if matches!(node.kind, NavKind::Entry(_)) {
            "◇ "
        } else {
            "· "
        });
        // The marker slot may carry the owner's child guide, but never add a
        // new slot just because the owner is expanded.
        continuation.push_str(if node.has_children && expanded {
            "│ "
        } else {
            "  "
        });
        Prefixes {
            first: super::bounded_tree_prefix(&first, width),
            continuation: super::bounded_tree_prefix(&continuation, width),
            #[cfg(test)]
            ancestor_visits: ancestors.len(),
        }
    }
}

#[cfg(test)]
type LayoutTrace = (usize, Vec<(usize, usize)>);
#[cfg(test)]
thread_local! {
    static TRACE: std::cell::RefCell<Option<LayoutTrace>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(crate) fn start_layout_trace() {
    TRACE.with_borrow_mut(|trace| *trace = Some((0, Vec::new())));
}
#[cfg(test)]
pub(crate) fn take_layout_trace() -> LayoutTrace {
    TRACE.with_borrow_mut(|trace| trace.take().expect("layout trace started"))
}
