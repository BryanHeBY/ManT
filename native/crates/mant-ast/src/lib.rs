//! Versioned, renderer-neutral contracts shared by every Mant frontend.

mod document;
mod outline;
mod query;
mod tldr;

pub use document::*;
pub use outline::*;
pub use query::*;
pub use tldr::*;

/// Native API version negotiated independently from document schema versions.
pub const NATIVE_API_VERSION: &str = "1";

#[cfg(test)]
mod tests {
    use super::NATIVE_API_VERSION;

    #[test]
    fn native_api_version_is_explicit() {
        assert_eq!(NATIVE_API_VERSION, "1");
    }
}
