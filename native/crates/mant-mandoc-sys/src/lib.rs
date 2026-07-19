//! Private libmandoc build and future FFI boundary.
//!
//! Building the pinned C parser is intentionally separate from exposing any
//! unsafe symbols. The next layer will add a narrow, owned C shim rather than
//! bind Rust directly to libmandoc's internal structures.

#[cfg(test)]
mod build_config;

/// Pinned upstream version compiled by this crate's build script.
pub const MANDOC_VERSION: &str = "1.14.6";

#[cfg(test)]
mod tests {
    use super::MANDOC_VERSION;

    #[test]
    fn upstream_version_is_pinned() {
        assert_eq!(MANDOC_VERSION, "1.14.6");
    }
}
