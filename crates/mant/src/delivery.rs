//! Process-owned terminal presentation and output delivery.

pub(crate) mod pager;
#[cfg(unix)]
mod signals;
pub(crate) mod terminal;
mod terminal_lease;
pub(crate) mod tldr;
