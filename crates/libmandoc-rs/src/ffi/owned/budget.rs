//! Cumulative limits shared by the synchronous document ownership transfer.

pub(super) const MAX_OWNED_EQUATION_DEPTH: usize = 256;
pub(super) const MAX_OWNED_EQUATION_NODES: usize = 50_000;
pub(super) const MAX_OWNED_EQUATION_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_OWNED_SYNTAX_ITEMS: usize = 250_000;
pub(super) const MAX_OWNED_SYNTAX_BYTES: usize = 128 * 1024 * 1024;

#[cfg_attr(not(test), derive(Default))]
pub(super) struct TransferBudget {
    pub(super) items: usize,
    pub(super) bytes: usize,
}

#[cfg(test)]
std::thread_local! {
    // Tests exercise the real parse entry points with a few KiB of remaining
    // budget. The override belongs to this thread and is restored on unwind.
    static INITIAL_TRANSFER_USAGE: std::cell::Cell<(usize, usize)> = const {
        std::cell::Cell::new((0, 0))
    };
}

#[cfg(test)]
impl Default for TransferBudget {
    fn default() -> Self {
        let (items, bytes) = INITIAL_TRANSFER_USAGE.get();
        Self { items, bytes }
    }
}

#[cfg(test)]
pub(super) fn with_remaining_budget<T>(
    items: usize,
    bytes: usize,
    operation: impl FnOnce() -> T,
) -> T {
    struct Restore((usize, usize));
    impl Drop for Restore {
        fn drop(&mut self) {
            INITIAL_TRANSFER_USAGE.set(self.0);
        }
    }
    let _restore = Restore(INITIAL_TRANSFER_USAGE.replace((
        MAX_OWNED_SYNTAX_ITEMS.checked_sub(items).unwrap(),
        MAX_OWNED_SYNTAX_BYTES.checked_sub(bytes).unwrap(),
    )));
    operation()
}

impl TransferBudget {
    pub(super) fn reserve_items(&mut self, count: usize) -> Result<(), String> {
        let items = self.items.saturating_add(count);
        if count > MAX_OWNED_SYNTAX_ITEMS.saturating_sub(self.items) {
            return Err(Self::exceeded(items, self.bytes));
        }
        self.items = items;
        Ok(())
    }

    pub(super) fn reserve_bytes(&mut self, count: usize) -> Result<(), String> {
        let bytes = self.bytes.saturating_add(count);
        if count > MAX_OWNED_SYNTAX_BYTES.saturating_sub(self.bytes) {
            return Err(Self::exceeded(self.items, bytes));
        }
        self.bytes = bytes;
        Ok(())
    }

    fn exceeded(items: usize, bytes: usize) -> String {
        format!(
            "owned syntax transfer exceeded its cumulative node/byte budget ({items} items, {bytes} bytes)"
        )
    }
}

#[derive(Default)]
pub(super) struct EquationBudget {
    pub(super) nodes: usize,
    pub(super) bytes: usize,
    pub(super) truncated: bool,
}
