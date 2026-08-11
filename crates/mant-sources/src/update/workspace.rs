//! Temporary paths owned by one source-update transaction.

use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) struct UpdateWorkspace {
    pub(super) download: PathBuf,
    pub(super) checkout: PathBuf,
    pub(super) staging: PathBuf,
}

impl UpdateWorkspace {
    pub(super) fn new(sources: &Path, name: &str) -> Self {
        let workspace = Self {
            download: sources.join(format!(".{name}.download")),
            checkout: sources.join(format!(".{name}.checkout")),
            staging: sources.join(format!(".{name}.staging")),
        };
        workspace.clear();
        workspace
    }

    pub(super) fn create_staging(&self) -> Result<(), String> {
        fs::create_dir(&self.staging)
            .map_err(|error| format!("could not create staging directory: {error}"))
    }

    fn clear(&self) {
        remove_internal_path(&self.download);
        remove_internal_path(&self.checkout);
        remove_internal_path(&self.staging);
    }
}

impl Drop for UpdateWorkspace {
    fn drop(&mut self) {
        self.clear();
    }
}

fn remove_internal_path(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else if path.exists() {
        let _ = fs::remove_file(path);
    }
}
