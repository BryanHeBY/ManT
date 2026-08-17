//! Small opaque continuation tokens bound to one normalized tool request.

use std::hash::{DefaultHasher, Hash, Hasher};

/// Tool-specific cursor namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CursorKind {
    Find,
    Outline,
    Read,
    Explain,
    Search,
}

impl CursorKind {
    const fn code(self) -> char {
        match self {
            Self::Find => 'f',
            Self::Outline => 'o',
            Self::Read => 'r',
            Self::Explain => 'e',
            Self::Search => 's',
        }
    }
}

/// Stable hash for the normalized inputs which define one result stream.
pub(super) fn fingerprint(parts: &[&str]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for part in parts {
        part.hash(&mut hasher);
    }
    hasher.finish()
}

/// Encode one session-local continuation position.
pub(super) fn encode(kind: CursorKind, fingerprint: u64, position: u64) -> String {
    format!("c1-{}-{fingerprint:016x}-{position:016x}", kind.code())
}

/// Decode and validate a cursor for an otherwise identical request.
pub(super) fn decode(
    value: Option<&str>,
    kind: CursorKind,
    expected_fingerprint: u64,
) -> Result<u64, String> {
    let Some(value) = value else {
        return Ok(0);
    };
    let mut fields = value.split('-');
    let valid_prefix = fields.next() == Some("c1");
    let valid_kind = fields
        .next()
        .is_some_and(|value| value.len() == 1 && value.starts_with(kind.code()));
    let fingerprint = fields
        .next()
        .and_then(|value| u64::from_str_radix(value, 16).ok());
    let position = fields
        .next()
        .and_then(|value| u64::from_str_radix(value, 16).ok());
    if !valid_prefix
        || !valid_kind
        || fields.next().is_some()
        || fingerprint != Some(expected_fingerprint)
    {
        return Err(
            "cursor is invalid or belongs to a different request; restart without it".to_owned(),
        );
    }
    position.ok_or_else(|| {
        "cursor is invalid or belongs to a different request; restart without it".to_owned()
    })
}

/// Combine a result offset with a byte position inside its rendered page.
pub(super) const fn join_position(offset: u32, byte: u32) -> u64 {
    (offset as u64) << 32 | byte as u64
}

/// Recover the result offset and rendered byte position.
pub(super) const fn split_position(position: u64) -> (u32, u32) {
    let bytes = position.to_be_bytes();
    (
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
    )
}

#[cfg(test)]
mod tests {
    use super::{CursorKind, decode, encode, fingerprint, join_position, split_position};

    #[test]
    fn cursors_are_bound_to_tool_and_normalized_request() {
        let fingerprint = fingerprint(&["manual/1/git", "needle"]);
        let token = encode(CursorKind::Search, fingerprint, join_position(20, 17));
        assert_eq!(
            split_position(
                decode(Some(&token), CursorKind::Search, fingerprint).expect("valid cursor")
            ),
            (20, 17)
        );
        assert!(decode(Some(&token), CursorKind::Read, fingerprint).is_err());
        assert!(decode(Some(&token), CursorKind::Search, fingerprint + 1).is_err());
    }

    #[test]
    fn malformed_cursors_fail_without_partial_interpretation() {
        for value in ["", "c1-s", "c1-s-not-hex-0000000000000000", "c2-s-0-0"] {
            assert!(decode(Some(value), CursorKind::Search, 0).is_err());
        }
    }
}
