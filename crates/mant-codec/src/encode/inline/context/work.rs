//! Test counters shared by local witnesses and final semantic projection.

std::thread_local! {
    pub(super) static PARSED_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static WHOLE_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static PROJECTED_SCALARS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
