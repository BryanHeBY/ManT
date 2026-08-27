use super::{Recovery, StructureOutcome};

/// Package-owned state that survives the source-order structure pass.
///
/// Keeping the result and its deferred findings together makes the ordering
/// boundary explicit before the driver is split into smaller passes.
#[derive(Default)]
pub(super) struct MdocContext {
    pub(super) outcome: StructureOutcome,
    pub(super) deferred: DeferredRecoveries,
}

impl DeferredRecoveries {
    /// Append the semantic queues in the order used by mandoc's validation
    /// walk, returning syntax findings that still need source-order merging.
    pub(super) fn flush_into(&mut self, outcome: &mut StructureOutcome) -> Vec<Recovery> {
        outcome.recoveries.append(&mut self.broken_items);
        outcome.recoveries.append(&mut self.list_content);
        outcome.recoveries.append(&mut self.paragraph_arguments);
        outcome.recoveries.append(&mut self.post_validation);
        std::mem::take(&mut self.syntax_stage)
    }
}

/// Findings whose observable order differs from their discovery order.
#[derive(Default)]
pub(super) struct DeferredRecoveries {
    pub(super) paragraph_arguments: Vec<Recovery>,
    pub(super) post_validation: Vec<Recovery>,
    pub(super) broken_items: Vec<Recovery>,
    pub(super) list_content: Vec<Recovery>,
    pub(super) syntax_stage: Vec<Recovery>,
}
