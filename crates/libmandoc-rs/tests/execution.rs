#![cfg(feature = "execute")]

use libmandoc_rs::{
    AtomDisposition, AtomKind, AtomRole, BoundaryRequest, ExecutionCancellation,
    ExecutionControlRequest, ExecutionErrorKind, ExecutionFont, ExecutionHeadingKind,
    ExecutionLimits, ExecutionLogicalTab, ExecutionManBlockKind, ExecutionMdocListKind,
    ExecutionReferenceKind, ExecutionRegionKind, ExecutionTableAlignment, ExecutionTableDataKind,
    ExecutionTableLayoutKind, ExecutionTableRowKind, ExecutionWrapperKind, FlushOutcome,
    FragmentRole, InputFormat, NativeExecutionReport, Node, NodeKind, ParseOptions, Parser,
};

const MAN: &[u8] = include_bytes!("fixtures/execution/plain-man.1");
const MDOC: &[u8] = include_bytes!("fixtures/execution/plain-mdoc.1");
const BUFFER_MUTATIONS: &[u8] = include_bytes!("fixtures/execution/buffer-mutations.1");
const EMPTY_WORD_MAN: &[u8] = include_bytes!("fixtures/execution/empty-word-man.1");
const FIELD_CONSUMPTION_MAN: &[u8] = include_bytes!("fixtures/execution/field-consumption-man.1");
const CONTROL_EFFECTS_MAN: &[u8] = include_bytes!("fixtures/execution/control-effects-man.1");
const CONTROL_EFFECTS_MDOC: &[u8] = include_bytes!("fixtures/execution/control-effects-mdoc.1");
const CONTROL_NESTED_MAN: &[u8] = include_bytes!("fixtures/execution/control-nested-man.1");
const CONTROL_GENERATED_MAN: &[u8] = include_bytes!("fixtures/execution/control-generated-man.1");
const CONTROL_WORK_MAN: &[u8] = include_bytes!("fixtures/execution/control-work-man.1");
const CONTROL_NUMERIC_OVERFLOW_MAN: &[u8] =
    include_bytes!("fixtures/execution/control-numeric-overflow-man.1");
const CONTROL_NUMERIC_UNITS_MAN: &[u8] =
    include_bytes!("fixtures/execution/control-numeric-units-man.1");
const CONTROL_NUMERIC_UNDERFLOW_MAN: &[u8] =
    include_bytes!("fixtures/execution/control-numeric-underflow-man.1");
const CONTROL_LARGE_NEGATIVE_VS_MAN: &[u8] =
    include_bytes!("fixtures/execution/control-large-negative-vs-man.1");
const TABLE: &[u8] = include_bytes!("fixtures/execution/unsupported-table.1");
const TABLE_VERTICAL_CONTINUATION: &[u8] =
    include_bytes!("fixtures/execution/table-vertical-continuation.1");
const TABLE_WITH_EMPTY_CELL: &[u8] = br#".TH PROBE 1 "September 14, 2026" "ManT" "Manual"
.SH DESCRIPTION
.TS
l l l.
left		third
.TE
"#;
const TABLE_WITH_WORD_SPACING: &[u8] = br#".TH PROBE 1 "September 14, 2026" "ManT" "Manual"
.SH DESCRIPTION
.TS
l.
left alpha
.TE
.B outside
.I words
"#;
const EQUATION: &[u8] = include_bytes!("fixtures/execution/unsupported-equation.1");
const EXECUTED_SO: &[u8] = include_bytes!("fixtures/execution/executed-so.1");
const INACTIVE_SO: &[u8] = include_bytes!("fixtures/execution/inactive-so.1");
const DEVICE_ROLES: &[u8] = include_bytes!("fixtures/execution/device-roles.1");
const ANNOTATED_MAN: &[u8] = include_bytes!("fixtures/execution/annotated-man.1");
const ANNOTATED_MDOC: &[u8] = include_bytes!("fixtures/execution/annotated-mdoc.1");
const REFERENCES_MAN: &[u8] = include_bytes!("fixtures/execution/references-man.1");
const REFERENCES_MDOC: &[u8] = include_bytes!("fixtures/execution/references-mdoc.1");
const WRAPPED_REFERENCE_MDOC: &[u8] = include_bytes!("fixtures/execution/wrapped-reference-mdoc.1");
const NESTED_REFERENCE_MAN: &[u8] = include_bytes!("fixtures/execution/nested-reference-man.1");
const HEADING_EXECUTION_MDOC: &[u8] = include_bytes!("fixtures/execution/heading-execution-mdoc.1");
const HEADING_EXECUTION_MAN: &[u8] = include_bytes!("fixtures/execution/heading-execution-man.1");
const INLINE_ANNOTATIONS_MDOC: &[u8] =
    include_bytes!("fixtures/execution/inline-annotations-mdoc.1");
const INLINE_ANNOTATIONS_MAN: &[u8] = include_bytes!("fixtures/execution/inline-annotations-man.1");
const MDOC_LIST_LIFECYCLE: &[u8] = include_bytes!("fixtures/execution/mdoc-list-lifecycle.1");
const MAN_DEFINITION_LIFECYCLE: &[u8] =
    include_bytes!("fixtures/execution/man-definition-lifecycle.1");
const DEFINITION_WIDE_MDOC: &[u8] = include_bytes!("fixtures/execution/definition-wide-mdoc.1");
const DEFINITION_WIDE_MAN: &[u8] = include_bytes!("fixtures/execution/definition-wide-man.1");
const DEFINITION_FRACTIONAL_MDOC: &[u8] =
    include_bytes!("fixtures/execution/definition-responsive-fractional.1");
const DEFINITION_PHASE_MARGIN_MDOC: &[u8] =
    include_bytes!("fixtures/execution/definition-phase-margin-mdoc.1");
const DEFINITION_HEAD_LINE_LENGTH_MDOC: &[u8] =
    include_bytes!("fixtures/execution/definition-head-line-length-mdoc.1");
const DEFINITION_FIT_FRACTIONAL_MDOC: &[u8] =
    include_bytes!("fixtures/execution/definition-fractional-fit-mdoc.1");
const DEFINITION_FIT_TRAILING_TAB_MDOC: &[u8] =
    include_bytes!("fixtures/execution/definition-trailing-tab-mdoc.1");
const DEFINITION_FIT_CUSTOM_TAB_MDOC: &[u8] =
    include_bytes!("fixtures/execution/definition-custom-tab-mdoc.1");
const DEFINITION_LOGICAL_TABS_MDOC: &[u8] =
    include_bytes!("fixtures/execution/logical-tabs-mdoc.1");
const DEFINITION_LOGICAL_TABS_WORD_END_MDOC: &[u8] =
    include_bytes!("fixtures/execution/logical-tabs-word-end-mdoc.1");
const DEFINITION_LOGICAL_TABS_MULTI_ROW_MDOC: &[u8] =
    include_bytes!("fixtures/execution/logical-tabs-multi-row-mdoc.1");
const LOGICAL_TABS_INHERITED_OFFSET_MAN: &[u8] =
    include_bytes!("fixtures/execution/logical-tabs-inherited-offset-man.1");
const LOGICAL_TABS_FRACTIONAL_ROW_ORIGIN_MDOC: &[u8] =
    include_bytes!("fixtures/execution/logical-tabs-fractional-row-origin-mdoc.1");
const DEFINITION_FIT_LONG_LOGICAL_MDOC: &[u8] =
    include_bytes!("fixtures/execution/definition-long-logical-fit-mdoc.1");
const DEFINITION_FIT_WORD_END_MDOC: &[u8] =
    include_bytes!("fixtures/execution/definition-word-end-fit-mdoc.1");
const DEFINITION_FIT_NBSP_MDOC: &[u8] =
    include_bytes!("fixtures/execution/definition-nbsp-fit-mdoc.1");
const DISPLAY_CONTROL_MAN: &[u8] = include_bytes!("fixtures/execution/display-control-man.1");
const DISPLAY_CONTROL_MDOC: &[u8] = include_bytes!("fixtures/execution/display-control-mdoc.1");
const DISPLAY_CONTROL_MDOC_SYNOPSIS_OVERLAP: &[u8] =
    include_bytes!("fixtures/execution/display-control-mdoc-synopsis-overlap.1");

fn execute(name: &str, input_format: InputFormat, source: &[u8]) -> libmandoc_rs::ExecutionReport {
    Parser::new(ParseOptions::default())
        .with_input_format(input_format)
        .with_mdoc_operating_system("ManT")
        .unwrap()
        .execute_bytes(name, source, ExecutionLimits::default())
        .unwrap()
}

fn execution_keys(node: &Node, keys: &mut Vec<u32>) {
    keys.push(
        node.execution_node_key
            .expect("executed AST node must expose its report-local key"),
    );
    for child in &node.children {
        execution_keys(child, keys);
    }
}

fn assert_ast_report_identity(node: &Node, report: &libmandoc_rs::NativeExecutionReport) {
    let key = node
        .execution_node_key
        .expect("executed AST node must expose its report-local key");
    let execution = &report.nodes()[key as usize];
    assert_eq!(execution.key.0, key);
    assert_eq!(execution.line, node.line, "line mismatch for node {key}");
    assert_eq!(
        execution.column, node.column,
        "column mismatch for node {key}"
    );
    assert_eq!(
        execution.macro_name.as_deref(),
        node.macro_name.as_deref(),
        "macro mismatch for node {key}"
    );
    for child in &node.children {
        assert_ast_report_identity(child, report);
    }
}

fn pool(report: &NativeExecutionReport, range: libmandoc_rs::PoolRange) -> &[u8] {
    report.pool_bytes(range).expect("valid report pool range")
}

fn has_ancestor_macro(
    report: &NativeExecutionReport,
    mut node: libmandoc_rs::ExecutionNodeKey,
    expected: &str,
) -> bool {
    loop {
        let current = &report.nodes()[node.0 as usize];
        if current.macro_name.as_deref() == Some(expected) {
            return true;
        }
        let Some(parent) = current.parent else {
            return false;
        };
        node = parent;
    }
}

fn ast_node_by_execution_key(node: &Node, key: u32) -> Option<&Node> {
    if node.execution_node_key == Some(key) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| ast_node_by_execution_key(child, key))
}

#[test]
fn one_native_session_owns_matching_ast_and_execution_nodes() {
    for (name, format, source) in [
        ("plain-man.1", InputFormat::Man, MAN),
        ("plain-mdoc.1", InputFormat::Mdoc, MDOC),
    ] {
        let report = execute(name, format, source);
        assert!(!report.execution.nodes().is_empty());
        assert!(!report.execution.atoms().is_empty());
        assert!(!report.execution.fragments().is_empty());
        assert_eq!(report.execution.sources().len(), 1);
        assert_eq!(report.execution.sources()[0].path.to_string_lossy(), name);
        assert_eq!(report.execution.nodes()[0].key.0, 0);
        assert_ast_report_identity(&report.document.root, &report.execution);
        let mut ast_keys = Vec::new();
        execution_keys(&report.document.root, &mut ast_keys);
        assert_eq!(
            ast_keys,
            report
                .execution
                .nodes()
                .iter()
                .map(|node| node.key.0)
                .collect::<Vec<_>>()
        );
        assert!(
            report
                .execution
                .nodes()
                .iter()
                .enumerate()
                .all(|(index, node)| { usize::try_from(node.key.0).ok() == Some(index) })
        );
        assert!(
            report
                .execution
                .atoms()
                .windows(2)
                .all(|atoms| { atoms[0].sequence < atoms[1].sequence })
        );
        assert!(!report.execution.buffer_generations().is_empty());
        assert!(
            report
                .execution
                .buffer_generations()
                .iter()
                .all(|generation| {
                    generation.extent <= generation.capacity
                        && generation.open_sequence < generation.close_sequence
                })
        );
        assert!(report.execution.flushes().iter().all(|flush| {
            let extent =
                report.execution.buffer_generations()[flush.buffer_generation as usize].extent;
            flush.scanned.start == flush.accepted.start
                && flush.accepted == flush.consumed
                && flush.accepted.end == flush.tail_discarded.start
                && flush.tail_discarded.end == flush.remaining.start
                && flush.accepted.end <= flush.scanned.end
                && flush.scanned.end <= extent
                && flush.remaining.end == extent
                && flush.sequence < flush.outcome_sequence
        }));
    }

    let report = execute("plain-mdoc.1", InputFormat::Mdoc, MDOC);
    let plain = report
        .execution
        .atoms()
        .iter()
        .find(|atom| {
            atom.role == AtomRole::Authored
                && atom.operand.is_some_and(|range| {
                    report.execution.pool_bytes(range) == Some(b"Plain".as_slice())
                })
        })
        .expect("authored Plain operand");
    let origin = &report.execution.nodes()[plain.node.expect("authored word node").0 as usize];
    assert_eq!(origin.macro_name, None);
}

