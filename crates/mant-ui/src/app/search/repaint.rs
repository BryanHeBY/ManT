//! Shared debounce, scan-slice and passive-preview repaint policy.
use std::time::{Duration, Instant};

pub(in crate::app) const SEARCH_DEBOUNCE: Duration = Duration::from_millis(100);
pub(in crate::app) const SEARCH_SLICE_DELAY: Duration = Duration::from_millis(8);
pub(in crate::app) const SEARCH_BYTES_PER_SLICE: usize = 8192;
const SEARCH_REPAINT_INTERVAL: Duration = Duration::from_millis(50);

/// Match inventories advance every slice; passive previews repaint at most
/// every 50 ms. Input and explicit navigation still redraw immediately.
#[derive(Debug, Clone, Default)]
pub(in crate::app) struct SearchRepaint {
    deadline: Option<Instant>,
    dirty: bool,
}

impl SearchRepaint {
    pub(in crate::app) fn update(
        &mut self,
        now: Instant,
        changed: bool,
        complete: bool,
        navigated: bool,
    ) -> bool {
        self.dirty |= changed;
        if complete
            || navigated
            || (self.dirty && self.deadline.is_none_or(|deadline| deadline <= now))
        {
            self.dirty = false;
            self.deadline = Some(now + SEARCH_REPAINT_INTERVAL);
            true
        } else {
            false
        }
    }
}
