//! Manual source, query, and output engine independent from Bun and Node-API.

mod source;

pub use source::{
    CommandOutput, CommandRunner, LocateError, ManualRequest, SystemCommandRunner,
    locate_manual_source, locate_manual_source_with,
};

/// Reports the native contract version through the engine layer.
#[must_use]
pub const fn native_api_version() -> &'static str {
    mant_ast::NATIVE_API_VERSION
}

#[cfg(test)]
mod tests {
    use super::native_api_version;

    #[test]
    fn exposes_the_ast_contract_version() {
        assert_eq!(native_api_version(), "1");
    }
}
