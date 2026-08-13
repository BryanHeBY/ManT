//! Versioned process contracts for `ManT`'s CLI, JSON, and MCP boundaries.

mod catalog;
mod document;
mod outline;
mod query;
mod schema;
mod search;
mod selector;
mod update;

pub use catalog::*;
pub use document::*;
pub use outline::*;
pub use query::*;
pub use schema::*;
pub use search::*;
pub use selector::*;
pub use update::*;

/// Native API version negotiated independently from document schema versions.
pub const NATIVE_API_VERSION: &str = "7";

#[cfg(test)]
mod tests {
    use super::NATIVE_API_VERSION;

    #[test]
    fn native_api_version_is_explicit() {
        assert_eq!(NATIVE_API_VERSION, "7");
    }
}
