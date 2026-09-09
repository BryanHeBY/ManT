//! The single work account shared across scan and optional inspection phases.
use super::{
    MAX_REFERENCE_SCAN_BYTES, MAX_REFERENCE_SCAN_STEPS, ReferenceScanLimits, ReferenceScanStop,
};
use crate::MAX_CONTENT_DEPTH;

/// Shared operation work account for traversal and optional form association.
/// A callback receives this same account, so repeated form checks cannot reset
/// or bypass the scan's step/byte limits.
#[derive(Debug)]
pub struct ReferenceWorkBudget {
    limits: ReferenceScanLimits,
    steps: usize,
    bytes: usize,
    stopped: Option<ReferenceScanStop>,
}

impl ReferenceWorkBudget {
    /// Start one bounded operation. All subsequent scans and callbacks may
    /// share this account rather than resetting work at each resolution phase.
    #[must_use]
    pub fn new(mut limits: ReferenceScanLimits) -> Self {
        limits.steps = limits.steps.min(MAX_REFERENCE_SCAN_STEPS);
        limits.bytes = limits.bytes.min(MAX_REFERENCE_SCAN_BYTES);
        limits.depth = limits.depth.min(MAX_CONTENT_DEPTH);
        Self {
            limits,
            steps: 0,
            bytes: 0,
            stopped: None,
        }
    }

    /// Remaining work units, shared by all phases in this scan.
    #[must_use]
    pub fn remaining_steps(&self) -> usize {
        self.limits.steps.saturating_sub(self.steps)
    }

    /// Remaining target/label/form inspection bytes.
    #[must_use]
    pub fn remaining_bytes(&self) -> usize {
        self.limits.bytes.saturating_sub(self.bytes)
    }

    /// Work units already consumed across every phase of this operation.
    #[must_use]
    pub const fn used_steps(&self) -> usize {
        self.steps
    }

    /// Inspection bytes already consumed across every phase of this operation.
    #[must_use]
    pub const fn used_bytes(&self) -> usize {
        self.bytes
    }

    /// The first failed charge, retained even if the consumer requests a stop.
    #[must_use]
    pub const fn stopped(&self) -> Option<ReferenceScanStop> {
        self.stopped
    }

    /// Charge bounded traversal/inspection before doing work or materializing
    /// data. Failed charges leave counters unchanged and latch the stop reason.
    ///
    /// # Errors
    /// Returns the first depth, step or byte limit that was exceeded, including
    /// a limit previously reached by another phase of this operation.
    pub fn consume(
        &mut self,
        depth: usize,
        steps: usize,
        bytes: usize,
    ) -> Result<(), ReferenceScanStop> {
        let stopped = self.stopped.or_else(|| {
            if depth > self.limits.depth {
                Some(ReferenceScanStop::Depth)
            } else if steps > self.remaining_steps() {
                Some(ReferenceScanStop::Steps)
            } else if bytes > self.remaining_bytes() {
                Some(ReferenceScanStop::Bytes)
            } else {
                None
            }
        });
        if let Some(reason) = stopped {
            self.stopped = Some(reason);
            return Err(reason);
        }
        self.steps += steps;
        self.bytes += bytes;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_clamps_limits_before_charging_and_depth_failure_is_atomic() {
        let mut budget = ReferenceWorkBudget::new(ReferenceScanLimits {
            steps: usize::MAX,
            bytes: usize::MAX,
            depth: usize::MAX,
        });
        assert_eq!(budget.remaining_steps(), MAX_REFERENCE_SCAN_STEPS);
        assert_eq!(budget.remaining_bytes(), MAX_REFERENCE_SCAN_BYTES);
        assert_eq!(
            budget.consume(MAX_CONTENT_DEPTH + 1, usize::MAX, usize::MAX),
            Err(ReferenceScanStop::Depth)
        );
        assert_eq!((budget.used_steps(), budget.used_bytes()), (0, 0));
        assert_eq!(budget.consume(0, 0, 0), Err(ReferenceScanStop::Depth));
    }

    #[test]
    fn failed_charge_preserves_counters_and_latches_the_original_limit() {
        let mut budget = ReferenceWorkBudget::new(ReferenceScanLimits {
            steps: 3,
            bytes: 7,
            depth: 5,
        });
        budget.consume(5, 2, 6).unwrap();
        assert_eq!(budget.consume(5, 1, 2), Err(ReferenceScanStop::Bytes));
        assert_eq!((budget.used_steps(), budget.used_bytes()), (2, 6));
        assert_eq!((budget.remaining_steps(), budget.remaining_bytes()), (1, 1));
        assert_eq!(budget.consume(6, 10, 0), Err(ReferenceScanStop::Bytes));
        assert_eq!(budget.stopped(), Some(ReferenceScanStop::Bytes));
    }
}
