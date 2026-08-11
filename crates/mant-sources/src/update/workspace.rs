//! Temporary paths owned by one source-update transaction.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(0);

pub(super) struct UpdateWorkspace {
    root: PathBuf,
    pub(super) download: PathBuf,
    pub(super) checkout: PathBuf,
    pub(super) staging: PathBuf,
}

impl UpdateWorkspace {
    pub(super) fn new(sources: &Path, name: &str) -> Result<Self, String> {
        for _ in 0..100 {
            let sequence = NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed);
            let root = sources.join(format!(".{name}.update-{}-{sequence}", std::process::id()));
            match fs::create_dir(&root) {
                Ok(()) => {
                    return Ok(Self {
                        download: root.join("download"),
                        checkout: root.join("checkout"),
                        staging: root.join("staging"),
                        root,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(format!("could not create update workspace: {error}"));
                }
            }
        }
        Err("could not allocate a unique update workspace".to_owned())
    }

    pub(super) fn create_staging(&self) -> Result<(), String> {
        fs::create_dir(&self.staging)
            .map_err(|error| format!("could not create staging directory: {error}"))
    }
}

impl Drop for UpdateWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
