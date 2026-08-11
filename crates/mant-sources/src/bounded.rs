//! Small bounded-I/O primitives for source configuration and metadata.

use std::io::{self, Read};

/// Read at most `limit` bytes without trusting source metadata.
pub(crate) fn read_bytes(reader: impl Read, limit: u64, label: &str) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label} exceeds the {limit}-byte limit"),
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
