//! Regular-file-only, size-bounded reads shared by every configuration dialect.

use std::{
    fs,
    io::{self, Read},
    path::Path,
};

pub(super) fn read_text(path: &Path, limit: u64) -> io::Result<String> {
    let limit = limit.min(super::MAX_MANUAL_PATH_CONFIG_BYTES);
    // Follow regular-file symlinks, but reject known special files before open.
    validate_metadata(&fs::metadata(path)?, limit)?;
    let file = open_regular_file(path, limit)?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("configuration exceeds the read budget"));
    }
    String::from_utf8(bytes).map_err(io::Error::other)
}

fn open_regular_file(path: &Path, limit: u64) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A checked path (or its symlink target) may be replaced by a FIFO
        // before open. Do not wait for a writer before we can check the handle.
        // O_NONBLOCK does not turn regular filesystem I/O into a timed read.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    validate_metadata(&file.metadata()?, limit)?;
    Ok(file)
}

fn validate_metadata(metadata: &fs::Metadata, limit: u64) -> io::Result<()> {
    if !metadata.is_file() {
        return Err(io::Error::other("configuration is not a regular file"));
    }
    if metadata.len() > limit {
        return Err(io::Error::other("configuration exceeds the read budget"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
