//! Locates the original source file selected by the host's `man` database.

use std::{
    ffi::{OsStr, OsString},
    fmt, io,
    os::unix::ffi::OsStringExt,
    path::PathBuf,
    process::Command,
};

/// One validated manual lookup independent from CLI token syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualRequest {
    pub topic: String,
    pub section: Option<String>,
}

impl ManualRequest {
    #[must_use]
    pub fn new(topic: impl Into<String>, section: Option<String>) -> Self {
        Self {
            topic: topic.into(),
            section,
        }
    }
}

/// Minimal subprocess result used by deterministic source-locator tests.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

/// Injectable boundary around process execution.
pub trait CommandRunner {
    /// Run one executable with already-separated arguments.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the executable cannot be started or waited.
    fn run(&self, program: &OsStr, arguments: &[OsString]) -> io::Result<CommandOutput>;
}

/// Production runner backed by [`std::process::Command`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, program: &OsStr, arguments: &[OsString]) -> io::Result<CommandOutput> {
        let output = Command::new(program).args(arguments).output()?;
        Ok(CommandOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

/// Expected source-discovery failures suitable for a user-facing CLI error.
#[derive(Debug)]
pub enum LocateError {
    EmptyTopic,
    InvalidSection,
    CommandUnavailable(io::Error),
    NotFound {
        topic: String,
        detail: Option<String>,
    },
    EmptyResult {
        topic: String,
    },
}

impl fmt::Display for LocateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTopic => formatter.write_str("manual topic must not be empty"),
            Self::InvalidSection => formatter.write_str("manual section must not be empty"),
            Self::CommandUnavailable(error) if error.kind() == io::ErrorKind::NotFound => {
                formatter.write_str("cannot locate manuals: the 'man' command is not installed")
            }
            Self::CommandUnavailable(error) => write!(formatter, "could not run 'man -w': {error}"),
            Self::NotFound { topic, detail } => {
                write!(formatter, "no local manual source was found for '{topic}'")?;
                if let Some(detail) = detail {
                    write!(formatter, ": {detail}")?;
                }
                Ok(())
            }
            Self::EmptyResult { topic } => {
                write!(formatter, "man returned no source path for '{topic}'")
            }
        }
    }
}

impl std::error::Error for LocateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CommandUnavailable(error) => Some(error),
            _ => None,
        }
    }
}

/// Locate a manual through the host's configured man database.
///
/// # Errors
///
/// Returns [`LocateError`] for invalid requests, unavailable host tooling,
/// failed lookups, and successful commands that return no path.
pub fn locate_manual_source(request: &ManualRequest) -> Result<PathBuf, LocateError> {
    locate_manual_source_with(request, &SystemCommandRunner)
}

/// Injectable form of [`locate_manual_source`] used by native unit tests.
///
/// # Errors
///
/// Returns the same [`LocateError`] variants as [`locate_manual_source`].
pub fn locate_manual_source_with(
    request: &ManualRequest,
    runner: &impl CommandRunner,
) -> Result<PathBuf, LocateError> {
    let topic = request.topic.trim();
    if topic.is_empty() {
        return Err(LocateError::EmptyTopic);
    }

    let mut arguments = vec![OsString::from("-w")];
    if let Some(section) = request.section.as_deref() {
        let section = section.trim();
        if section.is_empty() {
            return Err(LocateError::InvalidSection);
        }
        arguments.push(OsString::from(section));
    }
    // Terminate option parsing so a topic beginning with '-' is treated as a
    // positional operand rather than an option by man.
    arguments.push(OsString::from("--"));
    arguments.push(OsString::from(topic));

    let output = runner
        .run(OsStr::new("man"), &arguments)
        .map_err(LocateError::CommandUnavailable)?;
    if output.exit_code != 0 {
        return Err(LocateError::NotFound {
            topic: topic.to_owned(),
            detail: first_nonempty_line(&output.stderr),
        });
    }

    let path = first_line_bytes(&output.stdout).ok_or_else(|| LocateError::EmptyResult {
        topic: topic.to_owned(),
    })?;
    Ok(PathBuf::from(OsString::from_vec(path.to_vec())))
}

