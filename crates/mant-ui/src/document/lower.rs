//! IR to logical terminal content and anchors.
use super::{Arc, DocumentAddress, HashMap, LogicalLine, NavNode};

mod blocks;
mod inline;
mod lists;
mod sections;
mod table;
mod tldr;
pub(super) struct DocumentBuilder<'a> {
    pub(super) entry_styles: Arc<mant_render::EntryStyleMap<'a>>,
    pub(super) label: String,
    pub(super) address: Option<DocumentAddress>,
    pub(super) lines: Vec<LogicalLine>,
    pub(super) navigation: Vec<NavNode>,
    pub(super) anchors: HashMap<String, usize>,
    pub(super) reference_origins: Arc<super::references::ReferenceOrigins>,
    pending_anchors: Vec<String>,
    pending_gap: mant_ir::geometry::GapPlan,
    /// An inherited cursor has already emitted its rows in the actual parent.
    /// It must limit new requests without becoming this cell's completed tail.
    pending_gap_inherited: bool,
    has_content_row: bool,
    /// A nested row consumed already printed inherited gap rows without
    /// producing a new local row. Keep this finite completed-tail receipt.
    completed_external_gap: bool,
}

/// Logical payload and its anchors must cross layout boundaries together.
pub(super) struct LogicalFragment {
    pub(super) lines: Vec<LogicalLine>,
    pub(super) anchors: HashMap<String, usize>,
}

pub(super) struct BuiltDocument {
    pub(super) label: String,
    pub(super) navigation: Vec<NavNode>,
    pub(super) content: LogicalFragment,
}

impl DocumentBuilder<'_> {
    pub(super) fn finish(mut self) -> BuiltDocument {
        // No source content follows these targets. Keep the end-of-document
        // sentinel rather than inventing a visible row or landing in a gap.
        self.resolve_pending_anchors();
        BuiltDocument {
            label: self.label,
            navigation: self.navigation,
            content: LogicalFragment {
                lines: self.lines,
                anchors: self.anchors,
            },
        }
    }
    pub(super) fn new(label: String, address: Option<DocumentAddress>) -> Self {
        Self {
            entry_styles: Arc::default(),
            label,
            address,
            lines: Vec::new(),
            navigation: Vec::new(),
            anchors: HashMap::new(),
            reference_origins: Arc::default(),
            pending_anchors: Vec::new(),
            pending_gap: mant_ir::geometry::GapPlan::default(),
            pending_gap_inherited: false,
            has_content_row: false,
            completed_external_gap: false,
        }
    }

    pub(super) fn push(&mut self, line: LogicalLine) {
        self.resolve_pending_anchors();
        self.pending_gap = mant_ir::geometry::GapPlan::default();
        self.pending_gap_inherited = false;
        self.has_content_row = true;
        self.completed_external_gap = false;
        self.lines.push(line);
    }

    fn resolve_pending_anchors(&mut self) {
        for id in self.pending_anchors.drain(..) {
            self.anchors.entry(id).or_insert(self.lines.len());
        }
    }

    fn defer_anchors(&mut self, ids: impl IntoIterator<Item = String>) {
        self.pending_anchors.extend(ids);
    }

    pub(super) fn anchor(&mut self, node: NavNode) {
        self.anchors
            .insert(node.target_id.clone(), self.lines.len());
        self.navigation(node);
    }

    pub(super) fn navigation(&mut self, node: NavNode) {
        self.navigation.push(node);
    }

    pub(super) fn spacing(&mut self, lines: u16) {
        let before = self.pending_gap.rows(0);
        self.pending_gap.append_resolved(lines);
        if lines > 0 {
            self.pending_gap_inherited = false;
        }
        for _ in before..self.pending_gap.rows(0) {
            self.lines.push(LogicalLine::empty());
        }
    }
}
