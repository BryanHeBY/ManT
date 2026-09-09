#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod address;
mod content_location;
mod declaration;
mod document;
mod entry;
mod heading;
mod identity;
mod index;
mod outline;
mod references;
mod resolved;
mod table;
mod tldr;
mod validation;
pub mod visit;

pub use address::*;
pub use content_location::*;
pub use declaration::*;
pub use document::*;
pub use entry::*;
pub use heading::*;
pub use identity::*;
pub use index::*;
pub use outline::*;
pub use references::*;
pub use resolved::*;
pub use table::*;
pub use tldr::*;
pub use validation::*;