fn first_line_bytes(output: &[u8]) -> Option<&[u8]> {
    let line = output.split(|byte| *byte == b'\n').next()?;
    let line = trim_ascii(line);
    (!line.is_empty()).then_some(line)
}

fn first_nonempty_line(output: &[u8]) -> Option<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::{OsStr, OsString},
        io,
        sync::Mutex,
    };

    use super::{
        CommandOutput, CommandRunner, LocateError, ManualRequest, locate_manual_source_with,
    };

    struct StubRunner {
        output: CommandOutput,
        calls: Mutex<Vec<(OsString, Vec<OsString>)>>,
    }

    impl StubRunner {
        fn returning(output: CommandOutput) -> Self {
            Self {
                output,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl CommandRunner for StubRunner {
        fn run(&self, program: &OsStr, arguments: &[OsString]) -> io::Result<CommandOutput> {
            self.calls
                .lock()
                .expect("recorded calls lock")
                .push((program.to_owned(), arguments.to_vec()));
            Ok(self.output.clone())
        }
    }

    #[test]
    fn locates_the_first_path_and_passes_an_optional_section() {
        let runner = StubRunner::returning(CommandOutput {
            stdout: b" /usr/share/man/man1/printf.1.gz\n/other/path\n".to_vec(),
            stderr: Vec::new(),
            exit_code: 0,
        });
        let request = ManualRequest::new("printf", Some("1p".to_owned()));

        let path = locate_manual_source_with(&request, &runner).expect("locate source");

        assert_eq!(
            path,
            std::path::Path::new("/usr/share/man/man1/printf.1.gz")
        );
        assert_eq!(
            *runner.calls.lock().expect("recorded calls lock"),
            vec![(
                OsString::from("man"),
                vec![
                    OsString::from("-w"),
                    OsString::from("1p"),
                    OsString::from("--"),
                    OsString::from("printf")
                ]
            )]
        );
    }

    #[test]
    fn passes_a_dash_prefixed_topic_after_an_option_terminator() {
        let runner = StubRunner::returning(CommandOutput {
            stdout: b"/usr/share/man/man1/-dash.1.gz\n".to_vec(),
            stderr: Vec::new(),
            exit_code: 0,
        });

        locate_manual_source_with(&ManualRequest::new("-x", None), &runner).expect("locate source");

        assert_eq!(
            *runner.calls.lock().expect("recorded calls lock"),
            vec![(
                OsString::from("man"),
                vec![
                    OsString::from("-w"),
                    OsString::from("--"),
                    OsString::from("-x")
                ]
            )]
        );
    }

    #[test]
    fn reports_man_diagnostics_without_runtime_debug_output() {
        let runner = StubRunner::returning(CommandOutput {
            stdout: Vec::new(),
            stderr: b"No manual entry for definitely-missing\ntrace noise\n".to_vec(),
            exit_code: 16,
        });

        let error =
            locate_manual_source_with(&ManualRequest::new("definitely-missing", None), &runner)
                .expect_err("lookup must fail");

        assert!(matches!(error, LocateError::NotFound { .. }));
        assert_eq!(
            error.to_string(),
            "no local manual source was found for 'definitely-missing': No manual entry for definitely-missing"
        );
    }

    #[test]
    fn validates_the_request_before_starting_man() {
        let runner = StubRunner::returning(CommandOutput::default());

        assert!(matches!(
            locate_manual_source_with(&ManualRequest::new("  ", None), &runner),
            Err(LocateError::EmptyTopic)
        ));
        assert!(matches!(
            locate_manual_source_with(&ManualRequest::new("git", Some(" ".to_owned())), &runner),
            Err(LocateError::InvalidSection)
        ));
        assert!(runner.calls.lock().expect("recorded calls lock").is_empty());
    }
}
