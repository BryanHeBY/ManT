//! Cumulative limits shared by the synchronous document ownership transfer.

pub(super) const MAX_OWNED_EQUATION_DEPTH: usize = 256;
pub(super) const MAX_OWNED_EQUATION_NODES: usize = 50_000;
pub(super) const MAX_OWNED_EQUATION_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_OWNED_SYNTAX_ITEMS: usize = 250_000;
pub(super) const MAX_OWNED_SYNTAX_BYTES: usize = 128 * 1024 * 1024;

#[derive(Default)]
pub(super) struct TransferBudget {
    pub(super) items: usize,
    pub(super) bytes: usize,
}

impl TransferBudget {
    pub(super) fn charge(&mut self, bytes: usize) -> Result<(), String> {
        self.items = self.items.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
        if self.items > MAX_OWNED_SYNTAX_ITEMS || self.bytes > MAX_OWNED_SYNTAX_BYTES {
            return Err(format!(
                "owned syntax transfer exceeded its cumulative node/byte budget ({} items, {} bytes)",
                self.items, self.bytes
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct EquationBudget {
    pub(super) nodes: usize,
    pub(super) bytes: usize,
    pub(super) truncated: bool,
}
