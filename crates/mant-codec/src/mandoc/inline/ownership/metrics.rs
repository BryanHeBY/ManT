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
    pub previewed: usize,
    pub navigation_relocated_word_starts: usize,
    pub navigation_target_probe_visits: usize,
}

std::thread_local! {
    static COUNTS: std::cell::Cell<Counts> = const { std::cell::Cell::new(Counts {
        registered: 0, registered_bytes: 0, published: 0, published_bytes: 0,
        bound: 0, decorations: 0, released: 0, runs: 0, held: 0, normalization: 0,
        previewed: 0,
        navigation_relocated_word_starts: 0,
        navigation_target_probe_visits: 0,
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

// Test-only reference to the old eager traversal, to prove that gating it
// preserves complete output even when markers have been cut or relocated.
std::thread_local! {
    static EAGER_NAVIGATION_PROBES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(in crate::mandoc) fn eager_navigation_probes() -> bool {
    EAGER_NAVIGATION_PROBES.get()
}

pub(in crate::mandoc) fn with_eager_navigation_probes<T>(run: impl FnOnce() -> T) -> T {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            EAGER_NAVIGATION_PROBES.set(self.0);
        }
    }
    let _reset = Reset(EAGER_NAVIGATION_PROBES.replace(true));
    run()
}
