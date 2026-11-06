//! Test-only counters on production annotation paths, independent of time/RSS.
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::mandoc) struct Counts {
    pub registered: usize,
    pub registered_bytes: usize,
    pub published: usize,
    pub published_bytes: usize,
    pub bound: usize,
    pub decorations: usize,
    pub released: usize,
    pub runs: usize,
    pub held: usize,
    pub normalization: usize,
}

std::thread_local! {
    static COUNTS: std::cell::Cell<Counts> = const { std::cell::Cell::new(Counts {
        registered: 0, registered_bytes: 0, published: 0, published_bytes: 0,
        bound: 0, decorations: 0, released: 0, runs: 0, held: 0, normalization: 0,
    }) };
}

pub(in crate::mandoc) fn note(change: impl FnOnce(&mut Counts)) {
    COUNTS.with(|counter| {
        let mut counts = counter.get();
        change(&mut counts);
        counter.set(counts);
    });
}

pub(in crate::mandoc) fn take() -> Counts {
    COUNTS.with(|counter| counter.replace(Counts::default()))
}
