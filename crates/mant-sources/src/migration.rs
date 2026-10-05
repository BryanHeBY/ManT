//! Explicit installer-only copying of legacy application storage.
//! Ordinary discovery never invokes this module. Originals are never deleted.

use crate::{SourceConfigError, settings::Settings};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_FILES: usize = 100_000;

struct Copy {
    target: PathBuf,
    bytes: Vec<u8>,
    permissions: fs::Permissions,
}
struct Plan {
    copies: Vec<Copy>,
    bytes: usize,
    entries: usize,
    replacements: std::collections::BTreeMap<PathBuf, Vec<u8>>,
}

impl Plan {
    fn file(
        &mut self,
        source: &Path,
        target: PathBuf,
        replacement: Option<Vec<u8>>,
    ) -> Result<(), SourceConfigError> {
        let metadata = fs::symlink_metadata(source).map_err(problem)?;
        if !metadata.is_file() {
            return Err(problem(format!(
                "migration requires regular files, not links or special files: {}",
                source.display()
            )));
        }
        let bytes = replacement
            .or_else(|| self.replacements.get(source).cloned())
            .map_or_else(
                || crate::bounded::read_file_bytes(source, 64 * 1024 * 1024, "migration input"),
                Ok,
            )
            .map_err(problem)?;
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| problem("migration size overflow"))?;
        if self.bytes > MAX_BYTES {
            return Err(problem(
                "migration exceeds the 256 MiB budget; migrate large collections manually",
            ));
        }
        if source == target {
            return Ok(());
        }
        match fs::symlink_metadata(&target) {
            Ok(metadata) => {
                if metadata.is_file()
                    && crate::bounded::read_file_bytes(
                        &target,
                        64 * 1024 * 1024,
                        "migration target",
                    )
                    .map_err(problem)?
                        == bytes
                {
                    return Ok(());
                }
                return Err(problem(format!(
                    "migration conflict at '{}'; existing content was not overwritten",
                    target.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(problem(error)),
        }
        if self.copies.len() >= MAX_FILES {
            return Err(problem(
                "migration exceeds the 256 MiB / 100000-file budget; migrate large collections manually",
            ));
        }
        self.copies.push(Copy {
            target,
            bytes,
            permissions: metadata.permissions(),
        });
        Ok(())
    }

    fn tree(
        &mut self,
        source: &Path,
        target: &Path,
        depth: usize,
    ) -> Result<(), SourceConfigError> {
        let metadata = match fs::symlink_metadata(source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(problem(error)),
        };
        if source == target {
            return Ok(());
        }
        if metadata.is_file() {
            return self.file(source, target.to_owned(), None);
        }
        if !metadata.is_dir() || depth > 64 {
            return Err(problem(format!(
                "unsupported migration tree at '{}'",
                source.display()
            )));
        }
        if let Ok(existing) = fs::symlink_metadata(target)
            && !existing.is_dir()
        {
            return Err(problem(format!(
                "migration target is not a directory: {}",
                target.display()
            )));
        }
        let mut children = fs::read_dir(source)
            .map_err(problem)?
            .take(MAX_FILES.saturating_sub(self.entries) + 1)
            .collect::<Result<Vec<_>, _>>()
            .map_err(problem)?;
        self.entries += children.len();
        if self.entries > MAX_FILES {
            return Err(problem("migration exceeds the 100000-entry budget"));
        }
        children.sort_by_key(fs::DirEntry::file_name);
        for child in children {
            if child.file_name() == ".update.lock" {
                continue;
            }
            self.tree(&child.path(), &target.join(child.file_name()), depth + 1)?;
        }
        Ok(())
    }

    fn commit(self) -> Result<(), SourceConfigError> {
        let mut installed = Vec::new();
        let mut staged = Vec::new();
        let result = (|| {
            // All conflicts and content have been checked before any target file is installed.
            for copy in &self.copies {
                let parent = copy
                    .target
                    .parent()
                    .ok_or_else(|| problem("migration target has no parent"))?;
                fs::create_dir_all(parent).map_err(problem)?;
                let temporary = parent.join(format!(
                    ".mant-migrate-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .map_err(problem)?;
                staged.push(temporary.clone());
                file.write_all(&copy.bytes).map_err(problem)?;
                file.set_permissions(copy.permissions.clone())
                    .map_err(problem)?;
                file.sync_all().map_err(problem)?;
            }
            for (copy, temporary) in self.copies.iter().zip(&staged) {
                // A same-filesystem hard link publishes without overwriting a racing writer.
                fs::hard_link(temporary, &copy.target).map_err(problem)?;
                installed.push(copy.target.clone());
            }
            // Windows will not unlink a read-only hard link. Unlink our staging
            // names first, then restore each published file's original flags.
            for temporary in &staged {
                remove_owned_file(temporary).map_err(problem)?;
            }
            for copy in &self.copies {
                fs::set_permissions(&copy.target, copy.permissions.clone()).map_err(problem)?;
            }
            Ok(())
        })();
        if result.is_err() {
            for path in installed {
                let _ = remove_owned_file(&path);
            }
        }
        for path in staged {
            let _ = remove_owned_file(&path);
        }
        result
    }
}

fn remove_owned_file(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        if permissions.readonly() {
            // This Windows-only cleanup clears the DOS read-only flag on our
            // own copy; it never changes Unix mode bits or legacy originals.
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions)?;
        }
    }
    fs::remove_file(path)
}

fn problem(error: impl std::fmt::Display) -> SourceConfigError {
    SourceConfigError::new(format!("could not migrate legacy storage: {error}"))
}

/// Copy known configuration, documents, manual roots and source snapshots into
/// the new layout, preserving originals and refusing data conflicts.
/// Existing destination configuration wins. Relative local Git repositories
/// retain their old absolute meaning and matching installed metadata is updated.
///
/// # Errors
/// Reports invalid configuration, active updates, links/special files, conflicts,
/// exhausted copy budgets or I/O failures. Previously installed files are retained.
pub fn migrate_legacy_storage(old_root: &Path) -> Result<Settings, SourceConfigError> {
    migrate_with(old_root, Settings::load()?)
}

fn migrate_with(old_root: &Path, mut settings: Settings) -> Result<Settings, SourceConfigError> {
    if !old_root.is_absolute() || old_root.parent().is_none() {
        return Err(problem("legacy root must be a non-root absolute directory"));
    }
    let config = settings.config_home()?;
    let mut plan = Plan {
        copies: Vec::new(),
        bytes: 0,
        entries: 0,
        replacements: std::collections::BTreeMap::new(),
    };
    let old_mant = old_root.join("mant.toml");
    if !config.join("mant.toml").try_exists().map_err(problem)?
        && old_mant.try_exists().map_err(problem)?
    {
        let text = crate::bounded::read_file_utf8(&old_mant, 1024 * 1024, "mant.toml", false)
            .map_err(problem)?;
        settings.import_configuration(&text)?;
        plan.file(&old_mant, config.join("mant.toml"), None)?;
    }
    let data = settings.data_home()?;
    let _cache = settings.cache_home()?;
    let _manuals = settings.manual_paths()?;
    let old_sources = old_root.join("sources");
    let _old_lock = if real_directory(&old_sources)? {
        Some(crate::update::UpdateLock::acquire(&old_sources)?)
    } else {
        None
    };
    let _new_lock = if old_sources != data.join("sources") && real_directory(&data.join("sources"))?
    {
        Some(crate::update::UpdateLock::acquire(&data.join("sources"))?)
    } else {
        None
    };
    {
        let name = "man.conf";
        let source = old_root.join(name);
        if source.try_exists().map_err(problem)?
            && !config.join(name).try_exists().map_err(problem)?
        {
            plan.file(&source, config.join(name), None)?;
        }
    }
    prepare_sources(&mut plan, old_root, &data, &config)?;
    for name in ["documents", "sources", "man"] {
        plan.tree(&old_root.join(name), &data.join(name), 0)?;
    }
    plan.tree(&old_root.join("man.d"), &config.join("man.d"), 0)?;
    plan.commit()?;
    Ok(settings)
}

fn real_directory(path: &Path) -> Result<bool, SourceConfigError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err(problem(format!(
            "migration directory must not be a link or file: {}",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(problem(error)),
    }
}

fn prepare_sources(
    plan: &mut Plan,
    old_root: &Path,
    data: &Path,
    config: &Path,
) -> Result<(), SourceConfigError> {
    let old_sources = old_root.join("sources");
    let source_config = old_root.join("sources.toml");
    let target_config = config.join("sources.toml");
    let target = if target_config.try_exists().map_err(problem)? {
        Some(crate::config::load_source_config_from(&target_config)?)
    } else {
        None
    };
    let mut rewritten = if source_config.try_exists().map_err(problem)?
        && config.join("sources.toml").try_exists().map_err(problem)?
    {
        crate::config::load_source_config_from(&source_config)
            .ok()
            .zip(target)
    } else {
        None
    };
    if source_config.try_exists().map_err(problem)?
        && !config.join("sources.toml").try_exists().map_err(problem)?
    {
        let text =
            crate::bounded::read_file_utf8(&source_config, 1024 * 1024, "sources.toml", false)
                .map_err(problem)?;
        let old = crate::config::parse_source_config(&text, &source_config)?;
        let mut table: toml::Table = toml::from_str(&text).map_err(problem)?;
        let mut changed = false;
        if old_root != data {
            for declaration in table
                .iter_mut()
                .filter_map(|(_, value)| value.as_table_mut())
            {
                if let Some(toml::Value::String(repo)) = declaration.get_mut("repo")
                    && relative_local_repository(repo)
                {
                    let absolute = old_root.join(&*repo);
                    absolute
                        .to_str()
                        .ok_or_else(|| problem("local Git path is not UTF-8"))?
                        .clone_into(repo);
                    changed = true;
                }
            }
        }
        let updated = if changed {
            toml::to_string(&table).map_err(problem)?
        } else {
            text
        };
        let new = crate::config::parse_source_config(&updated, &config.join("sources.toml"))?;
        plan.file(
            &source_config,
            config.join("sources.toml"),
            Some(updated.into_bytes()),
        )?;
        rewritten = Some((old, new));
    }
    if let Some((old, new)) = rewritten {
        for name in old.sources().keys() {
            if let (Some(before), Some(after)) = (old.get(name), new.get(name)) {
                let mut expected = before.clone();
                if let crate::SourceLocation::Git { repo, .. } = &mut expected.location
                    && relative_local_repository(repo)
                {
                    *repo = old_root.join(&*repo).to_string_lossy().into_owned();
                }
                if before != after
                    && &expected == after
                    && let Ok(mut metadata) =
                        crate::metadata::read_source_metadata(&old_sources.join(name))
                {
                    metadata.relocate_configuration(name, before, after);
                    plan.replacements.insert(
                        old_sources.join(name).join(crate::SOURCE_METADATA_FILE),
                        toml::to_string(&metadata).map_err(problem)?.into_bytes(),
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn relative_local_repository(repo: &str) -> bool {
    // Git treats a colon as scp syntax only before the first path separator.
    !Path::new(repo).is_absolute()
        && repo.find(':').is_none_or(|colon| {
            repo[..colon].contains('/') || (cfg!(windows) && repo[..colon].contains('\\'))
        })
}
