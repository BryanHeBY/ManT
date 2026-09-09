//! Process-owned terminal presentation and output delivery.

#[cfg(feature = "pager")]
pub(crate) mod pager;
#[cfg(all(unix, any(feature = "tui", feature = "pager")))]
mod signals;
#[cfg(feature = "tui")]
pub(crate) mod terminal;
#[cfg(any(feature = "tui", feature = "pager"))]
mod terminal_lease;
pub(crate) mod tldr;
