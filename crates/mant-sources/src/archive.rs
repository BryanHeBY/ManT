//! Detects and safely extracts bounded Markdown document archives.

use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
};

use flate2::read::MultiGzDecoder;

use crate::limits::{
    MAX_DOCUMENT_BYTES, MAX_SOURCE_BYTES, MAX_SOURCE_DEPTH, MAX_SOURCE_DOCUMENTS,
    MAX_SOURCE_ENTRIES,
};

pub(crate) const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArchiveFormat {
    Zip,
    Tar,
    TarGzip,
    TarZstd,
}

/// Extract regular Markdown files while rejecting unsafe archive structure.
pub(crate) fn extract_archive(archive: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination)
        .map_err(|error| format!("could not create archive extraction directory: {error}"))?;
    match detect_archive_format(archive)? {
        ArchiveFormat::Zip => extract_zip(archive, destination),
        ArchiveFormat::Tar => {
            let file = File::open(archive)
                .map_err(|error| format!("could not open downloaded archive: {error}"))?;
            extract_tar(BufReader::new(file), destination)
        }
        ArchiveFormat::TarGzip => {
            let file = File::open(archive)
                .map_err(|error| format!("could not open downloaded archive: {error}"))?;
            extract_tar(MultiGzDecoder::new(BufReader::new(file)), destination)
        }
        ArchiveFormat::TarZstd => {
            let file = File::open(archive)
                .map_err(|error| format!("could not open downloaded archive: {error}"))?;
            let decoder = zstd::stream::read::Decoder::new(BufReader::new(file))
                .map_err(|error| format!("could not decompress zstd archive: {error}"))?;
            extract_tar(decoder, destination)
        }
    }
}

fn detect_archive_format(path: &Path) -> Result<ArchiveFormat, String> {
    let mut file =
        File::open(path).map_err(|error| format!("could not open downloaded archive: {error}"))?;
    let mut prefix = [0_u8; 4];
    let count = file
        .read(&mut prefix)
        .map_err(|error| format!("could not inspect downloaded archive: {error}"))?;
    let prefix = &prefix[..count];
    if prefix.starts_with(b"PK\x03\x04")
        || prefix.starts_with(b"PK\x05\x06")
        || prefix.starts_with(b"PK\x07\x08")
    {
        Ok(ArchiveFormat::Zip)
    } else if prefix.starts_with(&[0x1f, 0x8b]) {
        Ok(ArchiveFormat::TarGzip)
    } else if prefix.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        Ok(ArchiveFormat::TarZstd)
    } else {
        Ok(ArchiveFormat::Tar)
    }
}

fn extract_zip(path: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(path)
        .map_err(|error| format!("could not open downloaded ZIP archive: {error}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("could not read ZIP archive: {error}"))?;
    if archive.len() > MAX_SOURCE_ENTRIES {
        return Err(format!(
            "archive contains more than {MAX_SOURCE_ENTRIES} entries"
        ));
    }

    let mut expanded = 0_u64;
    let mut documents = 0_usize;
    let mut paths = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("could not read ZIP entry {index}: {error}"))?;
        let raw_name = entry.name();
        entry
            .enclosed_name()
            .ok_or_else(|| format!("ZIP entry '{}' has an unsafe path", entry.name()))?;
        let path = match normalize_archive_name(raw_name)? {
            Some(path) => path,
            None if entry.is_dir() => continue,
            None => {
                return Err(format!("archive entry '{raw_name}' has an unsafe path"));
            }
        };
        if entry.is_symlink() {
            return Err(format!(
                "archive entry '{}' is a symbolic link",
                path.display()
            ));
        }
        if entry.is_dir() {
            continue;
        }
        if !entry.is_file() {
            return Err(format!(
                "archive entry '{}' is not a regular file",
                path.display()
            ));
        }
        expanded = charge_expanded(expanded, entry.size(), &path)?;
        if !is_markdown(&path) {
            continue;
        }
        let size = entry.size();
        documents = charge_document(documents, size, &path)?;
        write_archive_file(destination, &path, &mut entry, size, &mut paths)?;
    }
    Ok(())
}

