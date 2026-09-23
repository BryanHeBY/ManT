#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod address;
mod content;
mod content_location;
mod content_projection;
mod content_store;
mod declaration;
mod document;
mod entry;
mod fixed_body;
mod fixed_section;
pub mod geometry;
mod heading;
mod identity;
mod index;
mod inline_text;
mod link_uri;
mod outline;
mod references;
mod resolved;
mod table;
#[cfg(test)]
pub(crate) mod test_support;
mod text_coordinates;
mod tldr;
mod validation;
pub mod visit;

pub use address::*;
pub use content::*;
pub use content_location::*;
pub use content_projection::*;
pub use content_store::*;
pub use declaration::*;
pub use document::*;
pub use entry::*;
pub use fixed_body::*;
pub use fixed_section::*;
pub use heading::*;
pub use identity::*;
pub use index::*;
pub use inline_text::*;
pub use link_uri::markdown_document_reference;
pub use outline::*;
pub use references::*;
pub use resolved::*;
pub use table::*;
pub use text_coordinates::*;
pub use tldr::*;
pub use validation::*;
