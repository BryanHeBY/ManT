//! Private libmandoc build and FFI boundary.
//!
//! The real bindings arrive after the versioned document contract is in
//! place. Keeping this crate separate prevents unsafe C details from leaking
//! into the document engine.

/// Pinned upstream version that the future build script will compile.
pub const MANDOC_VERSION: &str = "1.14.6";

#[cfg(test)]
mod tests {
    use super::MANDOC_VERSION;

    #[test]
    fn upstream_version_is_pinned() {
        assert_eq!(MANDOC_VERSION, "1.14.6");
    }
}
