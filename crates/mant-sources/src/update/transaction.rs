//! Metadata durability, controlled activation and authorized recovery.
use super::{Path, SOURCE_METADATA_FILE, SourceMetadata, fs};
/// Only synced documents and metadata may cross the activation boundary.
pub(super) struct PreparedInstallation<'a> {
    staging: &'a Path,
}
impl<'a> PreparedInstallation<'a> {
    pub(super) fn prepare(staging: &'a Path, metadata: &SourceMetadata) -> Result<Self, String> {
        let metadata_text = toml::to_string_pretty(metadata)
            .map_err(|error| format!("could not encode source metadata: {error}"))?;
        let metadata_path = staging.join(SOURCE_METADATA_FILE);
        fs::write(&metadata_path, metadata_text)
            .map_err(|error| format!("could not write source metadata: {error}"))?;
        sync_file(&metadata_path, "source metadata")?;
        #[cfg(unix)]
        sync_directory(staging)?;
        Ok(Self { staging })
    }
    pub(super) fn activate(self, target: &Path) -> Result<(), String> {
        replace_directory(self.staging, target)
    }
}

pub(super) fn replace_directory(staging: &Path, target: &Path) -> Result<(), String> {
    replace_with(staging, target, &mut NativeActivation)
}

// A narrow operation boundary keeps fault injection off the public API and
// exercises the real ordering/rollback code, not a parallel transaction model.
trait ActivationIo {
    fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()>;
    fn sync_parent(&mut self, path: &Path) -> Result<(), String>;
}

struct NativeActivation;
impl ActivationIo for NativeActivation {
    fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        fs::rename(from, to)
    }
    fn sync_parent(&mut self, path: &Path) -> Result<(), String> {
        sync_parent_directory(path)
    }
}

fn replace_with(
    staging: &Path,
    target: &Path,
    operations: &mut impl ActivationIo,
) -> Result<(), String> {
    recover_with(target, operations)?;
    let backup = target.with_extension("backup");
    let had_target = target.exists();
    if had_target {
        operations
            .rename(target, &backup)
            .map_err(|error| format!("could not preserve previous source: {error}"))?;
        operations.sync_parent(target)?;
    }
    if let Err(error) = operations.rename(staging, target) {
        if had_target {
            // Keep the activation failure as the primary error. Restoration
            // is best effort; if it cannot complete, the intact `.backup`
            // remains available to `recover_directory` on the next attempt.
            let _ = operations.rename(&backup, target);
            let _ = operations.sync_parent(target);
        }
        return Err(format!("could not activate updated source: {error}"));
    }
    operations.sync_parent(target)?;
    remove_internal_dir(&backup);
    operations.sync_parent(target)?;
    Ok(())
}

pub(super) fn sync_file(path: &Path, label: &str) -> Result<(), String> {
    #[cfg(windows)]
    let file = fs::OpenOptions::new().write(true).open(path);
    #[cfg(not(windows))]
    let file = fs::File::open(path);
    file.and_then(|file| file.sync_all())
        .map_err(|error| format!("could not sync {label}: {error}"))
}

pub(super) fn sync_parent_directory(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "source target has no parent directory".to_owned())?;
    #[cfg(unix)]
    {
        sync_directory(parent)
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Ok(())
    }
}

#[cfg(unix)]
pub(super) fn sync_directory(path: &Path) -> Result<(), String> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("could not sync directory '{}': {error}", path.display()))
}

pub(super) fn recover_directory(target: &Path) -> Result<(), String> {
    recover_with(target, &mut NativeActivation)
}

fn recover_with(target: &Path, operations: &mut impl ActivationIo) -> Result<(), String> {
    let backup = target.with_extension("backup");
    if !backup.exists() {
        return Ok(());
    }
    if target.exists() {
        remove_internal_dir(&backup);
        Ok(())
    } else {
        operations
            .rename(&backup, target)
            .map_err(|error| format!("could not recover previous source: {error}"))
    }
}

#[cfg(test)]
mod tests;

pub(super) fn remove_internal_dir(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else if path.exists() {
        let _ = fs::remove_file(path);
    }
}
