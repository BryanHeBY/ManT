//! Thin Node-API adapter for the renderer-neutral Mant core.

/// Smoke-test entry point used before Node-API bindings are introduced.
#[must_use]
pub const fn native_version() -> &'static str {
    mant_core::native_api_version()
}

#[cfg(test)]
mod tests {
    use super::native_version;

    #[test]
    fn delegates_versioning_to_the_core() {
        assert_eq!(native_version(), "1");
    }
}
