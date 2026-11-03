//! Single-dispatch receipts do not change native observation policy.

use super::{InlineBuilder, SourceLineObservation};

#[test]
fn consumed_source_line_matches_node_identity_without_enabling_disabled_observation() {
    let mut builder = InlineBuilder::with_spacing(true);
    builder.execution.note_no_fill_source_line_entered(7);
    assert!(matches!(
        builder.execution.observe_no_fill_source_lines,
        SourceLineObservation::DisabledEntered(7)
    ));
    assert!(!builder.execution.take_no_fill_source_line_entered(8));
    assert!(matches!(
        builder.execution.observe_no_fill_source_lines,
        SourceLineObservation::Disabled
    ));
    builder.execution.note_no_fill_source_line_entered(7);
    assert!(builder.execution.take_no_fill_source_line_entered(7));
    assert!(!builder.execution.take_no_fill_source_line_entered(7));
}

#[test]
fn explicit_observation_changes_keep_the_same_single_dispatch_receipt() {
    let mut builder = InlineBuilder::with_spacing(true);
    builder.observe_no_fill_source_lines(true);
    builder.execution.note_no_fill_source_line_entered(42);
    builder.observe_no_fill_source_lines(false);
    assert!(matches!(
        builder.execution.observe_no_fill_source_lines,
        SourceLineObservation::DisabledEntered(42)
    ));
    builder.observe_no_fill_source_lines(true);
    assert!(matches!(
        builder.execution.observe_no_fill_source_lines,
        SourceLineObservation::NoFillEntered(42)
    ));
    assert!(builder.execution.take_no_fill_source_line_entered(42));
    builder.execution.note_no_fill_source_line_entered(43);
    builder.execution.finish_no_fill_source_line_entry(43);
    assert!(matches!(
        builder.execution.observe_no_fill_source_lines,
        SourceLineObservation::NoFill
    ));
}

#[test]
fn grouped_dispatch_preserves_outer_receipt_until_its_source_node_returns() {
    let mut builder = InlineBuilder::with_spacing(true);
    builder.observe_no_fill_source_lines(true);
    builder.execution.note_no_fill_source_line_entered(5);
    builder.with_grouped_line_dispatch(|builder| {
        builder.execution.note_no_fill_source_line_entered(9);
        assert!(!builder.execution.take_no_fill_source_line_entered(9));
        assert!(matches!(
            builder.execution.observe_no_fill_source_lines,
            SourceLineObservation::Grouped
        ));
    });
    assert!(matches!(
        builder.execution.observe_no_fill_source_lines,
        SourceLineObservation::NoFillEntered(5)
    ));
    builder.execution.finish_no_fill_source_line_entry(5);
    assert!(!builder.execution.take_no_fill_source_line_entered(5));
    assert!(matches!(
        builder.execution.observe_no_fill_source_lines,
        SourceLineObservation::NoFill
    ));
}