fn extract_tar(reader: impl Read, destination: &Path) -> Result<(), String> {
    extract_tar_with_budget(reader, destination, MAX_SOURCE_BYTES)
}

fn extract_tar_with_budget(
    reader: impl Read,
    destination: &Path,
    stream_limit: u64,
) -> Result<(), String> {
    // tar-rs consumes GNU long-name/long-link and local PAX records before it
    // yields the described entry. Put the aggregate decompressed-byte gate
    // below the parser so those hidden records and any future parser-owned
    // metadata cannot allocate or read beyond the source budget.
    let mut archive = tar::Archive::new(ExpandedArchiveReader::new(reader, stream_limit));
    let entries = archive
        .entries()
        .map_err(|error| format!("could not read tar archive: {error}"))?;
    let mut count = 0_usize;
    let mut expanded = 0_u64;
    let mut documents = 0_usize;
    let mut paths = BTreeSet::new();
    for entry in entries {
        count += 1;
        if count > MAX_SOURCE_ENTRIES {
            return Err(format!(
                "archive contains more than {MAX_SOURCE_ENTRIES} entries"
            ));
        }
        let mut entry = entry.map_err(|error| format!("could not read tar entry: {error}"))?;
        let raw_path = entry.path_bytes();
        let raw_name =
            std::str::from_utf8(&raw_path).map_err(|_| "tar entry path is not UTF-8".to_owned())?;
        let entry_type = entry.header().entry_type();
        let size = entry.size();
        expanded = charge_expanded_with_limit(expanded, size, Path::new(raw_name), stream_limit)?;
        let path = match normalize_archive_name(raw_name)? {
            Some(path) => path,
            None if entry_type.is_dir() => continue,
            None => {
                return Err(format!("archive entry '{raw_name}' has an unsafe path"));
            }
        };
        if entry_type.is_pax_global_extensions() {
            validate_global_pax(&mut entry)?;
            continue;
        }
        if entry_type.is_dir() {
            continue;
        }
        if !entry_type.is_file() {
            return Err(format!(
                "archive entry '{}' is not a regular file",
                path.display()
            ));
        }
        if !is_markdown(&path) {
            continue;
        }
        documents = charge_document(documents, size, &path)?;
        write_archive_file(destination, &path, &mut entry, size, &mut paths)?;
    }
    Ok(())
}

struct ExpandedArchiveReader<R> {
    inner: R,
    remaining: u64,
    limit: u64,
}

impl<R> ExpandedArchiveReader<R> {
    const fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            remaining: limit,
            limit,
        }
    }
}

impl<R: Read> Read for ExpandedArchiveReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut probe = [0_u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "archive decompressed stream exceeds the {}-byte source limit",
                        self.limit
                    ),
                )),
            };
        }
        let maximum = usize::try_from(self.remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let read = self.inner.read(&mut buffer[..maximum])?;
        self.remaining = self
            .remaining
            .saturating_sub(u64::try_from(read).unwrap_or(u64::MAX));
        Ok(read)
    }
}

fn validate_global_pax(entry: &mut tar::Entry<'_, impl Read>) -> Result<(), String> {
    let extensions = entry
        .pax_extensions()
        .map_err(|error| format!("could not read global PAX metadata: {error}"))?
        .ok_or_else(|| "global PAX header did not contain metadata".to_owned())?;
    for extension in extensions {
        let extension =
            extension.map_err(|error| format!("could not read global PAX metadata: {error}"))?;
        let key = extension
            .key()
            .map_err(|_| "global PAX metadata key is not UTF-8".to_owned())?;
        if key != "comment" {
            return Err(format!("global PAX metadata key '{key}' is not supported"));
        }
    }
    Ok(())
}

