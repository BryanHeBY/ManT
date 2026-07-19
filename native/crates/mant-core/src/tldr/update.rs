//! Performs the explicit, transactional tldr cache update operation.

use std::{
    collections::BTreeMap,
    env,
    error::Error,
    ffi::{OsStr, OsString},
    fmt, fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{self, Command},
    sync::atomic::{AtomicU64, Ordering},
};

use mant_ast::{TldrCacheAction, TldrCacheUpdate};

use crate::source::CommandOutput;

use super::cache::{HostPlatform, TldrCacheError, get_tldr_cache_dir};

const DEFAULT_REPOSITORY: &str = "https://github.com/tldr-pages/tldr.git";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Failure to refresh an installed client or `ManT`'s private checkout.
#[derive(Debug)]
pub enum TldrUpdateError {
    Cache(TldrCacheError),
    NoUpdater,
    InvalidCheckout(PathBuf),
    CommandUnavailable {
        program: PathBuf,
        source: io::Error,
    },
    CommandFailed {
        command: String,
        exit_code: i32,
        detail: Option<String>,
    },
    FileOperation {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for TldrUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cache(error) => error.fmt(formatter),
            Self::NoUpdater => {
                formatter.write_str("cannot update tldr pages: install a 'tldr' client or git")
            }
            Self::InvalidCheckout(path) => write!(
                formatter,
                "{} exists but is not a tldr git checkout",
                path.display()
            ),
            Self::CommandUnavailable { program, source } => {
                write!(formatter, "cannot run {}: {source}", program.display())
            }
            Self::CommandFailed {
                command,
                exit_code,
                detail,
            } => {
                if let Some(detail) = detail {
                    formatter.write_str(detail)
                } else {
                    write!(formatter, "{command} failed with code {exit_code}")
                }
            }
            Self::FileOperation {
                action,
                path,
                source,
            } => write!(formatter, "cannot {action} {}: {source}", path.display()),
        }
    }
}

impl Error for TldrUpdateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cache(error) => Some(error),
            Self::CommandUnavailable { source, .. } | Self::FileOperation { source, .. } => {
                Some(source)
            }
            Self::NoUpdater | Self::InvalidCheckout(_) | Self::CommandFailed { .. } => None,
        }
    }
}

impl From<TldrCacheError> for TldrUpdateError {
    fn from(error: TldrCacheError) -> Self {
        Self::Cache(error)
    }
}

/// Refresh tldr through an installed client or `ManT`'s private Git checkout.
///
/// # Errors
///
/// Returns [`TldrUpdateError`] when no updater is installed, a subprocess
/// fails, or the private cache cannot be changed transactionally.
pub fn update_tldr_cache() -> Result<TldrCacheUpdate, TldrUpdateError> {
    let environment = env::vars().collect::<BTreeMap<_, _>>();
    update_tldr_cache_with(
        &environment,
        HostPlatform::current()?,
        DEFAULT_REPOSITORY,
        &SystemUpdateHost,
    )
}

trait TldrUpdateHost {
    fn find_executable(
        &self,
        name: &str,
        environment: &BTreeMap<String, String>,
    ) -> Option<PathBuf>;
    fn exists(&self, path: &Path) -> bool;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn make_temp_dir(&self, prefix: &Path) -> io::Result<PathBuf>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;
    fn run(&self, program: &OsStr, arguments: &[OsString]) -> io::Result<CommandOutput>;
}

struct SystemUpdateHost;

impl TldrUpdateHost for SystemUpdateHost {
    fn find_executable(
        &self,
        name: &str,
        environment: &BTreeMap<String, String>,
    ) -> Option<PathBuf> {
        let path = environment.get("PATH")?;
        env::split_paths(OsStr::new(path))
            .map(|directory| directory.join(name))
            .find(|candidate| {
                candidate.metadata().is_ok_and(|metadata| {
                    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                })
            })
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn make_temp_dir(&self, prefix: &Path) -> io::Result<PathBuf> {
        let parent = prefix.parent().unwrap_or_else(|| Path::new("."));
        let name = prefix
            .file_name()
            .unwrap_or_else(|| OsStr::new("tldr-pages.tmp-"))
            .to_string_lossy();
        for _ in 0..100 {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let candidate = parent.join(format!("{name}{}-{sequence}", process::id()));
            match fs::create_dir(&candidate) {
                Ok(()) => return Ok(candidate),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique temporary tldr directory",
        ))
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)
    }

