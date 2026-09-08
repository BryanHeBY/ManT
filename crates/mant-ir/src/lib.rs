#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod address;
mod declaration;
mod document;
mod entry;
mod identity;
mod index;
mod outline;
mod resolved;
mod table;
mod tldr;
mod validation;
pub mod visit;

pub use address::*;
pub use declaration::*;
pub use document::*;
pub use entry::*;
pub use identity::*;
pub use index::*;
pub use outline::*;
pub use resolved::*;
pub use table::*;
pub use tldr::*;
pub use validation::*;
