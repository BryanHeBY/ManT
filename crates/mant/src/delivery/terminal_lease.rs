//! One terminal session's restoration obligations, independent of its driver.
//!
//! TUI and pager acquisition order differs. Callers choose that order; this
//! owner reverses it during cleanup without assuming a single ready flag.

use std::io;

/// Independently acquired terminal modes with corresponding release actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminalResource {
    RawMode,
    AlternateScreen,
    MouseCapture,
    HiddenCursor,
}

/// The concrete terminal adapter supplies only these reversible operations.
///
/// Acquiring raw mode enables it; the other acquisitions enter the alternate
/// screen, capture the mouse, and hide the cursor respectively. Releases undo
/// those operations. A release must be safe after a partially applied acquire,
/// and must be retryable when its previous attempt returned an error.
pub(crate) trait TerminalOps {
    fn acquire(&mut self, resource: TerminalResource) -> io::Result<()>;
    fn release(&mut self, resource: TerminalResource) -> io::Result<()>;
}

/// Owns terminal cleanup from the first attempted acquisition through release.
///
/// An escape write can fail after its bytes have partially reached the terminal.
/// Register the restoration obligation before executing the operation, including
/// when that operation fails. This does not claim the terminal confirmed it.
/// No mode is touched merely by constructing or dropping an unused lease.
/// The host must provide exclusive ownership of this terminal session: this
/// lease must not restore modes owned by an embedding caller's active session.
pub(crate) struct TerminalLease<O: TerminalOps> {
    operations: O,
    pending: [Option<TerminalResource>; 4],
}

impl<O: TerminalOps> TerminalLease<O> {
    pub(crate) const fn new(operations: O) -> Self {
        Self {
            operations,
            pending: [None; 4],
        }
    }

