//! Request-local, read-only installation facts, not a trust or recovery policy.
use std::{path::Path, sync::OnceLock};

use crate::{
    metadata::{SourceMetadata, read_source_metadata, validate_source_directory},
    registry::managed_document_count,
};

pub(crate) struct InstalledSourceProbe<'a> {
    path: &'a Path,
    directory: OnceLock<Result<bool, String>>,
    metadata: OnceLock<Result<SourceMetadata, String>>,
    inventory: OnceLock<Result<u32, String>>,
}

impl<'a> InstalledSourceProbe<'a> {
    pub(crate) fn new(path: &'a Path) -> Self {
        Self {
            path,
            directory: OnceLock::new(),
            metadata: OnceLock::new(),
            inventory: OnceLock::new(),
        }
    }

    pub(crate) fn directory(&self) -> Result<bool, String> {
        self.directory
            .get_or_init(|| validate_source_directory(self.path))
            .clone()
    }

    pub(crate) fn metadata(&self) -> Result<SourceMetadata, String> {
        self.metadata
            .get_or_init(|| read_source_metadata(self.path))
            .clone()
    }

    pub(crate) fn document_count(&self) -> Result<u32, String> {
        self.inventory
            .get_or_init(|| managed_document_count(self.path))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_probe_does_not_create_or_recover_an_installation() {
        let root = std::env::temp_dir().join(format!(
            "mant-probe-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        assert!(!root.exists());
        let probe = InstalledSourceProbe::new(&root);
        assert_eq!(probe.directory(), Ok(false));
        assert!(probe.metadata().is_err());
        assert!(!root.exists());
        // A fresh probe observes later changes; a command-local fact is stable.
        std::fs::create_dir(&root).unwrap();
        assert_eq!(probe.directory(), Ok(false));
        assert_eq!(InstalledSourceProbe::new(&root).directory(), Ok(true));
        std::fs::remove_dir(&root).unwrap();
    }
}
