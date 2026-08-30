//! Single-commit Git acquisition for one configured source.

use std::{
    ffi::OsStr,
    path::Path,
    process::{Command, Stdio},
    thread,
};

use super::{
    SourceMetadata, SourceUpdateContext, SourceUpdateResult, UpdateWorkspace, activate_source,
    install_selected_documents, source_selects_markdown_path,
};

const MAX_GIT_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GitObjectFilter {
    Blobless,
    None,
}

const fn native_object_filter() -> GitObjectFilter {
    if cfg!(windows) {
        GitObjectFilter::None
    } else {
        GitObjectFilter::Blobless
    }
}

pub(super) fn update(
    context: &SourceUpdateContext<'_>,
    repo: &str,
    branch: &str,
) -> Result<SourceUpdateResult, String> {
    let revision = remote_revision(&context.paths.root, repo, branch)?;
    if let Some(metadata) = &context.metadata
        && metadata.revision() == revision
    {
        return Ok(context.unchanged(revision));
    }

    let workspace = UpdateWorkspace::new(&context.paths.sources, context.name)?;
    run_git(
        &context.paths.root,
        clone_arguments(
            OsStr::new(repo),
            OsStr::new(branch),
            workspace.checkout.as_os_str(),
            native_object_filter(),
        ),
    )?;
    let checked_out = git_revision(&context.paths.root, &workspace.checkout)?;
    if checked_out != revision {
        return Err(format!(
            "remote branch moved while updating (expected {revision}, checked out {checked_out}); retry"
        ));
    }
    let commit_count = run_git(
        &context.paths.root,
        [
            OsStr::new("-C"),
            workspace.checkout.as_os_str(),
            OsStr::new("rev-list"),
            OsStr::new("--count"),
            OsStr::new("HEAD"),
        ],
    )?;
    if commit_count.trim() != "1" {
        return Err("git did not produce the required single-commit checkout".to_owned());
    }
    reject_selected_symlink_documents(
        &context.paths.root,
        &workspace.checkout,
        context.configured,
    )?;
    materialize_configured_path(
        &context.paths.root,
        &workspace.checkout,
        &context.configured.path,
    )?;
    workspace.create_staging()?;
    let documents =
        install_selected_documents(&workspace.checkout, &workspace.staging, context.configured)?;
    let document_count = u32::try_from(documents).unwrap_or(u32::MAX);
    let metadata = SourceMetadata::git(
        context.name,
        repo,
        branch,
        revision.clone(),
        &context.fingerprint,
        document_count,
    );
    activate_source(&workspace.staging, &context.target, &metadata)?;
    Ok(context.updated(revision, document_count))
}

fn clone_arguments<'a>(
    repo: &'a OsStr,
    branch: &'a OsStr,
    checkout: &'a OsStr,
    object_filter: GitObjectFilter,
) -> Vec<&'a OsStr> {
    let mut arguments = vec![
        OsStr::new("clone"),
        OsStr::new("--depth"),
        OsStr::new("1"),
        OsStr::new("--single-branch"),
        OsStr::new("--no-local"),
        OsStr::new("--no-tags"),
    ];
    if object_filter == GitObjectFilter::Blobless {
        arguments.push(OsStr::new("--filter=blob:none"));
    }
    arguments.extend([
        OsStr::new("--no-checkout"),
        OsStr::new("--branch"),
        branch,
        OsStr::new("--"),
        repo,
        checkout,
    ]);
    arguments
}

fn materialize_configured_path(
    working_directory: &Path,
    checkout: &Path,
    configured_path: &str,
) -> Result<(), String> {
    let pathspec = if configured_path == "." {
        ".".to_owned()
    } else {
        format!(":(literal){configured_path}")
    };
    run_git(
        working_directory,
        [
            OsStr::new("-C"),
            checkout.as_os_str(),
            OsStr::new("checkout"),
            OsStr::new("--force"),
            OsStr::new("HEAD"),
            OsStr::new("--"),
            OsStr::new(&pathspec),
        ],
    )?;
    Ok(())
}

