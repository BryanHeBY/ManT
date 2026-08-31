//! Strict installed-source metadata shared by inspection and updates.

use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

#[cfg(feature = "update")]
use crate::download::Validators;
use crate::{ConfiguredSource, SOURCE_METADATA_FILE, SourceLocation};

pub(crate) const MAX_METADATA_BYTES: u64 = 64 * 1024;
const VERSION: u8 = 3;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceMetadata {
    version: u8,
    source: String,
    location: SourceMetadataLocation,
    revision: String,
    config_fingerprint: String,
    documents: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum SourceMetadataLocation {
    Git {
        repo: String,
        branch: String,
    },
    Archive {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        etag: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_modified: Option<String>,
    },
}

impl SourceMetadata {
    #[cfg(feature = "update")]
    pub(crate) fn git(
        source: &str,
        repo: &str,
        branch: &str,
        revision: String,
        config_fingerprint: &str,
        documents: u32,
    ) -> Self {
        Self {
            version: VERSION,
            source: source.to_owned(),
            location: SourceMetadataLocation::Git {
                repo: repo.to_owned(),
                branch: branch.to_owned(),
            },
            revision,
            config_fingerprint: config_fingerprint.to_owned(),
            documents,
        }
    }

    #[cfg(feature = "update")]
    pub(crate) fn archive(
        source: &str,
        url: &str,
        revision: String,
        config_fingerprint: &str,
        documents: u32,
        validators: Validators,
    ) -> Self {
        Self {
            version: VERSION,
            source: source.to_owned(),
            location: SourceMetadataLocation::Archive {
                url: url.to_owned(),
                etag: validators.etag,
                last_modified: validators.last_modified,
            },
            revision,
            config_fingerprint: config_fingerprint.to_owned(),
            documents,
        }
    }

    pub(crate) fn matches(
        &self,
        name: &str,
        configured: &ConfiguredSource,
        fingerprint: &str,
    ) -> bool {
        self.version == VERSION
            && self.source == name
            && self.config_fingerprint == fingerprint
            && match (&self.location, &configured.location) {
                (
                    SourceMetadataLocation::Git { repo, branch },
                    SourceLocation::Git {
                        repo: configured_repo,
                        branch: configured_branch,
                    },
                ) => repo == configured_repo && branch == configured_branch,
                (
                    SourceMetadataLocation::Archive { url, .. },
                    SourceLocation::Archive {
                        url: configured_url,
                    },
                ) => url == configured_url,
                _ => false,
            }
    }

    pub(crate) fn revision(&self) -> &str {
        &self.revision
    }

    pub(crate) const fn documents(&self) -> u32 {
        self.documents
    }

    #[cfg(feature = "update")]
    pub(crate) fn validators(&self) -> Option<Validators> {
        let SourceMetadataLocation::Archive {
            etag,
            last_modified,
            ..
        } = &self.location
        else {
            return None;
        };
        if etag.is_none() && last_modified.is_none() {
            None
        } else {
            Some(Validators {
                etag: etag.clone(),
                last_modified: last_modified.clone(),
            })
        }
    }
}

pub(crate) fn source_fingerprint(source: &ConfiguredSource) -> String {
    let mut include = source
        .include
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let mut exclude = source
        .exclude
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    include.sort_unstable();
    exclude.sort_unstable();
    include.dedup();
    exclude.dedup();
    let mut state = 0xcbf2_9ce4_8422_2325_u64;
    let location = match &source.location {
        SourceLocation::Git { repo, branch } => format!("git\0{repo}\0{branch}"),
        SourceLocation::Archive { url } => format!("archive\0{url}"),
    };
    for byte in location
        .bytes()
        .chain([0])
        .chain(source.path.bytes())
        .chain([0])
        .chain(
            include
                .into_iter()
                .flat_map(|value| value.bytes().chain([0])),
        )
        .chain([0xff])
        .chain(
            exclude
                .into_iter()
                .flat_map(|value| value.bytes().chain([0])),
        )
    {
        state ^= u64::from(byte);
        state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{state:016x}")
}

pub(crate) fn read_source_metadata(directory: &Path) -> Result<SourceMetadata, String> {
    if !validate_source_directory(directory)? {
        return Err("source directory is missing".to_owned());
    }
    let path = directory.join(SOURCE_METADATA_FILE);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("could not inspect source metadata: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err("source metadata is not a regular file".to_owned());
    }
    let file =
        fs::File::open(path).map_err(|error| format!("could not open source metadata: {error}"))?;
    let text = crate::bounded::read_utf8(file, MAX_METADATA_BYTES, "source metadata")
        .map_err(|error| error.to_string())?;
    toml::from_str(&text).map_err(|error| format!("source metadata is invalid: {error}"))
}

/// Validate the physical root shared by update, discovery, doctor, and prune.
///
/// A missing directory is an ordinary uninstalled state. Every present entry
/// must be a real directory rather than a link or another filesystem object.
pub(crate) fn validate_source_directory(directory: &Path) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("could not inspect source directory: {error}")),
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() || !file_type.is_dir() {
        return Err("source entry is not a regular directory".to_owned());
    }
    Ok(true)
}
