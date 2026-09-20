//! Same-thread guard for all native libmandoc sessions.

use std::cell::Cell;

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

pub(super) struct NativeSessionGuard;

impl NativeSessionGuard {
    pub(super) fn enter() -> Result<Self, &'static str> {
        ACTIVE.with(|active| {
            if active.replace(true) {
                Err("recursive libmandoc session entry is unsupported")
            } else {
                Ok(Self)
            }
        })
    }
}

impl Drop for NativeSessionGuard {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(false));
    }
}

#[cfg(test)]
mod tests {
    use super::NativeSessionGuard;

    #[test]
    fn reentry_rejection_releases_after_outer_drop() {
        let outer = NativeSessionGuard::enter().expect("outer session");
        assert!(NativeSessionGuard::enter().is_err());
        drop(outer);
        NativeSessionGuard::enter().expect("later session recovers");
    }
}