fn normalize_archive_name(name: &str) -> Result<Option<PathBuf>, String> {
    // Archive member names are POSIX identities. Parse the raw ZIP string or
    // tar bytes before a host `Path` can reinterpret `\\` as a separator on
    // Windows and erase the evidence that the archive was non-portable.
    if name.contains('\\')
        || name
            .chars()
            .any(crate::registry::is_unsafe_logical_path_character)
    {
        return Err(format!("archive entry '{name}' has an unsafe path"));
    }
    let mut normalized = PathBuf::new();
    let mut depth = 0_usize;
    let mut components = name.split('/').peekable();
    while let Some(component) = components.next() {
        if component == "." {
            continue;
        }
        if component.is_empty() && components.peek().is_none() {
            continue;
        }
        if component.is_empty() || component == ".." || (depth == 0 && component.ends_with(':')) {
            return Err(format!("archive entry '{name}' has an unsafe path"));
        }
        normalized.push(component);
        depth += 1;
    }
    if depth > MAX_SOURCE_DEPTH {
        return Err(format!(
            "archive entry '{name}' exceeds the maximum path depth of {MAX_SOURCE_DEPTH}"
        ));
    }
    Ok((depth != 0).then_some(normalized))
}

fn charge_expanded(current: u64, size: u64, path: &Path) -> Result<u64, String> {
    charge_expanded_with_limit(current, size, path, MAX_SOURCE_BYTES)
}

fn charge_expanded_with_limit(
    current: u64,
    size: u64,
    path: &Path,
    limit: u64,
) -> Result<u64, String> {
    let next = current
        .checked_add(size)
        .ok_or_else(|| "archive expanded-size budget overflow".to_owned())?;
    if next > limit {
        return Err(format!(
            "archive exceeds the {limit}-byte expanded-size limit at '{}'",
            path.display()
        ));
    }
    Ok(next)
}

fn charge_document(current: usize, size: u64, path: &Path) -> Result<usize, String> {
    if size > MAX_DOCUMENT_BYTES {
        return Err(format!(
            "Markdown entry '{}' exceeds the {MAX_DOCUMENT_BYTES}-byte limit",
            path.display()
        ));
    }
    let next = current + 1;
    if next > MAX_SOURCE_DOCUMENTS {
        return Err(format!(
            "archive contains more than {MAX_SOURCE_DOCUMENTS} Markdown documents"
        ));
    }
    Ok(next)
}

