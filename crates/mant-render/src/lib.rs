#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

pub mod cells;
mod output;
mod presentation;
pub mod table_border;
mod tldr;

pub use output::*;
pub use presentation::*;
pub use tldr::{TldrLine, TldrRole, TldrSpan, layout_tldr};
