//! Source-neutral document intermediate representation shared by `ManT`.

mod address;
mod document;
mod identity;
mod index;
mod outline;
mod resolved;
mod tldr;
mod validation;
pub mod visit;

pub use address::*;
pub use document::*;
pub use identity::*;
pub use index::*;
pub use outline::*;
pub use resolved::*;
pub use tldr::*;
pub use validation::*;
