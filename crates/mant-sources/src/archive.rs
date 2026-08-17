//! Detects and safely extracts bounded Markdown document archives.

use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{self, BufReader, Read},
    path::{Component, Path, PathBuf},
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
        validate_zip_entry_name(entry.name())?;
        let path = entry
            .enclosed_name()
            .ok_or_else(|| format!("ZIP entry '{}' has an unsafe path", entry.name()))?;
        validate_archive_path(&path)?;
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

fn validate_zip_entry_name(name: &str) -> Result<(), String> {
    let normalized = name.replace('\\', "/");
    let trimmed = normalized.strip_suffix('/').unwrap_or(&normalized);
    let mut components = trimmed.split('/');
    let first = components.next().unwrap_or_default();
    if first.is_empty()
        || first.ends_with(':')
        || components.clone().any(str::is_empty)
        || std::iter::once(first)
            .chain(components)
            .any(|component| matches!(component, "." | ".."))
    {
        return Err(format!("ZIP entry '{name}' has an unsafe path"));
    }
    Ok(())
}

fn extract_tar(reader: impl Read, destination: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(reader);
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
        let path = entry
            .path()
            .map_err(|error| format!("could not decode tar entry path: {error}"))?
            .into_owned();
        validate_archive_path(&path)?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            continue;
        }
        if !entry_type.is_file() {
            return Err(format!(
                "archive entry '{}' is not a regular file",
                path.display()
            ));
        }
        let size = entry.size();
        expanded = charge_expanded(expanded, size, &path)?;
        if !is_markdown(&path) {
            continue;
        }
        documents = charge_document(documents, size, &path)?;
        write_archive_file(destination, &path, &mut entry, size, &mut paths)?;
    }
    Ok(())
}

fn validate_archive_path(path: &Path) -> Result<(), String> {
    let mut depth = 0_usize;
    for component in path.components() {
        let Component::Normal(value) = component else {
            return Err(format!(
                "archive entry '{}' has an unsafe path",
                path.display()
            ));
        };
        value.to_str().ok_or_else(|| {
            format!(
                "archive entry '{}' does not use a UTF-8 path",
                path.display()
            )
        })?;
        depth += 1;
    }
    if depth == 0 || depth > MAX_SOURCE_DEPTH {
        return Err(format!(
            "archive entry '{}' exceeds the maximum path depth of {MAX_SOURCE_DEPTH}",
            path.display()
        ));
    }
    Ok(())
}

fn charge_expanded(current: u64, size: u64, path: &Path) -> Result<u64, String> {
    let next = current
        .checked_add(size)
        .ok_or_else(|| "archive expanded-size budget overflow".to_owned())?;
    if next > MAX_SOURCE_BYTES {
        return Err(format!(
            "archive exceeds the {MAX_SOURCE_BYTES}-byte expanded-size limit at '{}'",
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
    let copied = io::copy(reader, &mut file).map_err(|error| {
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
        fs,
        io::{Cursor, Write as _},
        path::PathBuf,
    };

    use flate2::{Compression, write::GzEncoder};
    use tar::{Builder, EntryType, Header};
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::extract_archive;

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
}