fn reject_selected_symlink_documents(
    working_directory: &Path,
    checkout: &Path,
    source: &crate::ConfiguredSource,
) -> Result<(), String> {
    let tree = run_git_bytes(
        working_directory,
        [
            OsStr::new("-C"),
            checkout.as_os_str(),
            OsStr::new("ls-tree"),
            OsStr::new("-rz"),
            OsStr::new("--full-tree"),
            OsStr::new("HEAD"),
        ],
    )?;
    if let Some(path) = selected_symlink_document(&tree, source)? {
        return Err(format!(
            "configured Git source selects symbolic-link Markdown document '{path}'; publish a regular file instead"
        ));
    }
    Ok(())
}

fn selected_symlink_document(
    tree: &[u8],
    source: &crate::ConfiguredSource,
) -> Result<Option<String>, String> {
    let configured_root = (source.path != ".").then(|| Path::new(&source.path));
    for record in tree
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let Some(separator) = record.iter().position(|byte| *byte == b'\t') else {
            return Err("git returned malformed tree metadata".to_owned());
        };
        let metadata = &record[..separator];
        let path = &record[separator + 1..];
        if !metadata.starts_with(b"120000 ") {
            continue;
        }
        let path = std::str::from_utf8(path)
            .map_err(|_| "Git source contains a symbolic link with a non-UTF-8 path".to_owned())?;
        let path = Path::new(path);
        if configured_root.is_some_and(|root| root == path || root.starts_with(path)) {
            return Err(format!(
                "configured path '{}' traverses Git symbolic link '{}'",
                source.path,
                path.display()
            ));
        }
        let Some(relative) =
            configured_root.map_or(Some(path), |root| path.strip_prefix(root).ok())
        else {
            continue;
        };
        if source_selects_markdown_path(source, relative) {
            return Ok(Some(path.to_string_lossy().into_owned()));
        }
    }
    Ok(None)
}

fn remote_revision(working_directory: &Path, repo: &str, branch: &str) -> Result<String, String> {
    let reference = format!("refs/heads/{branch}");
    let output = run_git(
        working_directory,
        [
            OsStr::new("ls-remote"),
            OsStr::new("--exit-code"),
            OsStr::new("--refs"),
            OsStr::new("--"),
            OsStr::new(repo),
            OsStr::new(&reference),
        ],
    )?;
    let line = output
        .lines()
        .next()
        .ok_or_else(|| format!("branch '{branch}' was not found in '{repo}'"))?;
    let revision = line.split_whitespace().next().unwrap_or_default();
    if !matches!(revision.len(), 40 | 64) || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("git returned an invalid remote revision".to_owned());
    }
    Ok(revision.to_ascii_lowercase())
}

fn git_revision(working_directory: &Path, checkout: &Path) -> Result<String, String> {
    let output = run_git(
        working_directory,
        [
            OsStr::new("-C"),
            checkout.as_os_str(),
            OsStr::new("rev-parse"),
            OsStr::new("HEAD"),
        ],
    )?;
    Ok(output.trim().to_ascii_lowercase())
}

fn run_git<'a>(
    working_directory: &Path,
    arguments: impl IntoIterator<Item = &'a OsStr>,
) -> Result<String, String> {
    let stdout = run_git_bytes(working_directory, arguments)?;
    String::from_utf8(stdout).map_err(|_| "git output was not UTF-8".to_owned())
}

