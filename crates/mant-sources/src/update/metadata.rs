//! Strict on-disk identity for one installed source.

use serde::{Deserialize, Serialize};

use crate::{ConfiguredSource, SourceLocation, download::Validators};

pub(super) const VERSION: u8 = 3;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::update) struct SourceMetadata {
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
    pub(super) fn git(
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

    pub(super) fn archive(
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

    pub(super) fn matches(
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

    pub(super) fn revision(&self) -> &str {
        &self.revision
    }

    pub(super) const fn documents(&self) -> u32 {
        self.documents
    }

    pub(super) fn validators(&self) -> Option<Validators> {
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
