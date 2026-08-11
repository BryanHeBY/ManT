//! Single-commit Git acquisition for one configured source.

use std::{ffi::OsStr, path::Path, process::Command};

use super::{
    SourceMetadata, SourceUpdateContext, SourceUpdateResult, UpdateWorkspace, activate_source,
    install_selected_documents,
};

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
        [
            OsStr::new("clone"),
            OsStr::new("--depth"),
            OsStr::new("1"),
            OsStr::new("--single-branch"),
            OsStr::new("--no-local"),
            OsStr::new("--no-tags"),
            OsStr::new("--branch"),
            OsStr::new(branch),
            OsStr::new("--"),
            OsStr::new(repo),
            workspace.checkout.as_os_str(),
        ],
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
    let output = Command::new("git")
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
        .output()
        .map_err(|error| format!("could not run git: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!("git exited with {}", output.status)
        } else {
            format!("git failed: {detail}")
        });
    }
    String::from_utf8(output.stdout).map_err(|_| "git output was not UTF-8".to_owned())
}
