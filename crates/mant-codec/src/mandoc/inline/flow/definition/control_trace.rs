//! Local test-only trace of one pre-br transaction; no document history.

use super::super::field_buffer::FlushReceipt;
use super::super::native_field::FieldFlags;

#[derive(Debug)]
pub(super) struct Trace {
    pub(super) old_flags: FieldFlags,
    pub(super) cells: usize,
    pub(super) viscol: usize,
    pub(super) nospace_before_capture: bool,
    pub(super) captures: usize,
    pub(super) projections: usize,
    pub(super) retirements: usize,
    pub(super) accepted_ends: Vec<usize>,
    pub(super) printed_column: Option<usize>,
    pub(super) ends_row: Option<bool>,
    pub(super) post_flags: Option<FieldFlags>,
    pub(super) phases: Vec<&'static str>,
}

std::thread_local! {
    static ACTIVE: std::cell::RefCell<Option<Trace>> = const { std::cell::RefCell::new(None) };
    static LAST: std::cell::RefCell<Option<Trace>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn begin(flags: FieldFlags, cells: usize, viscol: usize, nospace: bool) {
    ACTIVE.with(|slot| {
        *slot.borrow_mut() = Some(Trace {
            old_flags: flags,
            cells,
            viscol,
            nospace_before_capture: nospace,
            captures: 0,
            projections: 0,
            retirements: 0,
            accepted_ends: Vec::new(),
            printed_column: None,
            ends_row: None,
            post_flags: None,
            phases: vec!["nospace"],
        });
    });
}

pub(super) fn capture(receipt: &FlushReceipt, flags: FieldFlags) {
    ACTIVE.with(|slot| {
        if let Some(trace) = &mut *slot.borrow_mut() {
            assert_eq!(
                flags, trace.old_flags,
                "pre-br changed flags before old buffer consumption"
            );
            trace.captures += 1;
            trace.accepted_ends = match receipt {
                FlushReceipt::Accepted { passes, .. } | FlushReceipt::Rejected { passes, .. } => {
                    passes.iter().map(|pass| pass.end).collect()
                }
            };
            trace.phases.push("capture");
        }
    });
}

pub(super) fn device_tail(column: usize, ends_row: bool) {
    ACTIVE.with(|slot| {
        if let Some(trace) = &mut *slot.borrow_mut() {
            trace.printed_column = Some(column);
            trace.ends_row = Some(ends_row);
        }
    });
}

pub(super) fn projected() {
    ACTIVE.with(|slot| {
        if let Some(trace) = &mut *slot.borrow_mut() {
            trace.projections += 1;
            trace.phases.push("project");
        }
    });
}

pub(super) fn retired() {
    ACTIVE.with(|slot| {
        if let Some(trace) = &mut *slot.borrow_mut() {
            trace.retirements += 1;
            trace.phases.push("retire");
        }
    });
}

pub(super) fn finish(flags: Option<FieldFlags>) {
    let trace = ACTIVE.with(|slot| slot.borrow_mut().take());
    LAST.with(|slot| {
        *slot.borrow_mut() = trace.map(|mut trace| {
            trace.post_flags = flags;
            trace.phases.push("post-flags");
            trace
        });
    });
}

pub(super) fn take() -> Trace {
    LAST.with(|slot| slot.borrow_mut().take().expect("a pre-br request ran"))
}
