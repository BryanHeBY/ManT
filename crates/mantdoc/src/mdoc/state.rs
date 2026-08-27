use super::{
    BTreeMap, BTreeSet, DocumentBuilder, NodeId, NodeKind, Recovery, apply_presentation_flags,
    mark_sentence_end, normalize_filled_blank_lines, rebase_expanded_argument_locations,
    suppress_filled_c_blank_lines, trim_mdoc_filled_text_trailing_whitespace,
};

/// One prepared source-order event consumed by the mdoc structure pass.
pub(super) struct StructureEvent {
    pub(super) flat_index: usize,
    pub(super) node: NodeId,
    pub(super) suppressed: bool,
    pub(super) blank_line_recovery: Option<Recovery>,
}

/// Cursor and preprocessing state for the flat scanner projection.
pub(super) struct StructureEvents {
    nodes: std::iter::Enumerate<std::vec::IntoIter<NodeId>>,
    synopsis_events: std::iter::Peekable<std::vec::IntoIter<(usize, bool)>>,
    suppressed_nodes: BTreeSet<NodeId>,
    blank_line_recoveries: BTreeMap<NodeId, Recovery>,
}

impl StructureEvents {
    pub(super) fn prepare(builder: &mut DocumentBuilder, flat: Vec<NodeId>) -> Self {
        let synopsis_events = builder.take_mdoc_synopsis_events().into_iter().peekable();
        // Ordinary source text is tokenized before this package pass. Only
        // direct flat text events receive the generic sentence fallback;
        // macro arguments keep their macro-specific punctuation semantics.
        for node in &flat {
            if builder.node_kind(*node) == Some(NodeKind::Text) {
                mark_sentence_end(builder, *node);
            }
        }
        apply_presentation_flags(builder, &flat);
        trim_mdoc_filled_text_trailing_whitespace(builder, &flat);
        for node in &flat {
            let macro_name = builder.node_macro_name(*node);
            if matches!(macro_name, Some("Fd" | "Fl" | "Sy" | "Ar" | "Em" | "Sq")) {
                rebase_expanded_argument_locations(builder, *node);
            }
        }
        let suppressed = suppress_filled_c_blank_lines(builder, &flat);
        let blank_line_recoveries = normalize_filled_blank_lines(builder, &flat, &suppressed);
        Self {
            nodes: flat.into_iter().enumerate(),
            synopsis_events,
            suppressed_nodes: BTreeSet::from_iter(suppressed),
            blank_line_recoveries,
        }
    }

    pub(super) fn step(&mut self) -> Option<StructureEvent> {
        let (flat_index, node) = self.nodes.next()?;
        Some(StructureEvent {
            flat_index,
            node,
            suppressed: self.suppressed_nodes.contains(&node),
            blank_line_recovery: self.blank_line_recoveries.remove(&node),
        })
    }

    pub(super) fn next_synopsis_transition(&mut self, flat_index: usize) -> Option<bool> {
        let (boundary, _) = self.synopsis_events.peek()?;
        (*boundary <= flat_index)
            .then(|| self.synopsis_events.next().map(|(_, state)| state))
            .flatten()
    }

    pub(super) fn finish(self) {
        debug_assert!(self.nodes.len() == 0, "mdoc event machine finished early");
    }
}