#[test]
fn native_buffer_mutations_retain_origins_replacements_and_annotations() {
    // Fixed-CVS oracle: `term.c::term_word`, `encode1`, and `buffer_store`
    // execute these words as one ordered stream.  The UTF-8 reference output
    // retains the visible overlay/overstrike results while the native report
    // exposes the consumed zero-width, decoration, and replacement mutations.
    let report = execute("buffer-mutations.1", InputFormat::Mdoc, BUFFER_MUTATIONS);
    let word = |operand: &[u8]| {
        report
            .execution
            .words()
            .iter()
            .find(|word| report.execution.pool_bytes(word.operand) == Some(operand))
            .unwrap_or_else(|| panic!("missing formatter word {operand:?}"))
    };
    let atoms = |operand: &[u8]| {
        let word = word(operand);
        &report.execution.atoms()[word.atoms.start as usize..word.atoms.end as usize]
    };

    let overlay = atoms(br"OVERLAY-A\zX");
    assert!(overlay.iter().any(|atom| {
        atom.kind == AtomKind::Glyph
            && atom.role == AtomRole::Authored
            && atom.display_scalar == u32::from('X')
    }));
    let overlay_tail = atoms(b"B");
    assert!(
        overlay_tail.iter().any(|atom| {
            atom.kind == AtomKind::Backspace && atom.role == AtomRole::FontDecoration
        })
    );

    let zero_width = atoms(br"ZERO-A\&B");
    assert!(zero_width.iter().any(|atom| {
        atom.kind == AtomKind::ZeroWidth && atom.disposition == AtomDisposition::Consumed
    }));

    let styled = atoms(br"\fBB\fP");
    assert!(styled.iter().any(|atom| {
        atom.kind == AtomKind::Glyph
            && atom.role == AtomRole::Authored
            && atom.font == ExecutionFont::Bold
    }));
    assert!(
        styled
            .iter()
            .any(|atom| { atom.kind == AtomKind::Glyph && atom.role == AtomRole::FontDecoration })
    );

    let overstrike = atoms(br"OVERSTRIKE-\o'AB'");
    assert!(
        overstrike.iter().any(|atom| {
            atom.kind == AtomKind::Backspace && atom.role == AtomRole::FontDecoration
        })
    );

    let replacement = atoms(br"REPLACE-A\h'-1n'B");
    let replaced = replacement
        .iter()
        .find(|atom| atom.disposition == AtomDisposition::Replaced)
        .expect("negative motion must overwrite the preceding native buffer slot");
    let replacing = &report.execution.atoms()[replaced.replaced_by.unwrap().0 as usize];
    assert_eq!(replacing.slot, replaced.slot);
    assert_eq!(replacing.display_scalar, u32::from('B'));

    let reference = report
        .execution
        .references()
        .iter()
        .find(|reference| reference.kind == ExecutionReferenceKind::ExternalUri)
        .expect("native Lk reference");
    let linked = word(br"LINK-A\zX");
    assert!(linked.atoms.start >= reference.atoms.start);
    assert!(linked.atoms.end <= reference.atoms.end);
    assert!(linked.wrapper.is_some());

    let generations = report.execution.buffer_generations();
    assert!(generations.windows(2).any(|pair| {
        pair[0].buffer == pair[1].buffer
            && pair[0].generation + 1 == pair[1].generation
            && pair[0].close_sequence < pair[1].open_sequence
    }));
}

#[test]
fn empty_formatter_words_remain_addressable_without_buffer_atoms() {
    // Fixed-CVS oracle: `man_term.c::pre_alternate` calls `term_word()` for
    // every .BR operand, including the explicit empty operand; its reference
    // output is `AB`, so the word is observable without inventing an atom.
    let report = execute("empty-word-man.1", InputFormat::Man, EMPTY_WORD_MAN);
    let empty = report
        .execution
        .words()
        .iter()
        .find(|word| report.execution.pool_bytes(word.operand) == Some(b""))
        .expect("the native .BR handler executes the explicit empty operand");
    assert_eq!(empty.role, AtomRole::Authored);
    assert!(empty.atoms.is_empty());
    let node = empty.node.expect("empty authored operand node");
    assert_eq!(report.execution.nodes()[node.0 as usize].line, 3);
}

#[test]
fn reports_native_flush_branches_and_device_decoration_roles() {
    let plain = execute("plain-man.1", InputFormat::Man, MAN);
    assert!(
        plain
            .execution
            .flushes()
            .iter()
            .any(|flush| flush.outcome == FlushOutcome::Wrapped),
        "the fixed 78-column execution must expose its native wrap branch"
    );
    assert!(
        plain
            .execution
            .flushes()
            .iter()
            .any(|flush| flush.outcome == FlushOutcome::Exhausted)
    );

    let decorated = execute("device-roles.1", InputFormat::Man, DEVICE_ROLES);
    assert!(
        decorated
            .execution
            .fragments()
            .iter()
            .any(|fragment| fragment.role == FragmentRole::Content)
    );
    assert!(
        decorated
            .execution
            .fragments()
            .iter()
            .any(|fragment| fragment.role == FragmentRole::MarginDecoration)
    );
    assert!(
        decorated
            .execution
            .fragments()
            .iter()
            .any(|fragment| fragment.role == FragmentRole::PageDecoration)
    );
    let margin = decorated
        .execution
        .fragments()
        .iter()
        .find(|fragment| fragment.role == FragmentRole::MarginDecoration)
        .expect("native .mc margin fragment");
    let margin_atom = &decorated.execution.atoms()[margin.atoms[0].0 as usize];
    assert_eq!(margin_atom.role, AtomRole::Authored);
    assert_eq!(
        margin_atom
            .operand
            .and_then(|range| decorated.execution.pool_bytes(range)),
        Some(b"|".as_slice())
    );
    let margin_node =
        &decorated.execution.nodes()[margin_atom.node.expect(".mc operand node").0 as usize];
    assert_eq!(margin_node.line, 3);
    assert_eq!(margin.node, margin_atom.node);
    assert_ne!(
        margin_node.line, 4,
        "margin must not inherit VISIBLE's owner"
    );
}

fn assert_consumed_tail(
    execution: &NativeExecutionReport,
    buffer_generation: u32,
    tail: &std::ops::Range<u32>,
) {
    assert!(execution.atoms().iter().any(|atom| {
        atom.buffer_generation == Some(buffer_generation)
            && atom.slot.is_some_and(|slot| tail.contains(&slot))
            && atom.disposition == AtomDisposition::Consumed
    }));
}

fn assert_tab_fates(execution: &NativeExecutionReport) {
    let literal_tabs = execution
        .atoms()
        .iter()
        .filter(|atom| atom.kind == AtomKind::Tab)
        .collect::<Vec<_>>();
    assert_eq!(literal_tabs.len(), 3);
    assert!(
        literal_tabs
            .iter()
            .all(|atom| atom.role == AtomRole::Authored)
    );
    for disposition in [AtomDisposition::Consumed, AtomDisposition::TrailingDiscard] {
        assert!(execution.atoms().iter().any(|atom| {
            atom.kind == AtomKind::TabReference
                && atom.role == AtomRole::MacroGenerated
                && atom.disposition == disposition
        }));
    }
}

