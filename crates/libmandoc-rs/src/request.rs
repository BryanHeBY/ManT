//! Queries backed by the pinned parser's native request registry.

/// Return whether `name` is a native roff request in the pinned libmandoc.
///
/// This is intentionally narrower than a man(7) or mdoc(7) macro lookup.
/// Consumers that replay table input can use it to preserve libmandoc's tbl
/// dispatch boundary: native requests are handled by roff, while tbl receives
/// the operands of unknown and high-level macro control lines.
#[must_use]
pub fn is_native_roff_request(name: &str) -> bool {
    if name.is_empty() || name.as_bytes().contains(&0) {
        return false;
    }
    crate::ffi::is_native_roff_request(name)
}

#[cfg(test)]
mod tests {
    use super::is_native_roff_request;

    #[test]
    fn native_request_lookup_tracks_the_pinned_roff_registry() {
        for request in ["br", "ll", "po", "ft", "mc", "if", "TS"] {
            assert!(is_native_roff_request(request), "{request}");
        }
        for macro_name in ["BR", "Fl", "Sm", "Xr", "not-a-request"] {
            assert!(!is_native_roff_request(macro_name), "{macro_name}");
        }
    }
}
