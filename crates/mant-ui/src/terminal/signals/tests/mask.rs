//! Thread-local signal-mask fixture. Production must respect host signal masks.

use std::{marker::PhantomData, ptr, rc::Rc};

/// Restore the calling thread's original mask before its handlers are removed.
/// Rc's auto-traits prevent moving this guard to another thread.
pub(super) struct SignalMask {
    previous: libc::sigset_t,
    selected: libc::sigset_t,
    _same_thread: PhantomData<Rc<()>>,
}

#[allow(unsafe_code)] // Test-only, checked libc calls with correctly typed sigset_t storage.
impl SignalMask {
    pub(super) fn block(signal: libc::c_int) -> Self {
        // sigset_t is an integer bitset. Initialize all storage, including any
        // reserved words a platform syscall might leave untouched.
        let mut selected = unsafe { std::mem::zeroed::<libc::sigset_t>() };
        assert_eq!(unsafe { libc::sigemptyset(&raw mut selected) }, 0);
        assert_eq!(unsafe { libc::sigaddset(&raw mut selected, signal) }, 0);
        let mut previous = unsafe { std::mem::zeroed::<libc::sigset_t>() };
        // Only this thread's selected signal changes; successful pthread_sigmask
        // initializes previous, which the same-thread Drop restores verbatim.
        assert_eq!(
            unsafe {
                libc::pthread_sigmask(libc::SIG_BLOCK, &raw const selected, &raw mut previous)
            },
            0
        );
        Self {
            previous,
            selected,
            _same_thread: PhantomData,
        }
    }

    pub(super) fn unblock(&self) {
        assert_eq!(
            unsafe {
                libc::pthread_sigmask(libc::SIG_UNBLOCK, &raw const self.selected, ptr::null_mut())
            },
            0
        );
    }

    pub(super) fn was_blocked(&self, signal: libc::c_int) -> bool {
        unsafe { libc::sigismember(&raw const self.previous, signal) == 1 }
    }
}

#[allow(unsafe_code)] // Restore only the mask saved on this same test thread.
impl Drop for SignalMask {
    fn drop(&mut self) {
        let result = unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &raw const self.previous, ptr::null_mut())
        };
        if !std::thread::panicking() {
            assert_eq!(result, 0, "restore test thread's signal mask");
        }
    }
}

#[test]
fn nested_masks_restore_the_original_thread_state() {
    // Use another signal so this fixture test cannot interfere with the
    // concurrent real SIGUSR1 delivery test.
    let signal = libc::SIGUSR2;
    let initially_blocked = current_is_blocked(signal);
    {
        let _outer = SignalMask::block(signal);
        assert!(current_is_blocked(signal));
        {
            let inner = SignalMask::block(signal);
            inner.unblock();
            assert!(!current_is_blocked(signal));
        }
        assert!(current_is_blocked(signal));
    }
    assert_eq!(current_is_blocked(signal), initially_blocked);
}

#[allow(unsafe_code)] // A null input reads this thread's mask without changing it.
fn current_is_blocked(signal: libc::c_int) -> bool {
    let mut current = unsafe { std::mem::zeroed::<libc::sigset_t>() };
    assert_eq!(
        unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, ptr::null(), &raw mut current) },
        0
    );
    unsafe { libc::sigismember(&raw const current, signal) == 1 }
}