    fn run(&self, program: &OsStr, arguments: &[OsString]) -> io::Result<CommandOutput> {
        let output = Command::new(program).args(arguments).output()?;
        Ok(CommandOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

fn update_tldr_cache_with(
    environment: &BTreeMap<String, String>,
    platform: HostPlatform,
    repository: &str,
    host: &dyn TldrUpdateHost,
) -> Result<TldrCacheUpdate, TldrUpdateError> {
    if !environment.contains_key("MANT_TLDR_DIR") {
        if let Some(client) = host.find_executable("tldr", environment) {
            let output = run_checked(host, &client, &[OsString::from("--update")])?;
            let rendered_output = combined_output(&output);
            return Ok(TldrCacheUpdate {
                action: TldrCacheAction::Updated,
                cache_dir: None,
                client: Some(client.to_string_lossy().into_owned()),
                output: (!rendered_output.is_empty()).then_some(rendered_output),
                revision: None,
            });
        }
    }

    let git = host
        .find_executable("git", environment)
        .ok_or(TldrUpdateError::NoUpdater)?;
    let target = get_tldr_cache_dir(environment, platform)?;
    let action = if host.exists(&target) {
        if !host.exists(&target.join(".git")) {
            return Err(TldrUpdateError::InvalidCheckout(target));
        }
        run_checked(
            host,
            &git,
            &[
                OsString::from("-C"),
                target.as_os_str().to_owned(),
                OsString::from("pull"),
                OsString::from("--ff-only"),
            ],
        )?;
        TldrCacheAction::Updated
    } else {
        clone_cache(host, &git, repository, &target)?;
        TldrCacheAction::Cloned
    };

    let revision = host
        .run(
            git.as_os_str(),
            &[
                OsString::from("-C"),
                target.as_os_str().to_owned(),
                OsString::from("rev-parse"),
                OsString::from("--short"),
                OsString::from("HEAD"),
            ],
        )
        .ok()
        .filter(|output| output.exit_code == 0)
        .and_then(|output| first_nonempty_line(&output.stdout));

    Ok(TldrCacheUpdate {
        action,
        cache_dir: Some(target.to_string_lossy().into_owned()),
        client: None,
        output: None,
        revision,
    })
}

fn clone_cache(
    host: &dyn TldrUpdateHost,
    git: &Path,
    repository: &str,
    target: &Path,
) -> Result<(), TldrUpdateError> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    host.create_dir_all(parent)
        .map_err(|source| TldrUpdateError::FileOperation {
            action: "create directory",
            path: parent.to_owned(),
            source,
        })?;
    let prefix = parent.join(format!(
        "{}.tmp-",
        target
            .file_name()
            .unwrap_or_else(|| OsStr::new("tldr-pages"))
            .to_string_lossy()
    ));
    let temporary =
        host.make_temp_dir(&prefix)
            .map_err(|source| TldrUpdateError::FileOperation {
                action: "create temporary directory",
                path: prefix,
                source,
            })?;
    let clone_result = run_checked(
        host,
        git,
        &[
            OsString::from("clone"),
            OsString::from("--depth=1"),
            OsString::from("--single-branch"),
            OsString::from("--branch"),
            OsString::from("main"),
            OsString::from(repository),
            temporary.as_os_str().to_owned(),
        ],
    )
    .and_then(|_| {
        host.rename(&temporary, target)
            .map_err(|source| TldrUpdateError::FileOperation {
                action: "move completed tldr checkout to",
                path: target.to_owned(),
                source,
            })
    });
    if let Err(error) = clone_result {
        let _ = host.remove_dir_all(&temporary);
        return Err(error);
    }
    Ok(())
}

fn run_checked(
    host: &dyn TldrUpdateHost,
    program: &Path,
    arguments: &[OsString],
) -> Result<CommandOutput, TldrUpdateError> {
    let output = host.run(program.as_os_str(), arguments).map_err(|source| {
        TldrUpdateError::CommandUnavailable {
            program: program.to_owned(),
            source,
        }
    })?;
    if output.exit_code == 0 {
        return Ok(output);
    }
    let mut command = vec![program.to_string_lossy().into_owned()];
    command.extend(
        arguments
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned()),
    );
    Err(TldrUpdateError::CommandFailed {
        command: command.join(" "),
        exit_code: output.exit_code,
        detail: first_nonempty_line(&output.stderr),
    })
}

fn combined_output(output: &CommandOutput) -> String {
    [output.stdout.as_slice(), output.stderr.as_slice()]
        .into_iter()
        .filter_map(first_nonempty_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_nonempty_text(output: &[u8]) -> Option<String> {
    let value = String::from_utf8_lossy(output).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn first_nonempty_line(output: &[u8]) -> Option<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, HashMap, HashSet, VecDeque},
        ffi::{OsStr, OsString},
        io,
        path::{Path, PathBuf},
        sync::Mutex,
    };

    use mant_ast::{TldrCacheAction, TldrCacheUpdate};

    use crate::source::CommandOutput;

    use super::{HostPlatform, TldrUpdateError, TldrUpdateHost, update_tldr_cache_with};

    type Call = (PathBuf, Vec<OsString>);

    struct StubHost {
        executables: HashMap<String, PathBuf>,
        existing: HashSet<PathBuf>,
        outputs: Mutex<VecDeque<io::Result<CommandOutput>>>,
        calls: Mutex<Vec<Call>>,
        created: Mutex<Vec<PathBuf>>,
        temporary: PathBuf,
        renames: Mutex<Vec<(PathBuf, PathBuf)>>,
        removals: Mutex<Vec<PathBuf>>,
        cleanup_error: bool,
    }

    impl StubHost {
        fn new(outputs: Vec<CommandOutput>) -> Self {
            Self {
                executables: HashMap::new(),
                existing: HashSet::new(),
                outputs: Mutex::new(outputs.into_iter().map(Ok).collect()),
                calls: Mutex::new(Vec::new()),
                created: Mutex::new(Vec::new()),
                temporary: PathBuf::from("/cache/mant/tldr-pages.tmp-1"),
                renames: Mutex::new(Vec::new()),
                removals: Mutex::new(Vec::new()),
                cleanup_error: false,
            }
        }
    }

    impl TldrUpdateHost for StubHost {
        fn find_executable(
            &self,
            name: &str,
            _environment: &BTreeMap<String, String>,
        ) -> Option<PathBuf> {
            self.executables.get(name).cloned()
        }

        fn exists(&self, path: &Path) -> bool {
            self.existing.contains(path)
        }

        fn create_dir_all(&self, path: &Path) -> io::Result<()> {
            self.created
                .lock()
                .expect("created paths lock")
                .push(path.to_owned());
            Ok(())
        }

        fn make_temp_dir(&self, _prefix: &Path) -> io::Result<PathBuf> {
            Ok(self.temporary.clone())
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.renames
                .lock()
                .expect("rename calls lock")
                .push((from.to_owned(), to.to_owned()));
            Ok(())
        }

        fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
            self.removals
                .lock()
                .expect("removal calls lock")
                .push(path.to_owned());
            if self.cleanup_error {
                Err(io::Error::other("cleanup failed"))
            } else {
                Ok(())
            }
        }

        fn run(&self, program: &OsStr, arguments: &[OsString]) -> io::Result<CommandOutput> {
            self.calls
                .lock()
                .expect("command calls lock")
                .push((PathBuf::from(program), arguments.to_vec()));
            self.outputs
                .lock()
                .expect("command outputs lock")
                .pop_front()
                .unwrap_or_else(|| Ok(CommandOutput::default()))
        }
    }

