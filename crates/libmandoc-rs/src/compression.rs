//! Shared bounds for Rust-managed compressed manual sources.

use std::io::{self, Read};

#[cfg(windows)]
use std::{fs::File, path::Path};

/// Maximum decoded bytes retained from one Rust-managed compressed source.
pub const MAX_DECOMPRESSED_SOURCE_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn decode_zstd(reader: impl Read) -> io::Result<Vec<u8>> {
    let decoder = zstd::stream::read::Decoder::new(reader)?;
    read_bounded(decoder, MAX_DECOMPRESSED_SOURCE_BYTES)
}

#[cfg(windows)]
pub(crate) fn decode_gzip(reader: impl Read) -> io::Result<Vec<u8>> {
    read_bounded(
        flate2::read::MultiGzDecoder::new(reader),
        MAX_DECOMPRESSED_SOURCE_BYTES,
    )
}

#[cfg(windows)]
pub(crate) fn open_auto_file(path: &Path) -> io::Result<(File, bool)> {
    match File::open(path) {
        Ok(file) => Ok((
            file,
            path.extension().is_some_and(|extension| extension == "gz"),
        )),
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && path.extension().is_none_or(|extension| extension != "gz") =>
        {
            let mut compressed = path.as_os_str().to_os_string();
            compressed.push(".gz");
            File::open(compressed).map(|file| (file, true))
        }
        Err(error) => Err(error),
    }
}

fn read_bounded(reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let byte_budget = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut output = Vec::new();
    reader.take(byte_budget).read_to_end(&mut output)?;
    if output.len() > limit {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("decompressed manual source exceeds the {limit}-byte limit"),
        ))
    } else {
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::read_bounded;

    #[test]
    fn bounded_reader_accepts_the_limit_and_rejects_the_next_byte() {
        assert_eq!(
            read_bounded(&b"1234"[..], 4).expect("accept the exact limit"),
            b"1234"
        );
        let error = read_bounded(&b"12345"[..], 4).expect_err("reject one byte over the limit");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("4-byte limit"));
    }
}
