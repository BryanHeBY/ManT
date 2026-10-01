//! Test-only, thread-local observations at the actual recursive entry points.

use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Counts {
    pub(super) blocks: usize,
    pub(super) inline_fragments: usize,
}

thread_local! {
    static ACTIVE: Cell<Option<Counts>> = const { Cell::new(None) };
}

pub(super) fn observe<T>(run: impl FnOnce() -> T) -> (T, Counts) {
    assert!(ACTIVE.replace(Some(Counts::default())).is_none());
    let result = run();
    let counts = ACTIVE.take().expect("active observation");
    (result, counts)
}

pub(super) fn block() {
    ACTIVE.set(ACTIVE.get().map(|mut count| {
        count.blocks += 1;
        count
    }));
}

pub(super) fn inline() {
    ACTIVE.set(ACTIVE.get().map(|mut count| {
        count.inline_fragments += 1;
        count
    }));
}
