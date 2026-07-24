//! Versioned, renderer-neutral contracts shared by every `ManT` frontend.

mod document;
mod outline;
mod query;
mod schema;
mod search;
mod tldr;

pub use document::*;
pub use outline::*;
pub use query::*;
pub use schema::*;
pub use search::*;
pub use tldr::*;

/// Native API version negotiated independently from document schema versions.
pub const NATIVE_API_VERSION: &str = "3";

#[cfg(test)]
mod tests {
    use super::NATIVE_API_VERSION;

    #[test]
    fn native_api_version_is_explicit() {
        assert_eq!(NATIVE_API_VERSION, "3");
    }
}