    /// Acquire one resource, preserving the caller's ordering for restoration.
    ///
    /// A resource already awaiting restoration cannot be acquired again. After
    /// an error the caller should restore or drop the lease, not retry setup.
    pub(crate) fn acquire(&mut self, resource: TerminalResource) -> io::Result<()> {
        if self.pending.contains(&Some(resource)) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "terminal resource is already awaiting restoration",
            ));
        }
        let Some(slot) = self.pending.iter_mut().find(|slot| slot.is_none()) else {
            return Err(io::Error::other("terminal restoration capacity exhausted"));
        };
        *slot = Some(resource);
        self.operations.acquire(resource)
    }

    /// Whether a mode still carries a restoration obligation, including a
    /// partially applied acquisition or failed release.
    pub(crate) fn has_pending(&self, resource: TerminalResource) -> bool {
        self.pending.contains(&Some(resource))
    }

    /// Release one mode requested by an embedded driver (notably cursor Show).
    /// A failed release remains owned; an already released mode performs no IO.
    pub(crate) fn release_one(&mut self, resource: TerminalResource) -> io::Result<()> {
        let Some(index) = self.pending.iter().position(|item| *item == Some(resource)) else {
            return Ok(());
        };
        self.operations.release(resource)?;
        self.pending[index] = None;
        self.compact();
        Ok(())
    }

    /// Attempt every outstanding release in reverse acquisition order.
    ///
    /// Return the first error, without skipping later cleanup. Only successful
    /// releases are cleared; an explicit retry or Drop retries the rest. The
    /// driver retains any earlier setup/run error as its primary failure.
    pub(crate) fn restore(&mut self) -> io::Result<()> {
        let mut first_error = None;
        for slot in self.pending.iter_mut().rev() {
            let Some(resource) = *slot else { continue };
            match self.operations.release(resource) {
                Ok(()) => *slot = None,
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        self.compact();
        first_error.map_or(Ok(()), Err)
    }

    // Keep chronological order when a driver acquires a released mode while
    // other restoration obligations remain pending.
    fn compact(&mut self) {
        let mut retained = [None; 4];
        for (index, resource) in self.pending.iter().flatten().enumerate() {
            retained[index] = Some(*resource);
        }
        self.pending = retained;
    }
}

impl<O: TerminalOps> Drop for TerminalLease<O> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    use super::*;
    use TerminalResource::{AlternateScreen, HiddenCursor, MouseCapture, RawMode};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Event {
        Acquire(TerminalResource),
        Release(TerminalResource),
    }

    #[derive(Default)]
    struct State {
        events: Vec<Event>,
        active: Vec<TerminalResource>,
        acquire_error: Option<TerminalResource>,
        release_errors: VecDeque<TerminalResource>,
    }

    struct Operations(Rc<RefCell<State>>);

    impl TerminalOps for Operations {
        fn acquire(&mut self, resource: TerminalResource) -> io::Result<()> {
            let mut state = self.0.borrow_mut();
            state.events.push(Event::Acquire(resource));
            // Model an escape reaching the terminal before its flush fails.
            state.active.push(resource);
            if state.acquire_error == Some(resource) {
                Err(io::Error::other(format!("acquire {resource:?}")))
            } else {
                Ok(())
            }
        }

        fn release(&mut self, resource: TerminalResource) -> io::Result<()> {
            let mut state = self.0.borrow_mut();
            state.events.push(Event::Release(resource));
            if state.release_errors.front() == Some(&resource) {
                state.release_errors.pop_front();
                Err(io::Error::other(format!("release {resource:?}")))
            } else {
                state.active.retain(|active| *active != resource);
                Ok(())
            }
        }
    }

    fn lease(state: &Rc<RefCell<State>>) -> TerminalLease<Operations> {
        TerminalLease::new(Operations(Rc::clone(state)))
    }

    #[test]
    fn unused_lease_and_repeated_restoration_do_not_touch_the_terminal() {
        let state = Rc::new(RefCell::new(State::default()));
        let mut session = lease(&state);
        session.restore().unwrap();
        session.restore().unwrap();
        drop(session);
        assert!(state.borrow().events.is_empty());
    }

    #[test]
    fn callers_preserve_distinct_tui_and_pager_acquisition_orders() {
        for order in [
            [RawMode, AlternateScreen, MouseCapture, HiddenCursor],
            [AlternateScreen, RawMode, MouseCapture, HiddenCursor],
        ] {
            let state = Rc::new(RefCell::new(State::default()));
            let mut session = lease(&state);
            for resource in order {
                session.acquire(resource).unwrap();
            }
            session.restore().unwrap();
            session.restore().unwrap();
            drop(session);
            let expected = order
                .into_iter()
                .map(Event::Acquire)
                .chain(order.into_iter().rev().map(Event::Release))
                .collect::<Vec<_>>();
            assert_eq!(state.borrow().events, expected);
        }
    }

    #[test]
    fn each_setup_failure_restores_only_attempted_resources_in_reverse_order() {
        for order in [
            [RawMode, AlternateScreen, MouseCapture, HiddenCursor],
            [AlternateScreen, RawMode, MouseCapture, HiddenCursor],
        ] {
            for failure in 0..order.len() {
                let state = Rc::new(RefCell::new(State {
                    acquire_error: Some(order[failure]),
                    ..State::default()
                }));
                let mut session = lease(&state);
                let result = order
                    .into_iter()
                    .try_for_each(|resource| session.acquire(resource));
                assert_eq!(
                    result.unwrap_err().to_string(),
                    format!("acquire {:?}", order[failure])
                );
                assert_eq!(state.borrow().active, order[..=failure]);
                drop(session);
                let attempted = &order[..=failure];
                let expected = attempted
                    .iter()
                    .copied()
                    .map(Event::Acquire)
                    .chain(attempted.iter().rev().copied().map(Event::Release))
                    .collect::<Vec<_>>();
                assert_eq!(state.borrow().events, expected);
                assert!(state.borrow().active.is_empty());
            }
        }
    }

    #[test]
    fn restoration_attempts_all_resources_preserves_first_error_and_drop_retries_failures() {
        let state = Rc::new(RefCell::new(State {
            release_errors: VecDeque::from([HiddenCursor, RawMode]),
            ..State::default()
        }));
        let mut session = lease(&state);
        for resource in [RawMode, AlternateScreen, MouseCapture, HiddenCursor] {
            session.acquire(resource).unwrap();
        }
        assert_eq!(
            session.restore().unwrap_err().to_string(),
            "release HiddenCursor"
        );
        assert_eq!(
            &state.borrow().events[4..],
            &[
                Event::Release(HiddenCursor),
                Event::Release(MouseCapture),
                Event::Release(AlternateScreen),
                Event::Release(RawMode),
            ]
        );
        drop(session);
        assert_eq!(
            &state.borrow().events[8..],
            &[Event::Release(HiddenCursor), Event::Release(RawMode)]
        );
    }

    #[test]
    fn explicit_retry_releases_only_failures_and_keeps_new_acquisitions_last() {
        let state = Rc::new(RefCell::new(State {
            release_errors: VecDeque::from([RawMode]),
            ..State::default()
        }));
        let mut session = lease(&state);
        session.acquire(RawMode).unwrap();
        session.acquire(AlternateScreen).unwrap();
        assert!(session.restore().is_err());
        session.acquire(AlternateScreen).unwrap();
        session.restore().unwrap();
        drop(session);
        assert_eq!(
            state.borrow().events,
            [
                Event::Acquire(RawMode),
                Event::Acquire(AlternateScreen),
                Event::Release(AlternateScreen),
                Event::Release(RawMode),
                Event::Acquire(AlternateScreen),
                Event::Release(AlternateScreen),
                Event::Release(RawMode),
            ]
        );
    }

    #[test]
    fn each_release_failure_is_retried_without_releasing_successful_modes_twice() {
        let order = [RawMode, AlternateScreen, MouseCapture, HiddenCursor];
        for failed in order {
            let state = Rc::new(RefCell::new(State {
                release_errors: VecDeque::from([failed]),
                ..State::default()
            }));
            let mut session = lease(&state);
            for resource in order {
                session.acquire(resource).unwrap();
            }
            assert_eq!(
                session.restore().unwrap_err().to_string(),
                format!("release {failed:?}")
            );
            assert_eq!(state.borrow().active, [failed]);
            session.restore().unwrap();
            drop(session);
            let expected = order
                .into_iter()
                .map(Event::Acquire)
                .chain(order.into_iter().rev().map(Event::Release))
                .chain([Event::Release(failed)])
                .collect::<Vec<_>>();
            assert_eq!(state.borrow().events, expected);
            assert!(state.borrow().active.is_empty());
        }
    }

    #[test]
    fn drop_ignores_io_errors_but_still_attempts_every_remaining_release() {
        let state = Rc::new(RefCell::new(State {
            release_errors: VecDeque::from([MouseCapture, AlternateScreen, RawMode]),
            ..State::default()
        }));
        let mut session = lease(&state);
        for resource in [RawMode, AlternateScreen, MouseCapture] {
            session.acquire(resource).unwrap();
        }
        drop(session);
        assert_eq!(
            &state.borrow().events[3..],
            &[
                Event::Release(MouseCapture),
                Event::Release(AlternateScreen),
                Event::Release(RawMode),
            ]
        );
    }

    #[test]
    fn duplicate_acquisition_cannot_overwrite_a_restoration_obligation() {
        let state = Rc::new(RefCell::new(State {
            acquire_error: Some(AlternateScreen),
            ..State::default()
        }));
        let mut session = lease(&state);
        assert!(session.acquire(AlternateScreen).is_err());
        assert_eq!(
            session.acquire(AlternateScreen).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        drop(session);
        assert_eq!(
            state.borrow().events,
            [
                Event::Acquire(AlternateScreen),
                Event::Release(AlternateScreen),
            ]
        );
    }
}
