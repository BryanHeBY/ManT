//! Small bounded-I/O primitives for source configuration and metadata.

use std::io::{self, Read};
use std::{fs, path::Path};

/// Bound file type as well as byte count. Managed metadata rejects leaf links;
/// user configuration may point at a regular-file symlink.
pub(crate) fn read_file_utf8(
    path: &Path,
    limit: u64,
    label: &str,
    allow_links: bool,
) -> io::Result<String> {
    let file = open_regular_file(path, limit, allow_links)?;
    read_utf8(file, limit, label)
}

fn open_regular_file(path: &Path, limit: u64, allow_links: bool) -> io::Result<fs::File> {
    let metadata = if allow_links {
        fs::metadata(path)?
    } else {
        fs::symlink_metadata(path)?
    };
    if !metadata.is_file() {
        return Err(io::Error::other("input is not a regular file"));
    }
    if metadata.len() > limit {
        return Err(io::Error::other("input exceeds the read budget"));
    }
    open_checked(path, limit, allow_links)
}

fn open_checked(path: &Path, limit: u64, allow_links: bool) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | if allow_links { 0 } else { libc::O_NOFOLLOW });
    }
    #[cfg(not(unix))]
    let _ = allow_links;
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("input is not a regular file"));
    }
    if file.metadata()?.len() > limit {
        return Err(io::Error::other("input exceeds the read budget"));
    }
    Ok(file)
}

/// Read at most `limit` bytes without trusting source metadata.
pub(crate) fn read_bytes(reader: impl Read, limit: u64, label: &str) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label} exceeds the {limit}-byte limit"),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;

/// Read bounded UTF-8 while preserving failures from the underlying reader.
pub(crate) fn read_utf8(reader: impl Read, limit: u64, label: &str) -> io::Result<String> {
    let bytes = read_bytes(reader, limit, label)?;
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, format!("{label} must be UTF-8")))
}
