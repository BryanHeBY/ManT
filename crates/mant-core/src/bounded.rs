//! Small bounded-I/O primitives shared by local document source families.

use std::io::{self, Read};

#[derive(Debug)]
struct LimitExceeded {
    label: String,
    limit: u64,
}

impl std::fmt::Display for LimitExceeded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} exceeds the {}-byte limit",
            self.label, self.limit
        )
    }
}

impl std::error::Error for LimitExceeded {}

pub(crate) fn is_limit_exceeded(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|source| source.downcast_ref::<LimitExceeded>().is_some())
}

/// Read at most `limit` bytes without trusting source metadata.
pub(crate) fn read_bytes(reader: impl Read, limit: u64, label: &str) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            LimitExceeded {
                label: label.to_owned(),
                limit,
            },
        ));
    }
    Ok(bytes)
}

/// Read bounded UTF-8 while preserving failures from the underlying reader.
pub(crate) fn read_utf8(reader: impl Read, limit: u64, label: &str) -> io::Result<String> {
    let bytes = read_bytes(reader, limit, label)?;
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, format!("{label} must be UTF-8")))
}