fn write_archive_file(
    destination: &Path,
    relative: &Path,
    reader: &mut impl Read,
    expected: u64,
    paths: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    if !paths.insert(relative.to_owned()) {
        return Err(format!(
            "archive contains duplicate path '{}'",
            relative.display()
        ));
    }
    let output = destination.join(relative);
    let parent = output
        .parent()
        .ok_or_else(|| format!("archive entry '{}' has no parent", relative.display()))?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "could not create directory for archive entry '{}': {error}",
            relative.display()
        )
    })?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .map_err(|error| {
            format!(
                "could not create archive entry '{}': {error}",
                relative.display()
            )
        })?;
    let mut bounded = reader.take(expected.saturating_add(1));
    let copied = io::copy(&mut bounded, &mut file).map_err(|error| {
        format!(
            "could not extract archive entry '{}': {error}",
            relative.display()
        )
    })?;
    if copied != expected {
        return Err(format!(
            "archive entry '{}' decoded to {copied} bytes instead of {expected}",
            relative.display()
        ));
    }
    Ok(())
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        fs,
        io::{Cursor, Write as _},
        path::PathBuf,
    };

    use flate2::{Compression, write::GzEncoder};
    use tar::{Builder, EntryType, Header};
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::{
        extract_archive, extract_tar_with_budget, normalize_archive_name, write_archive_file,
    };

    fn temp(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mant-archive-{label}-{}", std::process::id()))
    }

    fn tar_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = Builder::new(Vec::new());
        for (path, contents) in entries {
            let mut header = Header::new_gnu();
            header.set_path(path).expect("set tar path");
            header.set_size(u64::try_from(contents.len()).expect("content length fits"));
            header.set_mode(0o644);
            header.set_cksum();
            builder.append(&header, *contents).expect("append tar file");
        }
        builder.into_inner().expect("finish tar")
    }

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (path, contents) in entries {
            writer
                .start_file(*path, SimpleFileOptions::default())
                .expect("start ZIP file");
            writer.write_all(contents).expect("write ZIP file");
        }
        writer.finish().expect("finish ZIP").into_inner()
    }

    fn pax_record(key: &str, value: &str) -> Vec<u8> {
        let remainder = format!(" {key}={value}\n");
        let mut width = 1;
        loop {
            let length = width + remainder.len();
            let digits = length.to_string().len();
            if digits == width {
                return format!("{length}{remainder}").into_bytes();
            }
            width = digits;
        }
    }

    fn tar_with_global_pax(key: &str, value: &str) -> Vec<u8> {
        let mut builder = Builder::new(Vec::new());
        let metadata = pax_record(key, value);
        let mut header = Header::new_ustar();
        header
            .set_path("pax_global_header")
            .expect("set global header path");
        header.set_entry_type(EntryType::XGlobalHeader);
        header.set_size(u64::try_from(metadata.len()).expect("metadata length fits"));
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append(&header, metadata.as_slice())
            .expect("append global PAX header");

        let contents = b"# tool";
        let mut header = Header::new_ustar();
        header.set_path("docs/tool.md").expect("set document path");
        header.set_size(u64::try_from(contents.len()).expect("content length fits"));
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append(&header, contents.as_slice())
            .expect("append document");
        builder.into_inner().expect("finish tar")
    }

    fn assert_extracts(label: &str, bytes: &[u8]) {
        let root = temp(label);
        let archive = root.join("download");
        let destination = root.join("tree");
        fs::create_dir_all(&root).expect("create fixture");
        fs::write(&archive, bytes).expect("write archive");
        extract_archive(&archive, &destination).expect("extract archive");
        assert_eq!(
            fs::read_to_string(destination.join("docs/tool.md")).expect("read document"),
            "# tool"
        );
        assert!(!destination.join("docs/ignored.txt").exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn extracts_zip_tar_gzip_and_zstd_by_content() {
        let entries: &[(&str, &[u8])] = &[
            ("docs/tool.md", b"# tool"),
            ("docs/ignored.txt", b"ignored"),
        ];
        assert_extracts("zip", &zip_bytes(entries));

        let tar = tar_bytes(entries);
        assert_extracts("tar", &tar);
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&tar).expect("compress gzip");
        assert_extracts("tar-gzip", &gzip.finish().expect("finish gzip"));
        assert_extracts(
            "tar-zstd",
            &zstd::stream::encode_all(Cursor::new(tar), 0).expect("compress zstd"),
        );
    }

    #[test]
    fn rejects_non_regular_archive_entries() {
        let root = temp("symlink");
        let archive = root.join("download");
        let destination = root.join("tree");
        fs::create_dir_all(&root).expect("create fixture");
        let mut builder = Builder::new(Vec::new());
        let mut header = Header::new_gnu();
        header.set_path("docs/tool.md").expect("set path");
        header.set_entry_type(EntryType::Symlink);
        header.set_link_name("../outside.md").expect("set link");
        header.set_size(0);
        header.set_mode(0o777);
        header.set_cksum();
        builder
            .append(&header, std::io::empty())
            .expect("append symlink");
        fs::write(&archive, builder.into_inner().expect("finish tar")).expect("write archive");
        let error = extract_archive(&archive, &destination).expect_err("reject symlink");
        assert!(error.contains("not a regular file"), "{error}");
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn rejects_zip_parent_components_before_normalization() {
        let root = temp("zip-parent");
        let archive = root.join("download");
        let destination = root.join("tree");
        fs::create_dir_all(&root).expect("create fixture");
        fs::write(&archive, zip_bytes(&[("docs/../escape.md", b"# escape")]))
            .expect("write archive");
        let error = extract_archive(&archive, &destination).expect_err("reject parent path");
        assert!(error.contains("unsafe path"), "{error}");
        assert!(!destination.join("escape.md").exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn accepts_git_archive_global_comment_metadata_only() {
        assert_extracts(
            "global-pax-comment",
            &tar_with_global_pax("comment", "0123456789abcdef0123456789abcdef01234567"),
        );

        let root = temp("global-pax-path");
        let archive = root.join("download");
        let destination = root.join("tree");
        fs::create_dir_all(&root).expect("create fixture");
        fs::write(&archive, tar_with_global_pax("path", "elsewhere.md")).expect("write archive");
        let error = extract_archive(&archive, &destination).expect_err("reject global path");
        assert!(error.contains("key 'path' is not supported"), "{error}");
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn tar_parser_owned_metadata_cannot_bypass_the_stream_budget() {
        let root = temp("tar-metadata-budget");
        let destination = root.join("tree");
        fs::create_dir_all(&root).expect("create fixture");

        let long_name = format!("docs/{}.md", "a".repeat(4_096));
        let mut builder = Builder::new(Vec::new());
        let mut header = Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o644);
        builder
            .append_data(&mut header, &long_name, Cursor::new(b"x"))
            .expect("append long-name entry");
        let archive = builder.into_inner().expect("finish tar");

        let error = extract_tar_with_budget(Cursor::new(archive), &destination, 1_024)
            .expect_err("reject parser-owned metadata above the stream budget");
        assert!(error.contains("decompressed stream exceeds"), "{error}");
        assert!(!destination.join("docs").exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn tar_directory_payloads_count_toward_the_expanded_budget() {
        let root = temp("tar-directory-budget");
        let destination = root.join("tree");
        fs::create_dir_all(&root).expect("create fixture");

        let mut builder = Builder::new(Vec::new());
        let mut header = Header::new_gnu();
        header.set_path("docs").expect("set directory path");
        header.set_entry_type(EntryType::Directory);
        header.set_size(1_025);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append(&header, Cursor::new(vec![0_u8; 1_025]))
            .expect("append oversized directory payload");
        let archive = builder.into_inner().expect("finish tar");

        let error = extract_tar_with_budget(Cursor::new(archive), &destination, 1_024)
            .expect_err("directory payload must consume the expanded-size budget");
        assert!(error.contains("1024-byte expanded-size limit"), "{error}");
        assert!(error.contains("docs"), "{error}");
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn archive_paths_accept_explicit_current_directory_components() {
        assert_eq!(
            normalize_archive_name("./docs/./tool.md").expect("normalize GNU tar path"),
            Some(PathBuf::from("docs/tool.md"))
        );
        assert_eq!(
            normalize_archive_name(".").expect("normalize archive root"),
            None
        );

        let entries: &[(&str, &[u8])] = &[("./docs/tool.md", b"# tool")];
        assert_extracts("tar-dot-prefix", &tar_bytes(entries));
        assert_extracts("zip-dot-prefix", &zip_bytes(entries));
    }

    #[test]
    fn archive_paths_reject_host_separator_and_control_characters() {
        for path in ["docs\\tool.md", "docs/bad\u{1f}name.md"] {
            let error = normalize_archive_name(path).expect_err("reject non-portable archive path");
            assert!(error.contains("unsafe path"), "{error}");
        }
    }

    #[test]
    fn archive_writes_stop_one_byte_after_the_declared_size() {
        let root = temp("bounded-write");
        fs::create_dir_all(&root).expect("create fixture");
        let relative = PathBuf::from("docs/tool.md");
        let mut input = Cursor::new(vec![b'x'; 64]);
        let error = write_archive_file(&root, &relative, &mut input, 3, &mut BTreeSet::new())
            .expect_err("reject declared size mismatch");

        assert!(error.contains("decoded to 4 bytes instead of 3"), "{error}");
        assert_eq!(fs::metadata(root.join(relative)).expect("output").len(), 4);
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