    fn environment(values: &[(&str, &str)]) -> BTreeMap<String, String> {
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn success(stdout: &str) -> CommandOutput {
        CommandOutput {
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            exit_code: 0,
        }
    }

    #[test]
    fn installed_client_owns_its_update() {
        let mut host = StubHost::new(vec![success("Updated cache for language en\n")]);
        host.executables
            .insert("tldr".to_owned(), PathBuf::from("/usr/bin/tldr"));

        let result = update_tldr_cache_with(
            &environment(&[("HOME", "/home/test")]),
            HostPlatform::Linux,
            "unused",
            &host,
        )
        .expect("client update");

        assert_eq!(
            result,
            TldrCacheUpdate {
                action: TldrCacheAction::Updated,
                cache_dir: None,
                client: Some("/usr/bin/tldr".to_owned()),
                output: Some("Updated cache for language en".to_owned()),
                revision: None,
            }
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            [(
                PathBuf::from("/usr/bin/tldr"),
                vec![OsString::from("--update")]
            )]
        );
    }

    #[test]
    fn installed_client_failure_uses_its_diagnostic() {
        let mut host = StubHost::new(vec![CommandOutput {
            stdout: Vec::new(),
            stderr: b"Unable to update cache\n".to_vec(),
            exit_code: 1,
        }]);
        host.executables
            .insert("tldr".to_owned(), PathBuf::from("/usr/bin/tldr"));

        let error = update_tldr_cache_with(
            &environment(&[("HOME", "/home/test")]),
            HostPlatform::Linux,
            "unused",
            &host,
        )
        .expect_err("client update must fail");

        assert_eq!(error.to_string(), "Unable to update cache");
    }

    #[test]
    fn clones_transactionally_then_reports_revision() {
        let mut host = StubHost::new(vec![success(""), success("abc123\n")]);
        host.executables
            .insert("git".to_owned(), PathBuf::from("/usr/bin/git"));

        let result = update_tldr_cache_with(
            &environment(&[("HOME", "/home/test"), ("XDG_CACHE_HOME", "/cache")]),
            HostPlatform::Linux,
            "https://example.test/tldr.git",
            &host,
        )
        .expect("clone cache");

        assert_eq!(result.action, TldrCacheAction::Cloned);
        assert_eq!(result.cache_dir.as_deref(), Some("/cache/mant/tldr-pages"));
        assert_eq!(result.revision.as_deref(), Some("abc123"));
        assert_eq!(
            *host.created.lock().expect("created lock"),
            [PathBuf::from("/cache/mant")]
        );
        assert_eq!(
            *host.renames.lock().expect("renames lock"),
            [(
                PathBuf::from("/cache/mant/tldr-pages.tmp-1"),
                PathBuf::from("/cache/mant/tldr-pages")
            )]
        );
        let calls = host.calls.lock().expect("calls lock");
        assert_eq!(calls[0].1[0], "clone");
        assert_eq!(calls[0].1[5], "https://example.test/tldr.git");
    }

    #[test]
    fn explicit_checkout_updates_without_using_installed_client() {
        let target = PathBuf::from("/custom/tldr");
        let mut host = StubHost::new(vec![success(""), success("def456\n")]);
        host.executables
            .insert("tldr".to_owned(), PathBuf::from("/usr/bin/tldr"));
        host.executables
            .insert("git".to_owned(), PathBuf::from("/usr/bin/git"));
        host.existing.extend([target.clone(), target.join(".git")]);

        let result = update_tldr_cache_with(
            &environment(&[("HOME", "/home/test"), ("MANT_TLDR_DIR", "/custom/tldr")]),
            HostPlatform::Linux,
            "unused",
            &host,
        )
        .expect("pull cache");

        assert_eq!(result.action, TldrCacheAction::Updated);
        let calls = host.calls.lock().expect("calls lock");
        assert_eq!(
            calls[0].1,
            ["-C", "/custom/tldr", "pull", "--ff-only"].map(OsString::from)
        );
    }

    #[test]
    fn preserves_clone_failure_even_when_cleanup_fails() {
        let mut host = StubHost::new(vec![CommandOutput {
            stdout: Vec::new(),
            stderr: b"network unavailable\n".to_vec(),
            exit_code: 128,
        }]);
        host.executables
            .insert("git".to_owned(), PathBuf::from("/usr/bin/git"));
        host.cleanup_error = true;

        let error = update_tldr_cache_with(
            &environment(&[("HOME", "/home/test"), ("XDG_CACHE_HOME", "/cache")]),
            HostPlatform::Linux,
            "https://example.test/tldr.git",
            &host,
        )
        .expect_err("clone must fail");

        assert!(matches!(error, TldrUpdateError::CommandFailed { .. }));
        assert_eq!(error.to_string(), "network unavailable");
        assert_eq!(
            *host.removals.lock().expect("removals lock"),
            [PathBuf::from("/cache/mant/tldr-pages.tmp-1")]
        );
    }

    #[test]
    fn rejects_an_existing_non_checkout_before_running_git() {
        let target = PathBuf::from("/custom/tldr");
        let mut host = StubHost::new(Vec::new());
        host.executables
            .insert("git".to_owned(), PathBuf::from("/usr/bin/git"));
        host.existing.insert(target);

        let error = update_tldr_cache_with(
            &environment(&[("MANT_TLDR_DIR", "/custom/tldr")]),
            HostPlatform::Linux,
            "unused",
            &host,
        )
        .expect_err("non-checkout must fail");

        assert_eq!(
            error.to_string(),
            "/custom/tldr exists but is not a tldr git checkout"
        );
        assert!(host.calls.lock().expect("calls lock").is_empty());
    }
}