fn assert_overstrike_slot_reuse(execution: &NativeExecutionReport) {
    let shrunk_buffer_flushes = execution
        .flushes()
        .iter()
        .filter(|flush| {
            flush.node.is_some_and(|node| {
                let owner = &execution.nodes()[node.0 as usize];
                owner.source == 0
                    && owner.line == 33
                    && owner.kind == NodeKind::Body
                    && owner.macro_name.as_deref() == Some("SH")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        shrunk_buffer_flushes.len(),
        1,
        "the SHRUNK BUFFER section body must own exactly one native flush"
    );
    let shrunk = shrunk_buffer_flushes[0];
    let generation = &execution.buffer_generations()[shrunk.buffer_generation as usize];
    // Fixed CVS `term.c::term_word()` handles `ESCAPE_OVERSTRIKE` by deleting
    // trailing backspace/blank pairs and reducing `lastcol` before a later
    // `buffer_store()` can reuse the cleared slot. Consequently, the current
    // logical end can be below this generation's historical high-water extent,
    // while `term_fill()` still scans only the live range.
    assert!(generation.extent > shrunk.remaining.end);
    assert!(shrunk.scanned.end <= shrunk.remaining.end);

    let mut reused_slots = std::collections::BTreeMap::<u32, Vec<_>>::new();
    for atom in execution
        .atoms()
        .iter()
        .filter(|atom| atom.buffer_generation == Some(shrunk.buffer_generation))
    {
        reused_slots
            .entry(atom.slot.unwrap())
            .or_default()
            .push(atom);
    }
    let reused = reused_slots
        .values()
        .find(|atoms| {
            atoms.len() > 1
                && atoms[..atoms.len() - 1]
                    .iter()
                    .any(|atom| atom.disposition == AtomDisposition::TrailingDiscard)
        })
        .expect("overstrike contraction must reuse one cleared native slot");
    assert_eq!(
        reused.last().unwrap().disposition,
        AtomDisposition::Emitted,
        "the newest atom is the live occupant attributed to the flush"
    );
}

#[test]
fn reports_exact_native_field_consumption() {
    let report = execute(
        "field-consumption-man.1",
        InputFormat::Man,
        FIELD_CONSUMPTION_MAN,
    );
    let execution = &report.execution;

    // Pinned CVS `man_term.c::pre_TP()` converts each tag width into terminal
    // basic units before `term_flushln()` calls `term_field()`.  The fixed
    // UTF-8 device uses 24 BU per `n`, so the report must retain 4n/8n/12n
    // exactly.  The long first word is accepted even when it overruns its
    // field; it is not mistaken for an unconsumed successor.
    let tp_heads = execution
        .flushes()
        .iter()
        .filter_map(|flush| {
            let node = flush.node.map(|key| &execution.nodes()[key.0 as usize])?;
            (node.macro_name.as_deref() == Some("TP") && node.kind == NodeKind::Head)
                .then_some((node.line, flush))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tp_heads
            .iter()
            .map(|(line, flush)| (*line, flush.field_bu))
            .collect::<Vec<_>>(),
        [(5, 96), (8, 192), (11, 288)]
    );
    assert!(tp_heads.iter().all(|(_, flush)| {
        flush.content_bu > flush.field_bu
            && flush.outcome == FlushOutcome::Exhausted
            && !flush.accepted.is_empty()
            && flush.remaining.is_empty()
    }));

    // `term_fill()` scans through the first word that no longer fits, but
    // `term_field()` consumes only the accepted prefix.  A later wrap skips
    // precisely the separating field atom and resumes at the first surviving
    // slot on the globally next flush.
    let wrapped = execution
        .flushes()
        .iter()
        .enumerate()
        .filter(|(_, flush)| flush.outcome == FlushOutcome::Wrapped)
        .collect::<Vec<_>>();
    assert_eq!(wrapped.len(), 2);
    for (index, flush) in wrapped {
        assert!(flush.scanned.end > flush.accepted.end);
        assert_eq!(flush.tail_discarded.end - flush.tail_discarded.start, 1);
        assert!(!flush.remaining.is_empty());
        let successor = &execution.flushes()[index + 1];
        assert_eq!(successor.buffer_generation, flush.buffer_generation);
        assert_eq!(successor.scanned.start, flush.remaining.start);
        assert!(flush.outcome_sequence < successor.sequence);
        assert_eq!(successor.taboff_before, flush.taboff_after);
        assert_consumed_tail(execution, flush.buffer_generation, &flush.tail_discarded);
    }

    // Table columns are flushed in alternating device order.  Deferred
    // continuation therefore resumes at the next flush for the same native
    // buffer generation, not necessarily at the globally adjacent record.
    let deferred = execution
        .flushes()
        .iter()
        .enumerate()
        .filter(|(_, flush)| flush.outcome == FlushOutcome::DeferredColumn)
        .collect::<Vec<_>>();
    assert_eq!(deferred.len(), 2);
    for (index, flush) in deferred {
        let (successor_index, successor) = execution.flushes()[index + 1..]
            .iter()
            .enumerate()
            .find(|(_, candidate)| candidate.buffer_generation == flush.buffer_generation)
            .map(|(offset, successor)| (index + 1 + offset, successor))
            .expect("deferred native field successor");
        assert!(successor_index > index + 1, "columns must be interleaved");
        assert_eq!(successor.scanned.start, flush.remaining.start);
        assert!(flush.outcome_sequence < successor.sequence);
        assert_eq!(successor.taboff_before, flush.taboff_after);
        assert!(flush.taboff_after > flush.taboff_before);
        assert_consumed_tail(execution, flush.buffer_generation, &flush.tail_discarded);
    }

    let empty = execution
        .flushes()
        .iter()
        .find(|flush| flush.outcome == FlushOutcome::NoContent)
        .expect("whitespace-only native field");
    assert!(empty.accepted.is_empty());
    assert!(empty.consumed.is_empty());
    assert!(empty.remaining.is_empty());
    assert_eq!(empty.tail_discarded, 0..4);
    assert!(empty.fragments.is_empty());

    assert_tab_fates(execution);
    assert!(execution.flushes().iter().any(|flush| {
        flush.outcome == FlushOutcome::Exhausted
            && !flush.accepted.is_empty()
            && flush.visual_after > 0
    }));
    assert_overstrike_slot_reuse(execution);
}

#[test]
fn reports_native_control_boundaries_and_zero_output_effects() {
    for (name, format, source, expected) in [
        (
            "control-effects-man.1",
            InputFormat::Man,
            CONTROL_EFFECTS_MAN,
            [5, 6, 7, 8, 9, 10, 13, 15, 16, 18, 20, 22, 24],
        ),
        (
            "control-effects-mdoc.1",
            InputFormat::Mdoc,
            CONTROL_EFFECTS_MDOC,
            [8, 9, 10, 11, 12, 13, 16, 18, 19, 21, 23, 25, 27],
        ),
    ] {
        // The complete fixtures are lint-clean against the pinned CVS binary
        // (SHA-256 f06ba20baedee4adc5914fa023bf02812645b41249077204849d465c08c02f59).
        // `roff_term.c` dispatches every request exactly once and `term.c`
        // supplies the nested newline/vspace/endline primitives asserted here.
        let report = execute(name, format, source);
        assert_native_control_fixture(&report.execution, expected, name);
    }
}

fn assert_native_control_fixture(
    execution: &NativeExecutionReport,
    expected: [u32; 13],
    name: &str,
) {
    let record_lengths = (
        u32::try_from(execution.atoms().len()).expect("bounded atom count"),
        u32::try_from(execution.fragments().len()).expect("bounded fragment count"),
        u32::try_from(execution.flushes().len()).expect("bounded flush count"),
        u32::try_from(execution.boundaries().len()).expect("bounded boundary count"),
        u32::try_from(execution.geometry().len()).expect("bounded geometry count"),
        u32::try_from(execution.wrappers().len()).expect("bounded wrapper count"),
    );
    assert!(!execution.controls().is_empty());
    assert_eq!(execution.controls().len(), expected.len());
    assert_eq!(
        execution
            .controls()
            .iter()
            .map(|control| execution.nodes()[control.node.0 as usize].line)
            .collect::<Vec<_>>(),
        expected
    );
    assert!(execution.controls().iter().all(|control| {
        let node = &execution.nodes()[control.node.0 as usize];
        let wrapper = &execution.wrappers()[control.wrapper as usize];
        control.enter_sequence < control.leave_sequence
            && wrapper
                .node
                .is_some_and(|owner| node_is_within(execution, control.node, owner))
            && wrapper.enter_sequence < control.enter_sequence
            && wrapper.leave_sequence > control.leave_sequence
            && control.atoms.end <= record_lengths.0
            && control.fragments.end <= record_lengths.1
            && control.flushes.end <= record_lengths.2
            && control.boundaries.end <= record_lengths.3
            && control.geometry.end <= record_lengths.4
            && control.wrappers.end <= record_lengths.5
            && matches!(
                (control.request, node.macro_name.as_deref()),
                (ExecutionControlRequest::Break, Some("br"))
                    | (ExecutionControlRequest::MarginCharacter, Some("mc"))
                    | (ExecutionControlRequest::VerticalSpace, Some("sp"))
                    | (ExecutionControlRequest::TemporaryIndent, Some("ti"))
                    | (ExecutionControlRequest::NoFill, Some("nf"))
                    | (ExecutionControlRequest::Fill, Some("fi"))
            )
    }));

    assert_control_requests(execution, name);

    assert!(execution.boundaries().iter().all(|boundary| {
        boundary.enter_sequence < boundary.leave_sequence
            && (boundary.request != BoundaryRequest::DeviceEndline
                || boundary.direct_device_lines == 0)
    }));
    let first_sp = execution
        .controls()
        .iter()
        .find(|control| {
            control.request == ExecutionControlRequest::VerticalSpace
                && execution.nodes()[control.node.0 as usize].line == expected[2]
        })
        .expect("first .sp control");
    assert_eq!(
        execution
            .boundaries()
            .iter()
            .filter(|boundary| {
                boundary.control == Some(first_sp.key)
                    && boundary.request == BoundaryRequest::VerticalSpace
                    && boundary.direct_device_lines == 1
            })
            .count(),
        2,
        ".sp 2 must execute two direct native vertical-space boundaries"
    );

    let empty_margin = execution
        .controls()
        .iter()
        .find(|control| control.request == ExecutionControlRequest::MarginCharacter)
        .expect("empty margin request");
    assert!(empty_margin.atoms.is_empty());
    assert!(empty_margin.fragments.is_empty());

    let delayed_margin_word = execution
        .words()
        .iter()
        .find(|word| pool(execution, word.operand) == b"|")
        .expect("delayed authored margin character word");
    let wrapper = &execution.wrappers()[delayed_margin_word.wrapper.unwrap() as usize];
    assert!(
        delayed_margin_word.node.is_some_and(|origin| {
            wrapper
                .node
                .is_some_and(|owner| origin != owner && !node_is_within(execution, origin, owner))
        }),
        "the report must keep delayed .mc source ownership separate from its later execution wrapper"
    );
}

fn assert_control_requests(execution: &NativeExecutionReport, name: &str) {
    let requests = execution
        .controls()
        .iter()
        .map(|control| control.request)
        .collect::<Vec<_>>();
    for request in [
        ExecutionControlRequest::Break,
        ExecutionControlRequest::MarginCharacter,
        ExecutionControlRequest::VerticalSpace,
        ExecutionControlRequest::TemporaryIndent,
        ExecutionControlRequest::NoFill,
        ExecutionControlRequest::Fill,
    ] {
        assert!(requests.contains(&request), "missing {request:?} in {name}");
    }
}

#[test]
fn nested_controls_keep_child_origins_and_direct_boundary_ownership() {
    // The complete fixture is lint-clean under the pinned CVS binary. Its
    // tree makes `sp` and `ti` children of `ce`, and `br` a child of `rj`;
    // `roff_term_pre_ce()` dispatches those children without opening another
    // terminal node wrapper.
    let report = execute("control-nested-man.1", InputFormat::Man, CONTROL_NESTED_MAN);
    let execution = &report.execution;
    let controls = execution.controls();
    assert_eq!(controls.len(), 5);
    for (parent_request, child_request) in [
        (
            ExecutionControlRequest::Center,
            ExecutionControlRequest::VerticalSpace,
        ),
        (
            ExecutionControlRequest::Center,
            ExecutionControlRequest::TemporaryIndent,
        ),
        (
            ExecutionControlRequest::RightJustify,
            ExecutionControlRequest::Break,
        ),
    ] {
        let parent = controls
            .iter()
            .find(|control| control.request == parent_request)
            .expect("parent control");
        let child = controls
            .iter()
            .find(|control| control.request == child_request)
            .expect("nested child control");
        assert_eq!(child.parent, Some(parent.key));
        assert_eq!(child.wrapper, parent.wrapper);
        assert_ne!(child.node, parent.node);
        assert!(node_is_within(execution, child.node, parent.node));
        assert!(execution.boundaries().iter().all(|boundary| {
            boundary.control != Some(child.key)
                || (boundary.enter_sequence > child.enter_sequence
                    && boundary.leave_sequence < child.leave_sequence)
        }));
    }
    let mut direct_keys = execution
        .boundaries()
        .iter()
        .filter_map(|boundary| boundary.control.map(|control| (boundary.key, control)))
        .collect::<Vec<_>>();
    direct_keys.sort_unstable();
    direct_keys.dedup();
    assert_eq!(
        direct_keys.len(),
        execution
            .boundaries()
            .iter()
            .filter(|b| b.control.is_some())
            .count()
    );
}

#[test]
fn generated_controls_retain_typed_source_provenance() {
    // Fixed CVS diagnoses a filled-mode blank line and inserts a source-less
    // `.sp`; the execution report must not present it as an authored request.
    let report = execute(
        "control-generated-man.1",
        InputFormat::Man,
        CONTROL_GENERATED_MAN,
    );
    let generated = report
        .execution
        .controls()
        .iter()
        .find(|control| {
            control.request == ExecutionControlRequest::VerticalSpace
                && report.execution.nodes()[control.node.0 as usize]
                    .flags
                    .contains(libmandoc_rs::ExecutionNodeFlags::GENERATED)
        })
        .expect("generated blank-line spacing control");
    assert!(generated.enter_sequence < generated.leave_sequence);
}

fn node_is_within(
    report: &NativeExecutionReport,
    mut node: libmandoc_rs::ExecutionNodeKey,
    owner: libmandoc_rs::ExecutionNodeKey,
) -> bool {
    loop {
        if node == owner {
            return true;
        }
        let Some(parent) = report.nodes()[node.0 as usize].parent else {
            return false;
        };
        node = parent;
    }
}

#[test]
fn zero_output_control_work_is_bounded_and_reentrant() {
    // The complete fixture is lint-clean under the pinned CVS binary and its
    // only visible DESCRIPTION content is `VISIBLE`.  The two long `.ta`
    // requests still execute native parsing and tab-list replacement work.
    let baseline = execute("control-work-man.1", InputFormat::Man, CONTROL_WORK_MAN);
    let tab_controls = baseline
        .execution
        .controls()
        .iter()
        .filter(|control| control.request == ExecutionControlRequest::TabStops)
        .collect::<Vec<_>>();
    assert_eq!(tab_controls.len(), 2);
    assert!(tab_controls.iter().all(|control| {
        control.atoms.is_empty()
            && control.fragments.is_empty()
            && control.enter_sequence < control.leave_sequence
    }));

    let error = Parser::new(ParseOptions::default())
        .with_input_format(InputFormat::Man)
        .with_mdoc_operating_system("ManT")
        .unwrap()
        .execute_bytes(
            "control-work-man.1",
            CONTROL_WORK_MAN,
            ExecutionLimits {
                max_work: baseline.execution.work_units() - 1,
                ..ExecutionLimits::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::Budget);
    assert_eq!(
        execute("control-work-man.1", InputFormat::Man, CONTROL_WORK_MAN)
            .execution
            .controls()
            .iter()
            .filter(|control| control.request == ExecutionControlRequest::TabStops)
            .count(),
        2
    );
}

#[test]
fn oversized_control_distances_do_not_overflow_native_state() {
    // The complete input was run through the pinned CVS binary before this
    // assertion was written; it renders both BEFORE and AFTER and reports no
    // lint diagnostic.  ManT additionally rejects the non-finite native
    // distance conversion instead of relying on an undefined float-to-int cast.
    let report = execute(
        "control-numeric-overflow-man.1",
        InputFormat::Man,
        CONTROL_NUMERIC_OVERFLOW_MAN,
    );
    let control = report
        .execution
        .controls()
        .iter()
        .find(|control| control.request == ExecutionControlRequest::VerticalSpace)
        .expect("oversized .sp still has an authored control fact");
    assert!(control.enter_sequence < control.leave_sequence);
    assert!(report.execution.fragments().iter().any(|fragment| {
        fragment.atoms.iter().any(|key| {
            char::from_u32(report.execution.atoms()[key.0 as usize].display_scalar) == Some('B')
        })
    }));
}

#[test]
fn finite_control_distances_are_checked_after_parsing_their_units() {
    // The exact fixture was linted and rendered by pinned CVS before this
    // assertion was written. `40000u` is a representable basic-unit indent,
    // and `-40000v` is a representable vertical debt; neither may be rejected
    // merely because font-size units use a larger conversion factor.
    let report = execute(
        "control-numeric-units-man.1",
        InputFormat::Man,
        CONTROL_NUMERIC_UNITS_MAN,
    );
    let execution = &report.execution;
    let controls = execution.controls();
    assert_eq!(
        controls
            .iter()
            .filter(|control| control.request == ExecutionControlRequest::TemporaryIndent)
            .count(),
        1
    );
    assert_eq!(
        controls
            .iter()
            .filter(|control| control.request == ExecutionControlRequest::VerticalSpace)
            .count(),
        2
    );
    let indent = controls
        .iter()
        .find(|control| control.request == ExecutionControlRequest::TemporaryIndent)
        .expect("representable basic-unit indent");
    assert!(indent.temporary_indent_after > indent.temporary_indent_before);
    let negative_space = controls
        .iter()
        .find(|control| {
            control.request == ExecutionControlRequest::VerticalSpace
                && execution.nodes()[control.node.0 as usize].line == 5
        })
        .expect("representable negative vertical-space request");
    assert!(negative_space.skip_vertical_after > negative_space.skip_vertical_before);
}

#[test]
fn finite_vertical_underflow_remains_zero_instead_of_using_the_default_distance() {
    // Pinned CVS renders BEFORE and AFTER on adjacent device lines: strtod(3)
    // underflow is a valid finite zero, not a parse failure that selects the
    // default one-line `.sp` distance.
    let report = execute(
        "control-numeric-underflow-man.1",
        InputFormat::Man,
        CONTROL_NUMERIC_UNDERFLOW_MAN,
    );
    let control = report
        .execution
        .controls()
        .iter()
        .find(|control| control.request == ExecutionControlRequest::VerticalSpace)
        .expect("underflowed vertical-space request");
    assert_eq!(control.line_after - control.line_before, 1);
    assert_eq!(control.skip_vertical_before, control.skip_vertical_after);
}

#[test]
fn large_negative_vertical_distance_accumulates_as_debt_without_overflow() {
    // Pinned CVS applies the large negative `v` distance as vertical debt;
    // the following positive request does not emit a device line.  The
    // conversion must therefore use the vertical factor at `term_vspan()`,
    // not a conservative horizontal factor at parse time.
    let report = execute(
        "control-large-negative-vs-man.1",
        InputFormat::Man,
        CONTROL_LARGE_NEGATIVE_VS_MAN,
    );
    let controls = report
        .execution
        .controls()
        .iter()
        .filter(|control| control.request == ExecutionControlRequest::VerticalSpace)
        .collect::<Vec<_>>();
    assert_eq!(controls.len(), 2);
    assert!(controls[0].skip_vertical_after > controls[0].skip_vertical_before);
    assert_eq!(controls[1].line_before, controls[1].line_after);
    assert_eq!(
        controls[1].skip_vertical_after,
        controls[0].skip_vertical_after - 2
    );
}

#[test]
fn reports_native_definition_fonts_references_and_actual_targets() {
    for (name, format, source, owner_macro) in [
        ("annotated-man.1", InputFormat::Man, ANNOTATED_MAN, "UR"),
        ("annotated-mdoc.1", InputFormat::Mdoc, ANNOTATED_MDOC, "Lk"),
    ] {
        let report = execute(name, format, source);
        let execution = &report.execution;
        let reference = execution
            .references()
            .iter()
            .find(|reference| reference.kind == ExecutionReferenceKind::ExternalUri)
            .expect("native URI reference");
        assert_eq!(
            pool(execution, reference.primary),
            b"https://example.org/manual"
        );
        assert_eq!(reference.secondary, None);
        assert_eq!(
            execution.nodes()[reference.owner_node.0 as usize]
                .macro_name
                .as_deref(),
            Some(owner_macro)
        );
        assert_eq!(
            ast_node_by_execution_key(&report.document.root, reference.target_node.0)
                .and_then(|node| node.text.as_deref()),
            Some("https://example.org/manual")
        );
        let label_operands = execution.atoms()
            [reference.atoms.start as usize..reference.atoms.end as usize]
            .iter()
            .filter_map(|atom| atom.operand)
            .map(|range| pool(execution, range))
            .collect::<Vec<_>>();
        assert!(label_operands.iter().any(|operand| {
            operand
                .windows(b"linked".len())
                .any(|part| part == b"linked")
        }));
        assert!(
            label_operands
                .iter()
                .any(|operand| operand.windows(b"label".len()).any(|part| part == b"label"))
        );
        assert!(!label_operands.contains(&b"https://example.org/manual".as_slice()));
        assert!(execution.atoms().iter().any(|atom| {
            atom.font == ExecutionFont::Underline
                && atom.operand.is_some_and(|range| {
                    pool(execution, range)
                        .windows(b"styled".len())
                        .any(|part| part == b"styled")
                })
        }));
        assert!(execution.flushes().iter().any(|flush| {
            flush.node.is_some_and(|node| {
                has_ancestor_macro(
                    execution,
                    node,
                    if format == InputFormat::Man {
                        "TP"
                    } else {
                        "It"
                    },
                )
            }) && flush.outcome != FlushOutcome::NoContent
        }));
    }

    let mdoc = execute("annotated-mdoc.1", InputFormat::Mdoc, ANNOTATED_MDOC);
    let anchor = mdoc
        .execution
        .anchors()
        .iter()
        .find(|anchor| pool(&mdoc.execution, anchor.target) == b"custom-target")
        .expect("moved native .Tg target");
    let owner = &mdoc.execution.nodes()[anchor.node.0 as usize];
    assert_eq!(owner.macro_name.as_deref(), Some("It"));
    assert_eq!(
        owner.kind,
        NodeKind::Head,
        "target must attach to the actual It head"
    );
    assert!(anchor.device_line > 0);
    assert!(
        usize::try_from(anchor.atom_cursor)
            .is_ok_and(|cursor| cursor <= mdoc.execution.atoms().len())
    );
    assert!(
        usize::try_from(anchor.fragment_cursor)
            .is_ok_and(|cursor| cursor <= mdoc.execution.fragments().len())
    );
}

#[test]
fn reports_typed_reference_components_and_exact_label_intervals() {
    for (name, format, source, expected) in [
        (
            "references-man.1",
            InputFormat::Man,
            REFERENCES_MAN,
            vec![
                (
                    ExecutionReferenceKind::Manual,
                    b"printf".as_slice(),
                    Some(b"3".as_slice()),
                ),
                (
                    ExecutionReferenceKind::ExternalUri,
                    b"https://example.org/path".as_slice(),
                    None,
                ),
                (
                    ExecutionReferenceKind::Email,
                    b"one@example.org".as_slice(),
                    None,
                ),
            ],
        ),
        (
            "references-mdoc.1",
            InputFormat::Mdoc,
            REFERENCES_MDOC,
            vec![
                (
                    ExecutionReferenceKind::ExternalUri,
                    b"https://example.org/path".as_slice(),
                    None,
                ),
                (
                    ExecutionReferenceKind::Email,
                    b"one@example.org".as_slice(),
                    None,
                ),
                (
                    ExecutionReferenceKind::Email,
                    b"two@example.org".as_slice(),
                    None,
                ),
                (
                    ExecutionReferenceKind::Manual,
                    b"printf".as_slice(),
                    Some(b"3".as_slice()),
                ),
            ],
        ),
    ] {
        let report = execute(name, format, source);
        let actual = report
            .execution
            .references()
            .iter()
            .map(|reference| {
                (
                    reference.kind,
                    pool(&report.execution, reference.primary),
                    reference
                        .secondary
                        .map(|range| pool(&report.execution, range)),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        assert!(report.execution.references().iter().all(|reference| {
            reference.atoms.start < reference.atoms.end
                && reference.atoms.end as usize <= report.execution.atoms().len()
                && report.execution.atoms()[reference.atoms.start as usize].role
                    != AtomRole::ImplicitSpace
                && report.execution.atoms()
                    [reference.execution_atoms.start as usize..reference.atoms.start as usize]
                    .iter()
                    .all(|atom| atom.role == AtomRole::ImplicitSpace)
        }));
        let manual = report
            .execution
            .references()
            .iter()
            .find(|reference| reference.kind == ExecutionReferenceKind::Manual)
            .unwrap();
        let operands = report.execution.atoms()
            [manual.atoms.start as usize..manual.atoms.end as usize]
            .iter()
            .filter_map(|atom| atom.operand)
            .map(|range| pool(&report.execution, range))
            .collect::<Vec<_>>();
        assert!(operands.contains(&b"printf".as_slice()));
        assert!(operands.contains(&b"3".as_slice()));
        assert!(!operands.contains(&b",".as_slice()));
    }
}

#[test]
fn reports_sx_authored_identity_separately_from_executed_display() {
    // Fixed CVS `html_make_id()` deroffs the authored `.Sx` phrase while
    // `mdoc_term.c::termp_sm_pre()` independently removes display spacing.
    // The exact fixture was checked with both the pinned UTF-8 and HTML
    // renderers before this assertion was written: its href is
    // `#NEXT_SECTION`, while its visible label is `NEXTSECTION`.
    let report = execute(
        "heading-execution-mdoc.1",
        InputFormat::Mdoc,
        HEADING_EXECUTION_MDOC,
    );
    let reference = report
        .execution
        .references()
        .iter()
        .find(|reference| reference.kind == ExecutionReferenceKind::SameDocumentSection)
        .expect("native Sx reference");
    assert_eq!(pool(&report.execution, reference.primary), b"NEXT SECTION");
    assert_eq!(reference.secondary, None);
    assert_eq!(
        report.execution.nodes()[reference.owner_node.0 as usize]
            .macro_name
            .as_deref(),
        Some("Sx")
    );
    assert_eq!(
        report.execution.nodes()[reference.target_node.0 as usize].kind,
        NodeKind::Text
    );
    let operands = report.execution.atoms()
        [reference.atoms.start as usize..reference.atoms.end as usize]
        .iter()
        .filter_map(|atom| atom.operand)
        .map(|range| pool(&report.execution, range))
        .collect::<Vec<_>>();
    assert!(operands.contains(&b"NEXT".as_slice()));
    assert!(operands.contains(&b"SECTION".as_slice()));
}

#[test]
fn reports_typed_heading_scopes_with_authored_phrases() {
    for (name, format, source, expected) in [
        (
            "heading-execution-mdoc.1",
            InputFormat::Mdoc,
            HEADING_EXECUTION_MDOC,
            vec![
                (ExecutionHeadingKind::MdocSection, "NAME"),
                (ExecutionHeadingKind::MdocSection, "Alice"),
                (ExecutionHeadingKind::MdocSection, "$ Alice"),
                (ExecutionHeadingKind::MdocSection, r"PARENT\fI"),
                (ExecutionHeadingKind::MdocSubsection, r"CHILD\fI"),
                (ExecutionHeadingKind::MdocSection, "NEXT SECTION"),
                (ExecutionHeadingKind::MdocSection, "NEXTSECTION"),
                (ExecutionHeadingKind::MdocSection, "AUTHORS"),
                (ExecutionHeadingKind::MdocSection, "Alice"),
                (ExecutionHeadingKind::MdocSection, "SPACING"),
                (ExecutionHeadingKind::MdocSection, "NEXT"),
                (ExecutionHeadingKind::MdocSection, "SEE ALSO"),
            ],
        ),
        (
            "heading-execution-man.1",
            InputFormat::Man,
            HEADING_EXECUTION_MAN,
            vec![
                (ExecutionHeadingKind::ManSection, "NAME"),
                (ExecutionHeadingKind::ManSection, r"NEXT\fI"),
                (ExecutionHeadingKind::ManSubsection, r"SUB\fI"),
            ],
        ),
    ] {
        let report = execute(name, format, source);
        let headings = report
            .execution
            .wrappers()
            .iter()
            .filter(|wrapper| wrapper.kind == ExecutionWrapperKind::Heading)
            .collect::<Vec<_>>();
        assert_eq!(headings.len(), expected.len(), "{name}");
        for (wrapper, (kind, phrase)) in headings.into_iter().zip(expected) {
            assert_eq!(wrapper.heading_kind, Some(kind), "{name}");
            assert_eq!(
                wrapper
                    .target
                    .and_then(|range| report.execution.pool_bytes(range)),
                Some(phrase.as_bytes()),
                "{name}"
            );
            assert!(wrapper.enter_sequence < wrapper.leave_sequence, "{name}");
            assert!(wrapper.enter_atom <= wrapper.leave_atom, "{name}");
            let parent = &report.execution.wrappers()[wrapper.parent.unwrap() as usize];
            assert_eq!(parent.kind, ExecutionWrapperKind::Node, "{name}");
            assert_eq!(parent.node, wrapper.node, "{name}");
            assert!(parent.enter_sequence < wrapper.enter_sequence, "{name}");
            assert!(parent.leave_sequence > wrapper.leave_sequence, "{name}");
        }
    }
}

#[test]
fn heading_authored_phrases_obey_report_budgets_and_cancellation() {
    // The complete fixture was rendered by the pinned CVS binary before this
    // assertion was added.  Fixed `roff.c::deroff()` recursively visits every
    // authored heading fragment; the execution observer performs the same
    // normalization directly into its bounded pool instead of allocating an
    // unmetered temporary string.
    let baseline = execute(
        "heading-execution-mdoc.1",
        InputFormat::Mdoc,
        HEADING_EXECUTION_MDOC,
    );
    for limits in [
        ExecutionLimits {
            max_work: baseline.execution.work_units() - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_pool_bytes: baseline.execution.pool_len() as u64 - 1,
            ..ExecutionLimits::default()
        },
    ] {
        let error = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes("heading-execution-mdoc.1", HEADING_EXECUTION_MDOC, limits)
            .unwrap_err();
        assert_eq!(error.kind, ExecutionErrorKind::Budget);
    }

    let cancellation = ExecutionCancellation::new();
    cancellation.cancel();
    let error = Parser::new(ParseOptions::default())
        .with_input_format(InputFormat::Mdoc)
        .with_mdoc_operating_system("ManT")
        .unwrap()
        .execute_bytes_with_cancellation(
            "heading-cancelled.1",
            HEADING_EXECUTION_MDOC,
            ExecutionLimits::default(),
            &cancellation,
        )
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::Cancelled);

    let next = execute(
        "heading-after-cancellation.1",
        InputFormat::Mdoc,
        HEADING_EXECUTION_MDOC,
    );
    assert!(
        next.execution
            .wrappers()
            .iter()
            .any(|wrapper| wrapper.kind == ExecutionWrapperKind::Heading)
    );
}

#[test]
#[allow(clippy::too_many_lines)] // One matrix asserts the complete native It lifecycle.
fn reports_exact_mdoc_list_item_lifecycles() {
    // Before these assertions were written, the complete fixture was run with
    // the pinned CVS terminal, tree, and lint renderers.  In
    // mdoc_term.c::termp_it_pre/post(), one formatter lifecycle surrounds the
    // It block's head, every column body, and its final post-handler cleanup;
    // Xo/Xc only changes the syntax nesting inside that lifecycle.
    let report = execute(
        "mdoc-list-lifecycle.1",
        InputFormat::Mdoc,
        MDOC_LIST_LIFECYCLE,
    );
    let execution = &report.execution;
    let items = execution
        .wrappers()
        .iter()
        .filter(|wrapper| wrapper.kind == ExecutionWrapperKind::MdocListItem)
        .collect::<Vec<_>>();
    let expected = [
        ExecutionMdocListKind::Bullet,
        ExecutionMdocListKind::Bullet,
        ExecutionMdocListKind::Dash,
        ExecutionMdocListKind::Hyphen,
        ExecutionMdocListKind::Enum,
        ExecutionMdocListKind::Enum,
        ExecutionMdocListKind::Item,
        ExecutionMdocListKind::Tag,
        ExecutionMdocListKind::Tag,
        ExecutionMdocListKind::Hang,
        ExecutionMdocListKind::Overhang,
        ExecutionMdocListKind::Inset,
        ExecutionMdocListKind::Diagnostic,
        ExecutionMdocListKind::Column,
        ExecutionMdocListKind::Column,
    ];
    assert_eq!(items.len(), expected.len());
    for (index, (item, expected_kind)) in items.iter().zip(expected).enumerate() {
        assert_eq!(item.mdoc_list_kind, Some(expected_kind), "item {index}");
        assert_eq!(item.mdoc_list_compact(), index < 2, "item {index}");
        assert!(item.enter_sequence < item.leave_sequence, "item {index}");
        assert!(item.enter_atom <= item.leave_atom, "item {index}");
        let node = item.node.expect("list item node");
        let origin = &execution.nodes()[node.0 as usize];
        assert_eq!(origin.kind, NodeKind::Block);
        assert_eq!(origin.macro_name.as_deref(), Some("It"));
        let parent = &execution.wrappers()[item.parent.unwrap() as usize];
        assert_eq!(parent.kind, ExecutionWrapperKind::Node);
        assert_eq!(parent.node, Some(node));
        assert!(parent.enter_sequence < item.enter_sequence);
        assert!(parent.leave_sequence > item.leave_sequence);

        let child_nodes = execution
            .nodes()
            .iter()
            .filter(|candidate| candidate.parent == Some(node))
            .collect::<Vec<_>>();
        assert!(child_nodes.iter().any(|child| child.kind == NodeKind::Head));
        assert!(child_nodes.iter().any(|child| child.kind == NodeKind::Body));
        assert!(child_nodes.iter().all(|child| {
            execution.wrappers().iter().any(|wrapper| {
                wrapper.kind == ExecutionWrapperKind::Node
                    && wrapper.node == Some(child.key)
                    && wrapper.enter_sequence > item.enter_sequence
                    && wrapper.leave_sequence < item.leave_sequence
            })
        }));
    }

    for item in items.iter().filter(|item| {
        matches!(
            item.mdoc_list_kind,
            Some(
                ExecutionMdocListKind::Bullet
                    | ExecutionMdocListKind::Dash
                    | ExecutionMdocListKind::Hyphen
                    | ExecutionMdocListKind::Enum
            )
        )
    }) {
        assert!(
            execution.atoms()[item.enter_atom as usize..item.leave_atom as usize]
                .iter()
                .any(|atom| atom.role == AtomRole::MacroGenerated),
            "fixed CVS termp_it_pre() must own each generated marker"
        );
    }

    let anchor_owner = |target: &[u8]| {
        let anchor = execution
            .anchors()
            .iter()
            .find(|anchor| pool(execution, anchor.target) == target)
            .expect("fixture anchor");
        items.iter().position(|item| {
            item.enter_sequence < anchor.sequence && anchor.sequence < item.leave_sequence
        })
    };
    assert_eq!(anchor_owner(b"before-bullet"), None);
    assert_eq!(anchor_owner(b"bullet-first"), Some(0));
    assert_eq!(anchor_owner(b"bullet-empty"), Some(0));
    assert_eq!(anchor_owner(b"tag-target"), Some(7));
    assert_eq!(anchor_owner(b"after-lists"), None);

    let empty_column = items
        .iter()
        .rev()
        .find(|item| item.mdoc_list_kind == Some(ExecutionMdocListKind::Column))
        .expect("empty final column row");
    assert_eq!(empty_column.enter_atom, empty_column.leave_atom);

    for item in &items {
        let is_definition = matches!(
            item.mdoc_list_kind,
            Some(
                ExecutionMdocListKind::Tag
                    | ExecutionMdocListKind::Hang
                    | ExecutionMdocListKind::Overhang
                    | ExecutionMdocListKind::Inset
                    | ExecutionMdocListKind::Diagnostic
            )
        );
        assert_eq!(item.definition.is_some(), is_definition);
        let Some(definition) = item.definition else {
            continue;
        };
        let owner = item.node.expect("definition item owner");
        assert_eq!(
            execution.nodes()[definition.head.node.0 as usize].parent,
            Some(owner)
        );
        assert_eq!(
            execution.nodes()[definition.body.node.0 as usize].parent,
            Some(owner)
        );
        assert_eq!(
            execution.nodes()[definition.head.node.0 as usize].kind,
            NodeKind::Head
        );
        assert_eq!(
            execution.nodes()[definition.body.node.0 as usize].kind,
            NodeKind::Body
        );
        assert!(definition.cell_bu > 0);
        assert!(definition.head.maxrmargin_bu > definition.head.offset_bu);
        assert!(definition.body.maxrmargin_bu > definition.body.offset_bu);
        assert!(definition.head.sequence < definition.body.sequence);
        match item.mdoc_list_kind.unwrap() {
            ExecutionMdocListKind::Tag => {
                assert!(definition.head_may_stay_open_if_field_fits);
                assert!(definition.count_trailing_space);
                assert!(definition.wrapped_continuation_uses_field_end);
            }
            ExecutionMdocListKind::Hang => {
                assert!(definition.head_stays_open_unconditionally);
                assert!(definition.head_may_stay_open_if_field_fits);
            }
            ExecutionMdocListKind::Overhang | ExecutionMdocListKind::Inset => {
                assert!(!definition.head_stays_open_unconditionally);
                assert!(!definition.head_may_stay_open_if_field_fits);
            }
            ExecutionMdocListKind::Diagnostic => {
                assert!(!definition.head_stays_open_unconditionally);
                assert!(definition.head_may_stay_open_if_field_fits);
                assert!(definition.wrapped_continuation_uses_field_end);
            }
            _ => unreachable!(),
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)] // One matrix asserts the complete native man block lifecycle.
fn reports_exact_man_block_lifecycles() {
    // Before these assertions were written, this exact fixture was run with
    // the pinned CVS `-Tlint`, `-Tutf8`, and `-Ttree` renderers.  Fixed
    // `man_term.c::print_man_node()` surrounds the complete pre/children/post
    // execution of each block; `pre_TP()` owns the paragraph boundary while
    // `TQ` deliberately omits that boundary, and `.PD 0` changes distance but
    // never merges the independently parsed block owners.
    let report = execute(
        "man-definition-lifecycle.1",
        InputFormat::Man,
        MAN_DEFINITION_LIFECYCLE,
    );
    let execution = &report.execution;
    let blocks = execution
        .wrappers()
        .iter()
        .filter(|wrapper| wrapper.kind == ExecutionWrapperKind::ManBlock)
        .collect::<Vec<_>>();
    let expected = [
        ExecutionManBlockKind::TaggedParagraph,
        ExecutionManBlockKind::AdditionalTag,
        ExecutionManBlockKind::TaggedParagraph,
        ExecutionManBlockKind::Paragraph,
        ExecutionManBlockKind::TaggedParagraph,
        ExecutionManBlockKind::TaggedParagraph,
        ExecutionManBlockKind::IndentedParagraph,
        ExecutionManBlockKind::HangingParagraph,
        ExecutionManBlockKind::RelativeIndent,
        ExecutionManBlockKind::TaggedParagraph,
        ExecutionManBlockKind::Paragraph,
        ExecutionManBlockKind::ParagraphP,
        ExecutionManBlockKind::ParagraphLp,
    ];
    assert_eq!(blocks.len(), expected.len());
    for (index, (block, expected_kind)) in blocks.iter().zip(expected).enumerate() {
        assert_eq!(block.man_block_kind, Some(expected_kind), "block {index}");
        assert!(block.enter_sequence < block.leave_sequence, "block {index}");
        let node = block.node.expect("man block node");
        let origin = &execution.nodes()[node.0 as usize];
        assert_eq!(origin.kind, NodeKind::Block);
        let parent = &execution.wrappers()[block.parent.unwrap() as usize];
        assert_eq!(parent.kind, ExecutionWrapperKind::Node);
        assert_eq!(parent.node, Some(node));
        assert!(parent.enter_sequence < block.enter_sequence);
        assert!(block.leave_sequence < parent.leave_sequence);
        assert!(
            execution
                .wrappers()
                .iter()
                .filter(|candidate| {
                    candidate.kind == ExecutionWrapperKind::Node
                        && candidate.node.is_some_and(|child| {
                            execution.nodes()[child.0 as usize].parent == Some(node)
                        })
                })
                .all(|child| {
                    block.enter_sequence < child.enter_sequence
                        && child.leave_sequence < block.leave_sequence
                })
        );

        let is_definition = matches!(
            expected_kind,
            ExecutionManBlockKind::IndentedParagraph
                | ExecutionManBlockKind::TaggedParagraph
                | ExecutionManBlockKind::AdditionalTag
        );
        assert_eq!(block.definition.is_some(), is_definition, "block {index}");
        if let Some(definition) = block.definition {
            assert_eq!(
                execution.nodes()[definition.head.node.0 as usize].parent,
                Some(node)
            );
            assert_eq!(
                execution.nodes()[definition.body.node.0 as usize].parent,
                Some(node)
            );
            assert_eq!(
                execution.nodes()[definition.head.node.0 as usize].kind,
                NodeKind::Head
            );
            assert_eq!(
                execution.nodes()[definition.body.node.0 as usize].kind,
                NodeKind::Body
            );
            assert!(definition.cell_bu > 0);
            assert!(definition.head.maxrmargin_bu > definition.head.offset_bu);
            assert!(definition.body.maxrmargin_bu > definition.body.offset_bu);
            assert!(definition.head.sequence < definition.body.sequence);
            assert!(!definition.head_stays_open_unconditionally);
            assert!(definition.head_may_stay_open_if_field_fits);
            assert_eq!(
                definition.count_trailing_space,
                matches!(
                    expected_kind,
                    ExecutionManBlockKind::TaggedParagraph | ExecutionManBlockKind::AdditionalTag
                )
            );
            assert!(!definition.wrapped_continuation_uses_field_end);
        }
    }

    let relative = blocks
        .iter()
        .find(|block| block.man_block_kind == Some(ExecutionManBlockKind::RelativeIndent))
        .unwrap();
    let nested = blocks
        .iter()
        .find(|block| {
            block.man_block_kind == Some(ExecutionManBlockKind::TaggedParagraph)
                && block.enter_sequence > relative.enter_sequence
                && block.leave_sequence < relative.leave_sequence
        })
        .expect("TP nested inside RS execution scope");
    assert!(nested.enter_atom >= relative.enter_atom);
    assert!(nested.leave_atom <= relative.leave_atom);
}

#[test]
fn accepts_native_definition_fields_beyond_the_device_margin() {
    // Both exact inputs were rendered with the pinned CVS `-Tutf8` and
    // `-Tlint` frontends before this assertion was written.  Fixed CVS
    // `termp_it_pre()` and `pre_TP()` allow a declared field right margin to
    // exceed `maxrmargin`; `term_flushln()` still uses `maxrmargin` as the
    // NOBREAK wrap target rather than treating it as a geometry invariant.
    for (name, format, source) in [
        (
            "definition-wide-mdoc.1",
            InputFormat::Mdoc,
            DEFINITION_WIDE_MDOC,
        ),
        (
            "definition-wide-man.1",
            InputFormat::Man,
            DEFINITION_WIDE_MAN,
        ),
    ] {
        let report = execute(name, format, source);
        let definition = report
            .execution
            .wrappers()
            .iter()
            .find_map(|wrapper| wrapper.definition)
            .expect("wide definition contract");
        assert!(definition.head.rmargin_bu > definition.head.maxrmargin_bu);
        assert!(definition.body.offset_bu > definition.body.maxrmargin_bu);
    }
}

#[test]
fn preserves_fractional_native_definition_origins() {
    // This exact input was rendered with the pinned CVS `-Tutf8` and `-Tlint`
    // frontends before this assertion was written.  `ascii_advance()` rounds
    // each absolute device destination independently, so consumers need both
    // raw origins rather than a pre-rounded relative delta.
    let report = execute(
        "definition-responsive-fractional.1",
        InputFormat::Mdoc,
        DEFINITION_FRACTIONAL_MDOC,
    );
    let definition = report
        .execution
        .wrappers()
        .iter()
        .find_map(|wrapper| {
            let definition = wrapper.definition?;
            let owner = wrapper.node?;
            let node = ast_node_by_execution_key(&report.document.root, owner.0)?;
            (node.macro_name.as_deref() == Some("It") && node.line == 9).then_some(definition)
        })
        .expect("fractional It definition contract");
    assert_ne!(definition.head.offset_bu % definition.cell_bu, 0);
    assert_eq!(definition.body.offset_bu % definition.cell_bu, 0);
    assert_eq!(definition.head.offset_bu, 132);
    assert_eq!(definition.body.offset_bu, 192);
    assert_eq!(definition.cell_bu, 24);
    assert_eq!(definition.head.offset_bu / definition.cell_bu, 5);
    assert_eq!(definition.body.offset_bu / definition.cell_bu, 8);
    assert_eq!(
        (definition.body.offset_bu - definition.head.offset_bu) / definition.cell_bu,
        2
    );
}

#[test]
fn preserves_phase_local_definition_device_margins() {
    // This exact input was rendered with pinned CVS `-Ttree`, `-Tutf8`, and
    // `-Tlint` before this assertion was written.  `roff_term_pre_ll()` calls
    // `term_setwidth()` while executing the It HEAD, so the BODY phase must
    // carry the changed device margin instead of being rejected or conflated
    // with the earlier HEAD snapshot.
    let report = execute(
        "definition-phase-margin-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_PHASE_MARGIN_MDOC,
    );
    let definition = report
        .execution
        .wrappers()
        .iter()
        .find_map(|wrapper| wrapper.definition)
        .expect("phase-local definition contract");
    assert_eq!(definition.head.maxrmargin_bu, 78 * definition.cell_bu);
    assert_eq!(definition.body.maxrmargin_bu, 20 * definition.cell_bu);
}

fn definition_head_flush(
    report: &libmandoc_rs::ExecutionReport,
) -> (
    libmandoc_rs::ExecutionDefinitionContract,
    &libmandoc_rs::ExecutionFlush,
) {
    let definition = report
        .execution
        .wrappers()
        .iter()
        .find_map(|wrapper| wrapper.definition)
        .expect("definition execution contract");
    let flush = report
        .execution
        .flushes()
        .iter()
        .rev()
        .find(|flush| {
            flush.node == Some(definition.head.node) && flush.outcome == FlushOutcome::Exhausted
        })
        .unwrap_or_else(|| {
            panic!(
                "exhausted definition head flush; definition={definition:?}; candidates={:?}",
                report
                    .execution
                    .flushes()
                    .iter()
                    .map(|flush| (
                        flush.sequence,
                        flush.node,
                        flush.outcome,
                        flush.logical_content_bu,
                        flush.logical_field_bu,
                        flush.logical_origin_bu,
                    ))
                    .collect::<Vec<_>>()
            )
        });
    (definition, flush)
}

#[test]
fn measures_definition_head_after_executed_line_length_change() {
    // The exact fixture was checked with pinned CVS `-Tlint` and `-Tutf8`
    // before this assertion was written.  `roff_term_pre_ll()` calls
    // `term_setwidth()` immediately, then `termp_it_post()` reaches the HEAD
    // `term_flushln()`: the exact HEAD flush therefore uses the changed
    // device margin while preserving the field origin computed by the list
    // handler.
    let report = execute(
        "definition-head-line-length-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_HEAD_LINE_LENGTH_MDOC,
    );
    let (definition, flush) = definition_head_flush(&report);
    assert_eq!(flush.node, Some(definition.head.node));
    assert_eq!(flush.logical_field_bu, 10 * definition.cell_bu);
    assert_eq!(flush.logical_origin_bu, 5 * definition.cell_bu);
    assert_eq!(flush.maxrmargin_bu, 34 * definition.cell_bu);
}

#[test]
fn measures_definition_fit_in_native_basic_units_without_rewriting_input() {
    // Each exact fixture was rendered with the pinned CVS `-Tutf8` and
    // `-Tlint` before these assertions were written.  The observer reuses the
    // scanner in `term.c::term_fill()` and `term_tab_next()`: fractional field
    // widths and configured tabs must remain in BU instead of being rounded
    // into an invented reader-side tab cycle.
    let fractional = execute(
        "definition-fractional-fit-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_FIT_FRACTIONAL_MDOC,
    );
    let (definition, flush) = definition_head_flush(&fractional);
    assert_eq!(definition.cell_bu, 24);
    assert_eq!(flush.logical_content_bu, 24);
    assert_eq!(definition.head.rmargin_bu - definition.head.offset_bu, 60);

    let trailing = execute(
        "definition-trailing-tab-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_FIT_TRAILING_TAB_MDOC,
    );
    let (_, flush) = definition_head_flush(&trailing);
    assert_eq!(flush.logical_content_bu, 10 * 24);

    let custom = execute(
        "definition-custom-tab-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_FIT_CUSTOM_TAB_MDOC,
    );
    let (_, flush) = definition_head_flush(&custom);
    assert_eq!(flush.logical_content_bu, 9 * 24);
    assert_eq!(
        flush.logical_tabs,
        [ExecutionLogicalTab {
            row_epoch: 0,
            destination_bu: 8 * 24,
        }]
    );

    // The exact multi-tab fixture was checked with pinned CVS `-Tlint`,
    // `-Ttree`, and `-Tutf8` before this assertion was written.  In
    // `term.c::term_fill_mode()`, each literal tab passes its current BU
    // position through `term_tab_next()` using the `.ta 3n 7n 12n` list, so
    // the observer must retain both destinations rather than only the final
    // aggregate width.
    let multiple = execute(
        "logical-tabs-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_LOGICAL_TABS_MDOC,
    );
    let (_, flush) = definition_head_flush(&multiple);
    assert_eq!(
        flush.logical_tabs,
        [
            ExecutionLogicalTab {
                row_epoch: 0,
                destination_bu: 3 * 24,
            },
            ExecutionLogicalTab {
                row_epoch: 0,
                destination_bu: 7 * 24,
            },
        ]
    );
    assert_eq!(flush.logical_content_bu, 8 * 24);
    let non_breaking = execute(
        "definition-nbsp-fit-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_FIT_NBSP_MDOC,
    );
    let (_, flush) = definition_head_flush(&non_breaking);
    assert_eq!(flush.logical_content_bu, 10 * 24);
    assert!(!flush.logical_forced_break);
}

#[test]
fn logical_tab_records_obey_budgets_and_release_on_failure() {
    let initial = execute(
        "logical-tabs-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_LOGICAL_TABS_MDOC,
    );
    let exact_records = initial.execution.record_count();
    Parser::default()
        .with_input_format(InputFormat::Mdoc)
        .with_mdoc_operating_system("ManT")
        .unwrap()
        .execute_bytes(
            "logical-tabs-mdoc.1",
            DEFINITION_LOGICAL_TABS_MDOC,
            ExecutionLimits {
                max_records: exact_records,
                ..ExecutionLimits::default()
            },
        )
        .expect("logical-tab records fit the exact sealed-record budget");
    let error = Parser::default()
        .with_input_format(InputFormat::Mdoc)
        .with_mdoc_operating_system("ManT")
        .unwrap()
        .execute_bytes(
            "logical-tabs-mdoc.1",
            DEFINITION_LOGICAL_TABS_MDOC,
            ExecutionLimits {
                max_records: exact_records - 1,
                ..ExecutionLimits::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::Budget);

    let recovered = execute(
        "logical-tabs-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_LOGICAL_TABS_MDOC,
    );
    assert_eq!(
        definition_head_flush(&recovered).1.logical_tabs,
        [
            ExecutionLogicalTab {
                row_epoch: 0,
                destination_bu: 3 * 24,
            },
            ExecutionLogicalTab {
                row_epoch: 0,
                destination_bu: 7 * 24,
            },
        ]
    );
}

#[test]
fn tracks_logical_tab_rows_across_executed_word_end_breaks() {
    // The pure pinned-CVS renderer (SHA-256 `3468a220...`) renders the first
    // fixture as an `A` row followed by `B    C`, then BODY.  In
    // `term_fill_mode()`, the word-end break is realized at the following
    // space; only the observer's row-local tab position resets, while the
    // fit-width `vis` continues over the complete logical field.
    let word_end = execute(
        "logical-tabs-word-end-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_LOGICAL_TABS_WORD_END_MDOC,
    );
    let (_, flush) = definition_head_flush(&word_end);
    assert!(flush.logical_forced_break);
    assert_eq!(
        flush.logical_tabs,
        [ExecutionLogicalTab {
            row_epoch: 1,
            destination_bu: 5 * 24,
        }]
    );

    // The matching two-break oracle renders three rows: `A`, `B    C`, and
    // `D     E`.  Responsive consumers need row epochs rather than one
    // cumulative destination list. Each row-local visual position starts from
    // zero while the native inter-row tab offset follows `term_flushln()`'s
    // `vbr + one cell` settlement before applying the active `.ta` stops.
    let multi_row = execute(
        "logical-tabs-multi-row-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_LOGICAL_TABS_MULTI_ROW_MDOC,
    );
    let (_, flush) = definition_head_flush(&multi_row);
    assert!(flush.logical_forced_break);
    assert_eq!(
        flush.logical_tabs,
        [
            ExecutionLogicalTab {
                row_epoch: 1,
                destination_bu: 5 * 24,
            },
            ExecutionLogicalTab {
                row_epoch: 2,
                destination_bu: 3 * 24,
            },
        ]
    );
}

#[test]
fn logical_tabs_follow_inherited_native_tab_offsets() {
    // The pure pinned-CVS renderer (SHA-256 `3468a220...`) renders the first
    // tbl column as four five-cell physical rows: `AAAA`, `AAAA`, `AAAA`,
    // then `AAAAB`.  Each `TERMP_MULTICOL` yield re-enters
    // `term_flushln()` with the preceding segment's `tcol->taboff`; the
    // logical scan must use that inherited offset instead of restarting the
    // active `.ta 3n 7n 12n` list at zero.
    let report = execute(
        "logical-tabs-inherited-offset-man.1",
        InputFormat::Man,
        LOGICAL_TABS_INHERITED_OFFSET_MAN,
    );
    let observed = report
        .execution
        .flushes()
        .iter()
        .filter(|flush| !flush.logical_tabs.is_empty())
        .map(|flush| {
            (
                flush.outcome,
                flush.taboff_before,
                flush.logical_tabs.as_slice(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        observed,
        [
            (
                FlushOutcome::DeferredColumn,
                0,
                &[ExecutionLogicalTab {
                    row_epoch: 0,
                    destination_bu: 19 * 24,
                }][..],
            ),
            (
                FlushOutcome::DeferredColumn,
                5 * 24,
                &[ExecutionLogicalTab {
                    row_epoch: 0,
                    destination_bu: 14 * 24,
                }][..],
            ),
            (
                FlushOutcome::DeferredColumn,
                10 * 24,
                &[ExecutionLogicalTab {
                    row_epoch: 0,
                    destination_bu: 9 * 24,
                }][..],
            ),
            (
                FlushOutcome::Exhausted,
                15 * 24,
                &[ExecutionLogicalTab {
                    row_epoch: 0,
                    destination_bu: 4 * 24,
                }][..],
            ),
        ]
    );
}

#[test]
fn logical_tabs_retain_fractional_hard_row_geometry() {
    // The pure pinned-CVS renderer (SHA-256 `3468a220...`) was run before
    // these assertions.  `termp_it_pre()` establishes a 0.55n head origin
    // and a 7.5n width; `term_flushln()` keeps epoch zero at the logical
    // origin, then uses the BRIND right margin as the continuation origin.
    // The `.ta 4.5n T 4n` stop yields a 60-BU row-relative destination, so
    // consumers must round the absolute 421-BU endpoint before subtracting
    // the rounded 361-BU origin. Both physical fill segments describe the
    // same complete logical field.
    let report = execute(
        "logical-tabs-fractional-row-origin-mdoc.1",
        InputFormat::Mdoc,
        LOGICAL_TABS_FRACTIONAL_ROW_ORIGIN_MDOC,
    );
    let definition = report
        .execution
        .wrappers()
        .iter()
        .find_map(|wrapper| wrapper.definition)
        .expect("fractional definition contract");
    assert_eq!(definition.head.offset_bu, 133);
    assert_eq!(definition.head.rmargin_bu, 361);
    assert!(definition.wrapped_continuation_uses_field_end);

    let head_flushes = report
        .execution
        .flushes()
        .iter()
        .filter(|flush| flush.node == Some(definition.head.node))
        .collect::<Vec<_>>();
    assert_eq!(head_flushes.len(), 2);
    assert_eq!(head_flushes[0].outcome, FlushOutcome::Wrapped);
    assert_eq!(head_flushes[1].outcome, FlushOutcome::Exhausted);
    assert_eq!(
        head_flushes[0].buffer_generation,
        head_flushes[1].buffer_generation
    );
    for flush in head_flushes {
        assert_eq!(flush.logical_origin_bu, 133);
        assert!(flush.logical_forced_break);
        assert_eq!(
            flush.logical_tabs,
            [ExecutionLogicalTab {
                row_epoch: 1,
                destination_bu: 60,
            }]
        );
    }
}

#[test]
fn distinguishes_device_wraps_from_executed_word_end_breaks() {
    // The exact inputs were checked with the pinned CVS renderer before this
    // test was written.  Its `term_fill()` retains the entire long logical
    // field despite fixed-device segments, while `\\p` becomes a hard fact
    // only when `breakline` is actually consumed by a later word boundary.
    let long = execute(
        "definition-long-logical-fit-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_FIT_LONG_LOGICAL_MDOC,
    );
    let (_, flush) = definition_head_flush(&long);
    assert!(flush.logical_content_bu > flush.field_bu);
    assert!(flush.logical_content_bu > flush.effective_content_bu);
    assert!(!flush.logical_forced_break);

    let forced = execute(
        "definition-word-end-fit-mdoc.1",
        InputFormat::Mdoc,
        DEFINITION_FIT_WORD_END_MDOC,
    );
    let definition = forced
        .execution
        .wrappers()
        .iter()
        .find_map(|wrapper| wrapper.definition)
        .expect("definition execution contract");
    let flush = forced
        .execution
        .flushes()
        .iter()
        .find(|flush| flush.node == Some(definition.head.node) && flush.logical_forced_break)
        .expect("executed word-end break flush");
    assert!(flush.logical_forced_break);

    for (name, term) in [
        ("word-end-at-tail.1", "A\\p"),
        ("word-end-before-glyph.1", "A\\pB"),
    ] {
        let source = format!(
            ".Dd September 19, 2026\n.Dt K18BREAK 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width 20n\n.It Xo\n.No \\\"{term}\\\"\n.Xc\nBODY\n.El\n"
        );
        let report = execute(name, InputFormat::Mdoc, source.as_bytes());
        assert!(
            report
                .execution
                .flushes()
                .iter()
                .all(|flush| !flush.logical_forced_break),
            "{name}: {:?}",
            report.execution.flushes()
        );
    }
}

#[test]
fn reports_exact_display_synopsis_literal_and_capture_regions() {
    // Both fixtures were rendered with the pinned CVS `-Tlint`, `-Ttree`,
    // and `-Tutf8` paths before these assertions were written.  The scopes
    // below are the actual handler boundaries in
    // `man_term.c::print_man_node()`, `mdoc_term.c::print_mdoc_node()`, and
    // `roff_term.c::roff_term_pre_ce()`, not section-name inference in Rust.
    for (name, format, source, expected) in [
        (
            "display-control-man.1",
            InputFormat::Man,
            DISPLAY_CONTROL_MAN,
            vec![
                (ExecutionRegionKind::ManSynopsisSection, "SH"),
                (ExecutionRegionKind::ManSynopsisCommand, "SY"),
                (ExecutionRegionKind::ManLiteralBegin, "EX"),
                (ExecutionRegionKind::ManLiteralEnd, "EE"),
                (ExecutionRegionKind::CenteredLines, "ce"),
                (ExecutionRegionKind::RightJustifiedLines, "rj"),
            ],
        ),
        (
            "display-control-mdoc.1",
            InputFormat::Mdoc,
            DISPLAY_CONTROL_MDOC,
            vec![
                (ExecutionRegionKind::MdocSynopsisSection, "Sh"),
                (ExecutionRegionKind::MdocSynopsisItem, "Nm"),
                (ExecutionRegionKind::MdocSynopsisItem, "Op"),
                (ExecutionRegionKind::MdocSynopsisItem, "Fl"),
                (ExecutionRegionKind::MdocSynopsisItem, "Ar"),
                (ExecutionRegionKind::MdocDisplayFilled, "Bd"),
                (ExecutionRegionKind::MdocDisplayUnfilled, "Bd"),
                (ExecutionRegionKind::MdocDisplayLiteral, "Bd"),
                (ExecutionRegionKind::MdocDisplayRagged, "Bd"),
                (ExecutionRegionKind::MdocDisplayCentered, "Bd"),
                (ExecutionRegionKind::MdocDisplayOneLine, "D1"),
                (ExecutionRegionKind::MdocDisplayOneLineLiteral, "Dl"),
                (ExecutionRegionKind::CenteredLines, "ce"),
                (ExecutionRegionKind::RightJustifiedLines, "rj"),
            ],
        ),
    ] {
        let report = execute(name, format, source);
        let regions = report
            .execution
            .wrappers()
            .iter()
            .filter(|wrapper| wrapper.kind == ExecutionWrapperKind::Region)
            .collect::<Vec<_>>();
        assert_eq!(regions.len(), expected.len(), "{name}");
        for (region, (kind, macro_name)) in regions.iter().zip(expected) {
            assert_eq!(region.region_kind, Some(kind), "{name}");
            let node = region.node.expect("region node");
            assert_eq!(
                report.execution.nodes()[node.0 as usize]
                    .macro_name
                    .as_deref(),
                Some(macro_name),
                "{name}"
            );
            assert!(region.enter_sequence < region.leave_sequence, "{name}");
            let parent = &report.execution.wrappers()[region.parent.unwrap() as usize];
            assert_eq!(parent.kind, ExecutionWrapperKind::Node, "{name}");
            assert_eq!(parent.node, Some(node), "{name}");
            assert!(parent.enter_sequence < region.enter_sequence, "{name}");
            assert!(region.leave_sequence < parent.leave_sequence, "{name}");

            if matches!(
                kind,
                ExecutionRegionKind::CenteredLines | ExecutionRegionKind::RightJustifiedLines
            ) {
                let controls = report
                    .execution
                    .controls()
                    .iter()
                    .filter(|control| control.wrapper == region.key)
                    .collect::<Vec<_>>();
                assert!(controls.iter().any(|control| {
                    report.execution.nodes()[control.node.0 as usize]
                        .macro_name
                        .as_deref()
                        == Some(macro_name)
                }));
            }
        }
    }
}

#[test]
fn overlapping_synopsis_and_captured_control_regions_remain_nested() {
    // The exact source was rendered with the pinned CVS lint, tree, and UTF-8
    // devices before this assertion was added.  roff.c marks ce/rj elements
    // NODE_SYNPRETTY while mdoc_state.c keeps the SYNOPSIS state; the same
    // node then enters its ce/rj handler through roff_term.c.
    let report = execute(
        "display-control-mdoc-synopsis-overlap.1",
        InputFormat::Mdoc,
        DISPLAY_CONTROL_MDOC_SYNOPSIS_OVERLAP,
    );
    let regions = report
        .execution
        .wrappers()
        .iter()
        .filter(|wrapper| wrapper.kind == ExecutionWrapperKind::Region)
        .collect::<Vec<_>>();
    assert_eq!(
        regions
            .iter()
            .map(|wrapper| wrapper.region_kind.unwrap())
            .collect::<Vec<_>>(),
        [
            ExecutionRegionKind::MdocSynopsisSection,
            ExecutionRegionKind::MdocSynopsisItem,
            ExecutionRegionKind::CenteredLines,
            ExecutionRegionKind::MdocSynopsisItem,
            ExecutionRegionKind::RightJustifiedLines,
            ExecutionRegionKind::MdocSynopsisItem,
            ExecutionRegionKind::MdocSynopsisItem,
            ExecutionRegionKind::MdocSynopsisItem,
        ]
    );
    for pair in [regions[1..3].as_ref(), regions[3..5].as_ref()] {
        assert_eq!(pair[0].node, pair[1].node);
        assert_eq!(pair[1].parent, Some(pair[0].key));
        assert!(pair[0].enter_sequence < pair[1].enter_sequence);
        assert!(pair[1].leave_sequence < pair[0].leave_sequence);
    }
    let item_region = regions
        .iter()
        .find(|region| {
            region.region_kind == Some(ExecutionRegionKind::MdocSynopsisItem)
                && region.node.is_some_and(|node| {
                    report.execution.nodes()[node.0 as usize]
                        .macro_name
                        .as_deref()
                        == Some("It")
                })
        })
        .expect("synopsis list item region");
    assert_eq!(
        report.execution.wrappers()[item_region.parent.unwrap() as usize].kind,
        ExecutionWrapperKind::MdocListItem
    );
}

#[test]
fn nested_native_references_form_a_parent_linked_execution_stack() {
    let report = execute(
        "nested-reference-man.1",
        InputFormat::Man,
        NESTED_REFERENCE_MAN,
    );
    assert_eq!(report.execution.references().len(), 2);
    let outer = &report.execution.references()[0];
    let inner = &report.execution.references()[1];
    assert_eq!(outer.kind, ExecutionReferenceKind::ExternalUri);
    assert_eq!(inner.kind, ExecutionReferenceKind::Manual);
    assert_eq!(outer.parent, None);
    assert_eq!(inner.parent, Some(outer.key));
    assert!(outer.execution_atoms.start <= inner.execution_atoms.start);
    assert!(outer.execution_atoms.end >= inner.execution_atoms.end);
    assert!(outer.enter_sequence < inner.enter_sequence);
    assert!(outer.leave_sequence > inner.leave_sequence);
}

#[test]
fn one_native_reference_interval_survives_multiple_flushes() {
    let report = execute(
        "wrapped-reference-mdoc.1",
        InputFormat::Mdoc,
        WRAPPED_REFERENCE_MDOC,
    );
    let reference = report
        .execution
        .references()
        .iter()
        .find(|reference| reference.kind == ExecutionReferenceKind::ExternalUri)
        .expect("wrapped URI reference");
    assert_eq!(
        pool(&report.execution, reference.primary),
        b"https://example.org/wrapped"
    );
    let mut device_lines = report
        .execution
        .fragments()
        .iter()
        .filter(|fragment| {
            fragment
                .atoms
                .iter()
                .any(|atom| reference.atoms.start <= atom.0 && atom.0 < reference.atoms.end)
        })
        .map(|fragment| fragment.device_line)
        .collect::<Vec<_>>();
    device_lines.sort_unstable();
    device_lines.dedup();
    assert_eq!(device_lines.len(), 2);
}

#[test]
fn generated_inline_words_retain_native_provenance_and_reference_ranges() {
    // Both complete fixtures were rendered with the pinned CVS binary before
    // this assertion was written.  In fixed `mdoc_validate.c::post_bx()`, the
    // `BSD` spelling and separators are NODE_NOSRC nodes; `termp_fn_pre()`,
    // `termp_quote_pre/post()`, `man_term.c::pre_OP()`, and `post_UR()` emit
    // their punctuation by calling `term_word()` with handler-owned strings.
    for (name, format, source) in [
        (
            "inline-annotations-mdoc.1",
            InputFormat::Mdoc,
            INLINE_ANNOTATIONS_MDOC,
        ),
        (
            "inline-annotations-man.1",
            InputFormat::Man,
            INLINE_ANNOTATIONS_MAN,
        ),
    ] {
        let report = execute(name, format, source);
        let generated = report
            .execution
            .words()
            .iter()
            .filter(|word| word.role == AtomRole::MacroGenerated)
            .map(|word| pool(&report.execution, word.operand))
            .collect::<Vec<_>>();
        for value in ["[", "]"] {
            assert_eq!(
                generated
                    .iter()
                    .filter(|operand| **operand == value.as_bytes())
                    .count(),
                1,
                "{name}: {value} must execute exactly once"
            );
        }

        if format == InputFormat::Mdoc {
            for value in ["(", ",", ")", ";", "BSD"] {
                assert_eq!(
                    generated
                        .iter()
                        .filter(|operand| **operand == value.as_bytes())
                        .count(),
                    1,
                    "{name}: {value} must execute exactly once"
                );
            }
            let bsd = report
                .execution
                .words()
                .iter()
                .find(|word| pool(&report.execution, word.operand) == b"BSD")
                .expect("validator-generated Bx word");
            assert!(
                report.execution.nodes()[bsd.node.expect("generated Bx node").0 as usize]
                    .flags
                    .contains(libmandoc_rs::ExecutionNodeFlags::GENERATED)
            );
            assert_eq!(bsd.role, AtomRole::MacroGenerated);
        } else {
            for value in ["<", ">"] {
                assert_eq!(
                    generated
                        .iter()
                        .filter(|operand| **operand == value.as_bytes())
                        .count(),
                    2,
                    "{name}: each UR/MT wrapper emits one {value}"
                );
            }
        }

        for reference in report.execution.references() {
            if report.execution.nodes()[reference.owner_node.0 as usize]
                .macro_name
                .as_deref()
                == Some("Mt")
            {
                continue;
            }
            for atom in &report.execution.atoms()
                [reference.atoms.start as usize..reference.atoms.end as usize]
            {
                assert_ne!(
                    atom.operand
                        .and_then(|range| report.execution.pool_bytes(range)),
                    Some(pool(&report.execution, reference.primary)),
                    "the target operand is metadata, not part of a labelled reference"
                );
            }
        }
    }
}

#[test]
fn semantic_reference_and_anchor_callbacks_obey_report_budgets() {
    let baseline = execute("annotated-mdoc.1", InputFormat::Mdoc, ANNOTATED_MDOC);
    for limits in [
        ExecutionLimits {
            max_work: baseline.execution.work_units() - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_records: baseline.execution.record_count() - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_pool_bytes: baseline.execution.pool_len() as u64 - 1,
            ..ExecutionLimits::default()
        },
    ] {
        let error = Parser::default()
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes("annotated-mdoc.1", ANNOTATED_MDOC, limits)
            .unwrap_err();
        assert_eq!(error.kind, ExecutionErrorKind::Budget);
    }
}

#[cfg(feature = "serde")]
#[test]
fn report_local_execution_keys_do_not_escape_ast_serialization() {
    let report = execute("plain-man.1", InputFormat::Man, MAN);
    let json = serde_json::to_value(&report.document).unwrap();
    assert!(!json.to_string().contains("execution_node_key"));
}

#[test]
fn exhausted_budget_never_exposes_a_partial_report() {
    let baseline = execute("plain-man.1", InputFormat::Man, MAN);
    let max_depth = baseline
        .execution
        .nodes()
        .iter()
        .map(|node| {
            let mut depth = 1_u64;
            let mut parent = node.parent;
            while let Some(key) = parent {
                depth += 1;
                parent = baseline.execution.nodes()[key.0 as usize].parent;
            }
            depth
        })
        .max()
        .unwrap();
    let exact = [
        ExecutionLimits {
            max_nodes: baseline.execution.nodes().len() as u64,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_depth,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_work: baseline.execution.work_units(),
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_records: baseline.execution.record_count(),
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_report_bytes: baseline.execution.record_bytes(),
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_pool_bytes: baseline.execution.pool_len() as u64,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_buffer_cells: baseline.execution.buffer_cells(),
            ..ExecutionLimits::default()
        },
    ];
    for (index, limits) in exact.into_iter().enumerate() {
        Parser::default()
            .with_input_format(InputFormat::Man)
            .execute_bytes("plain-man.1", MAN, limits)
            .unwrap_or_else(|error| panic!("exact budget case {index} failed: {error:?}"));
    }
    let insufficient = [
        ExecutionLimits {
            max_nodes: baseline.execution.nodes().len() as u64 - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_depth: max_depth - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_work: baseline.execution.work_units() - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_records: baseline.execution.record_count() - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_report_bytes: baseline.execution.record_bytes() - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_pool_bytes: baseline.execution.pool_len() as u64 - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_buffer_cells: baseline.execution.buffer_cells() - 1,
            ..ExecutionLimits::default()
        },
    ];
    for limits in insufficient {
        let error = Parser::default()
            .with_input_format(InputFormat::Man)
            .execute_bytes("plain-man.1", MAN, limits)
            .unwrap_err();
        assert_eq!(error.kind, ExecutionErrorKind::Budget);
        assert!(!error.message.is_empty());
    }
    assert!(
        !execute("plain-man.1", InputFormat::Man, MAN)
            .execution
            .fragments()
            .is_empty()
    );
}

#[test]
fn invalid_execution_limits_are_rejected_as_budgets() {
    for limits in [
        ExecutionLimits {
            max_nodes: 0,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_depth: 0,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_report_bytes: 0,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_records: u64::from(u32::MAX) + 1,
            ..ExecutionLimits::default()
        },
    ] {
        let error = Parser::default()
            .with_input_format(InputFormat::Man)
            .execute_bytes("plain-man.1", MAN, limits)
            .unwrap_err();
        assert_eq!(error.kind, ExecutionErrorKind::Budget);
    }
}

#[test]
fn cancelled_execution_is_atomic_and_does_not_poison_the_next_session() {
    let cancellation = ExecutionCancellation::new();
    assert!(!cancellation.is_cancelled());
    cancellation.cancel();
    assert!(cancellation.is_cancelled());

    let error = Parser::default()
        .with_input_format(InputFormat::Man)
        .execute_bytes_with_cancellation(
            "cancelled.1",
            MAN,
            ExecutionLimits::default(),
            &cancellation,
        )
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::Cancelled);
    assert!(error.message.contains("cancelled"));

    let next = execute("after-cancellation.1", InputFormat::Man, MAN);
    assert!(!next.execution.fragments().is_empty());
}

#[test]
fn concurrent_execution_sessions_keep_reports_and_cancellation_isolated() {
    use std::sync::{Arc, Barrier};

    const WORKERS: usize = 4;
    let start = Arc::new(Barrier::new(WORKERS));
    let workers = (0..WORKERS)
        .map(|worker| {
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                let source =
                    format!(".TH WORKER-{worker} 1\n.SH NAME\nworker-{worker} \\- isolated\n");
                start.wait();
                for round in 0..8 {
                    let report = Parser::default()
                        .with_input_format(InputFormat::Man)
                        .execute_bytes(
                            format!("worker-{worker}-{round}.1"),
                            source.as_bytes(),
                            ExecutionLimits::default(),
                        )
                        .expect("concurrent execution must succeed");
                    assert_eq!(
                        report.document.metadata.title.as_deref(),
                        Some(format!("WORKER-{worker}").as_str())
                    );
                    assert_eq!(report.execution.sources().len(), 1);
                    assert_eq!(
                        report.execution.sources()[0].path.to_string_lossy(),
                        format!("worker-{worker}-{round}.1")
                    );
                }
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().expect("execution worker must not panic");
    }
}

#[test]
fn table_execution_transfers_typed_rows_cells_and_payload_ownership() {
    // The pinned CVS renderer was run at widths 32, 78, and 120 before this
    // assertion was written.  In each case tbl_term.c executes one data row
    // with two left-aligned data cells and renders `left   right`.
    let report = execute("table.1", InputFormat::Man, TABLE);
    let execution = &report.execution;
    assert_eq!(execution.tables().len(), 1);
    assert_eq!(execution.table_rows().len(), 1);
    assert_eq!(execution.table_cells().len(), 2);

    let table = &execution.tables()[0];
    let row = &execution.table_rows()[0];
    assert_eq!(table.rows, 0..1);
    assert_eq!(table.cells, 0..2);
    assert_eq!(table.logical_columns, 2);
    assert_eq!(row.table, table.key);
    assert_eq!(row.kind, ExecutionTableRowKind::Data);
    assert_eq!(row.logical_columns, 2);
    assert_eq!(row.cells, 0..2);
    assert_eq!(row.node, table.first_row_node);

    for (index, cell) in execution.table_cells().iter().enumerate() {
        let index = u32::try_from(index).unwrap();
        assert_eq!(cell.row, row.key);
        assert_eq!(cell.ordinal, index);
        assert_eq!(cell.data_ordinal, index);
        assert_eq!(cell.logical_column, index);
        assert_eq!(cell.column_span, 1);
        assert_eq!(cell.row_span, 1);
        assert_eq!(cell.layout_kind, ExecutionTableLayoutKind::Left);
        assert_eq!(cell.data_kind, ExecutionTableDataKind::Text);
        assert_eq!(cell.alignment, ExecutionTableAlignment::Left);
        let buffer = cell.buffer.expect("non-empty table cell buffer");
        let generation_key = cell
            .buffer_generation
            .expect("non-empty table cell generation");
        let generation = &execution.buffer_generations()[generation_key as usize];
        assert_eq!(generation.buffer, buffer);
        let payload_atoms = execution.atoms()[cell.atoms.start as usize..cell.atoms.end as usize]
            .iter()
            .filter(|atom| atom.role == AtomRole::TableCellPayload)
            .collect::<Vec<_>>();
        assert!(!payload_atoms.is_empty());
        assert!(payload_atoms.iter().all(|atom| {
            atom.buffer == Some(buffer) && atom.buffer_generation == Some(generation_key)
        }));
        assert!(
            execution.flushes()[row.flushes.start as usize..row.flushes.end as usize]
                .iter()
                .any(|flush| flush.buffer_generation == generation_key)
        );
    }

    let owned_table = table.clone();
    let owned_cells = execution.table_cells().to_vec();
    drop(report);
    assert_eq!(owned_table.logical_columns, 2);
    assert_eq!(owned_cells[0].data_ordinal, 0);
    assert_eq!(owned_cells[1].data_ordinal, 1);
}

#[test]
fn table_execution_preserves_layout_and_data_kinds_independently() {
    // Pinned CVS `tbl_data.c::getdata()` retains ordinary `TBL_DATA_DATA`
    // even when a `TBL_CELL_HORIZ` layout suppresses it; `-Ttree` reports
    // `1-[HIDDEN]` and `-Tutf8` renders only the rule.  Owned transfer must
    // validate both native facts rather than compare data kind to the
    // effective AST presentation kind.
    let report = execute(
        "table-layout-data.1",
        InputFormat::Man,
        b".TH PROBE 1\n.SH TABLE\n.TS\ntab(:);\nl _ l.\nleft:HIDDEN:right\n.TE\n",
    );
    let [row] = report.execution.table_rows() else {
        panic!("one native table row")
    };
    let cells = &report.execution.table_cells()[row.cells.start as usize..row.cells.end as usize];
    assert_eq!(cells.len(), 3);
    assert_eq!(
        cells[1].layout_kind,
        libmandoc_rs::ExecutionTableLayoutKind::HorizontalRule
    );
    assert_eq!(
        cells[1].data_kind,
        libmandoc_rs::ExecutionTableDataKind::Text
    );
    let ast = ast_node_by_execution_key(&report.document.root, cells[1].node.0)
        .expect("table row remains bound to the same native AST");
    assert_eq!(
        ast.table_cells[1].layout_kind,
        libmandoc_rs::TableCellLayoutKind::HorizontalRule
    );
    assert_eq!(
        ast.table_cells[1].data_kind,
        libmandoc_rs::TableCellDataKind::Text
    );
    assert_eq!(
        ast.table_cells[1].kind,
        libmandoc_rs::TableCellKind::HorizontalRule
    );
}

#[test]
fn table_execution_budget_failure_is_atomic_and_reentrant() {
    let baseline = execute("table.1", InputFormat::Man, TABLE);
    for limits in [
        ExecutionLimits {
            max_work: baseline.execution.work_units() - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_records: baseline.execution.record_count() - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_buffer_cells: baseline.execution.buffer_cells() - 1,
            ..ExecutionLimits::default()
        },
    ] {
        let error = Parser::default()
            .with_input_format(InputFormat::Man)
            .execute_bytes("table.1", TABLE, limits)
            .unwrap_err();
        assert_eq!(error.kind, ExecutionErrorKind::Budget);

        let next = execute("table.1", InputFormat::Man, TABLE);
        assert_eq!(next.execution.tables().len(), 1);
        assert_eq!(next.execution.table_cells().len(), 2);
    }
}

#[test]
fn empty_native_table_cell_has_no_buffer_identity() {
    // The pinned CVS renderer was run before this assertion was written and
    // emits `left` and `third`; tbl_term.c still executes the empty middle
    // logical data cell.
    let report = execute("empty-table.1", InputFormat::Man, TABLE_WITH_EMPTY_CELL);
    assert_eq!(report.execution.table_cells().len(), 3);
    assert!(report.execution.table_cells()[0].buffer.is_some());
    assert!(
        report.execution.table_cells()[0]
            .buffer_generation
            .is_some()
    );
    assert_eq!(report.execution.table_cells()[1].buffer, None);
    assert_eq!(report.execution.table_cells()[1].buffer_generation, None);
    assert!(report.execution.table_cells()[1].atoms.is_empty());
    assert!(report.execution.table_cells()[2].buffer.is_some());
    assert!(
        report.execution.table_cells()[2]
            .buffer_generation
            .is_some()
    );
}

#[test]
fn explicit_table_vertical_continuation_matches_the_owned_ast() {
    // The complete fixture was accepted by the pinned CVS linter and rendered
    // as one visible `first` row before this assertion was written.  tbl_data.c
    // recognizes the data spelling `\^` as the same continuation fact as a
    // layout `^` cell.
    let report = execute(
        "table-vertical-continuation.1",
        InputFormat::Man,
        TABLE_VERTICAL_CONTINUATION,
    );
    assert_eq!(report.execution.table_rows().len(), 2);
    assert_eq!(report.execution.table_cells().len(), 2);
    let continuation = &report.execution.table_cells()[1];
    assert!(
        continuation
            .flags
            .contains(libmandoc_rs::ExecutionTableCellFlags::VERTICAL_CONTINUATION)
    );
    let ast_row = ast_node_by_execution_key(&report.document.root, continuation.node.0)
        .expect("continuation row remains in the owned AST");
    assert!(ast_row.table_cells[0].vertical_continuation);
}

#[test]
fn table_payload_role_does_not_overwrite_implicit_spacing_provenance() {
    // The pinned CVS renderer was run before this assertion was written and
    // emits `left alpha` followed by the two native words `outside words`.
    let report = execute("spaced-table.1", InputFormat::Man, TABLE_WITH_WORD_SPACING);
    let cell = &report.execution.table_cells()[0];
    let atoms = &report.execution.atoms()[cell.atoms.start as usize..cell.atoms.end as usize];
    assert!(
        atoms
            .iter()
            .any(|atom| atom.role == AtomRole::TableCellPayload)
    );
    assert!(
        report
            .execution
            .atoms()
            .iter()
            .any(|atom| atom.role == AtomRole::ImplicitSpace)
    );
    assert!(
        atoms
            .iter()
            .all(|atom| atom.role != AtomRole::ImplicitSpace)
    );
}

#[test]
fn unsupported_execution_shapes_fail_before_returning_a_partial_report() {
    let error = Parser::default()
        .with_input_format(InputFormat::Man)
        .execute_bytes("equation.1", EQUATION, ExecutionLimits::default())
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::Unsupported);

    let parser = Parser::new(ParseOptions {
        includes: libmandoc_rs::IncludePolicy::SourceTree,
        compression: libmandoc_rs::Compression::Plain,
    });
    let error = parser
        .execute_bytes("plain-man.1", MAN, ExecutionLimits::default())
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::Unsupported);

    let error = Parser::default()
        .with_input_format(InputFormat::Man)
        .execute_bytes("executed-so.1", EXECUTED_SO, ExecutionLimits::default())
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::Unsupported);
    let inactive = Parser::default()
        .with_input_format(InputFormat::Man)
        .execute_bytes("inactive-so.1", INACTIVE_SO, ExecutionLimits::default())
        .unwrap();
    assert!(!inactive.execution.fragments().is_empty());
}