fn run_git_bytes<'a>(
    working_directory: &Path,
    arguments: impl IntoIterator<Item = &'a OsStr>,
) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    command
        .args([
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.https.allow=always",
            "-c",
            "protocol.ssh.allow=always",
            "-c",
            "protocol.file.allow=always",
        ])
        .args(arguments)
        .current_dir(working_directory)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PROTOCOL_FROM_USER", "0")
        .env("GIT_ALLOW_PROTOCOL", "https:ssh:file")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not run git: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "could not capture git stdout".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "could not capture git stderr".to_owned())?;
    let stdout_reader = thread::spawn(move || {
        crate::bounded::read_bytes(stdout, MAX_GIT_OUTPUT_BYTES, "git stdout")
    });
    let stderr_reader = thread::spawn(move || {
        crate::bounded::read_bytes(stderr, MAX_GIT_OUTPUT_BYTES, "git stderr")
    });
    let status = child
        .wait()
        .map_err(|error| format!("could not wait for git: {error}"))?;
    let stdout = stdout_reader
        .join()
        .map_err(|_| "git stdout reader panicked".to_owned())?
        .map_err(|error| format!("could not read git stdout: {error}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "git stderr reader panicked".to_owned())?
        .map_err(|error| format!("could not read git stderr: {error}"))?;
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!("git exited with {status}")
        } else {
            format!("git failed: {detail}")
        });
    }
    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use crate::{ConfiguredSource, SourceLocation};

    use super::{
        GitObjectFilter, clone_arguments, native_object_filter, selected_symlink_document,
    };

    #[test]
    fn complete_object_clone_retains_shallow_bounds() {
        let arguments = clone_arguments(
            OsStr::new("repo"),
            OsStr::new("main"),
            OsStr::new("checkout"),
            GitObjectFilter::None,
        );

        assert!(!arguments.contains(&OsStr::new("--filter=blob:none")));
        for required in [
            "--depth",
            "1",
            "--single-branch",
            "--no-local",
            "--no-tags",
            "--no-checkout",
        ] {
            assert!(
                arguments.contains(&OsStr::new(required)),
                "missing {required}"
            );
        }
    }

    #[test]
    fn native_object_filter_avoids_blobless_clones_on_windows() {
        assert_eq!(
            native_object_filter(),
            if cfg!(windows) {
                GitObjectFilter::None
            } else {
                GitObjectFilter::Blobless
            }
        );
    }

    #[test]
    fn blobless_clone_changes_only_the_object_filter() {
        let filtered = clone_arguments(
            OsStr::new("repo"),
            OsStr::new("main"),
            OsStr::new("checkout"),
            GitObjectFilter::Blobless,
        );
        let complete = clone_arguments(
            OsStr::new("repo"),
            OsStr::new("main"),
            OsStr::new("checkout"),
            GitObjectFilter::None,
        );

        assert!(filtered.contains(&OsStr::new("--filter=blob:none")));
        assert_eq!(filtered.len(), complete.len() + 1);
        assert_eq!(
            filtered
                .into_iter()
                .filter(|argument| *argument != OsStr::new("--filter=blob:none"))
                .collect::<Vec<_>>(),
            complete
        );
    }

    fn source(path: &str) -> ConfiguredSource {
        ConfiguredSource {
            location: SourceLocation::Git {
                repo: "repo".to_owned(),
                branch: "main".to_owned(),
            },
            path: path.to_owned(),
            include: Vec::new(),
            exclude: vec!["drafts".to_owned()],
            priority: 1,
        }
    }

    #[test]
    fn git_tree_modes_reject_only_selected_markdown_links() {
        let tree = b"100644 blob aaaa\tdocs/regular.md\0\
120000 blob bbbb\tdocs/linked.md\0\
120000 blob cccc\tdocs/drafts/ignored.md\0\
120000 blob dddd\tdocs/image.png\0";

        assert_eq!(
            selected_symlink_document(tree, &source("docs")).expect("inspect tree"),
            Some("docs/linked.md".to_owned())
        );
    }

    #[test]
    fn configured_paths_cannot_traverse_git_links() {
        let tree = b"120000 blob aaaa\tdocs\0";
        let error = selected_symlink_document(tree, &source("docs/reference"))
            .expect_err("reject linked configured root");
        assert!(error.contains("traverses Git symbolic link 'docs'"));
    }

    #[test]
    fn unrelated_git_links_do_not_affect_a_source() {
        let tree = b"120000 blob aaaa\twebsite/index.md\0\
120000 blob bbbb\tdocs/drafts/ignored.md\0\
120000 blob cccc\tdocs/image.png\0";

        assert_eq!(
            selected_symlink_document(tree, &source("docs")).expect("inspect tree"),
            None
        );
    }
}
