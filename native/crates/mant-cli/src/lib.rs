//! Public process boundary for Mant's native document CLI.
//!
//! `mant-cli` is both an agent-friendly command and the versioned stdio
//! backend used by the interactive TypeScript application. Standard output is
//! reserved for the requested document; diagnostics go to standard error.

mod arguments;

use std::io::{self, Read, Write};

use mant_ast::{QueryBundle, QueryRequest, TldrCacheUpdate};
use mant_core::QueryError;
use serde::Serialize;

use arguments::{Command, HELP, QueryFormat, QuerySource, UsageError};

// ── Stable process protocol ────────────────────────────────────────────────

/// Exact stdio protocol understood by the TypeScript client.
pub const CLI_PROTOCOL_VERSION: &str = "mant.cli/v1";

const MAX_REQUEST_BYTES: u64 = 64 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolDescription<'a> {
    protocol: &'a str,
    native_api_version: &'a str,
    query_schema: &'a str,
    document_schema: &'a str,
}

// ── Host boundary ─────────────────────────────────────────────────────────

trait CliHost {
    fn query(&self, request: &QueryRequest) -> Result<QueryBundle, Failure>;
    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure>;
}

struct SystemHost;

impl CliHost for SystemHost {
    fn query(&self, request: &QueryRequest) -> Result<QueryBundle, Failure> {
        mant_core::query(request).map_err(|error| match error {
            QueryError::EmptyTopic | QueryError::InvalidSection => Failure::usage(error),
            _ => Failure::operational(error),
        })
    }

    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure> {
        mant_core::update_tldr_cache().map_err(Failure::operational)
    }
}

// ── Process execution ─────────────────────────────────────────────────────

/// Run one CLI invocation using explicit streams and return its exit status.
///
/// Keeping the process streams injectable makes malformed protocol requests
/// testable without consulting the host man database or tldr client.
pub fn run(
    arguments: &[String],
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> u8 {
    run_with_host(arguments, input, output, diagnostics, &SystemHost)
}

fn run_with_host(
    arguments: &[String],
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
) -> u8 {
    let command = match arguments::parse(arguments) {
        Ok(command) => command,
        Err(error) => return report_failure(&error.into(), diagnostics),
    };

    let rendered = match execute(command, input, host) {
        Ok(rendered) => rendered,
        Err(error) => return report_failure(&error, diagnostics),
    };

    match write_output(output, &rendered) {
        Ok(()) => 0,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => report_failure(&Failure::operational(error), diagnostics),
    }
}

fn execute(command: Command, input: &mut dyn Read, host: &dyn CliHost) -> Result<String, Failure> {
    match command {
        Command::Help => Ok(HELP.to_owned()),
        Command::ProtocolVersion { pretty } => render_json(
            &ProtocolDescription {
                protocol: CLI_PROTOCOL_VERSION,
                native_api_version: mant_core::native_api_version(),
                query_schema: "mant.query/v1",
                document_schema: "mant.document/v1",
            },
            pretty,
        ),
        Command::UpdateTldr { pretty } => {
            let update = host.update_tldr()?;
            mant_core::render_update_json(&update, pretty).map_err(Failure::operational)
        }
        Command::Query {
            source,
            format,
            pretty,
        } => {
            let request = read_query_request(source, input)?;
            validate_query_request(&request)?;
            let query = host.query(&request)?;
            match format {
                QueryFormat::Markdown => Ok(mant_core::render_markdown(&query)),
                QueryFormat::Json => {
                    mant_core::render_query_json(&query, pretty).map_err(Failure::operational)
                }
            }
        }
    }
}

fn read_query_request(source: QuerySource, input: &mut dyn Read) -> Result<QueryRequest, Failure> {
    if let QuerySource::Arguments(request) = source {
        return Ok(request);
    }

    let mut bytes = Vec::new();
    input
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Failure::usage(format!("cannot read request JSON: {error}")))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_REQUEST_BYTES {
        return Err(Failure::usage(format!(
            "request JSON exceeds the {MAX_REQUEST_BYTES}-byte limit"
        )));
    }
    let request =
        std::str::from_utf8(&bytes).map_err(|_| Failure::usage("request JSON must be UTF-8"))?;
    serde_json::from_str(request)
        .map_err(|error| Failure::usage(format!("invalid query request JSON: {error}")))
}

fn validate_query_request(request: &QueryRequest) -> Result<(), Failure> {
    if request.topic.trim().is_empty() {
        return Err(Failure::usage("manual topic must not be empty"));
    }
    if request
        .section
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(Failure::usage("manual section must not be empty"));
    }
    Ok(())
}

fn render_json(value: &impl Serialize, pretty: bool) -> Result<String, Failure> {
    if pretty {
        serde_json::to_string_pretty(value).map_err(Failure::operational)
    } else {
        serde_json::to_string(value).map_err(Failure::operational)
    }
}

fn write_output(output: &mut dyn Write, rendered: &str) -> io::Result<()> {
    output.write_all(rendered.as_bytes())?;
    if !rendered.ends_with('\n') {
        output.write_all(b"\n")?;
    }
    output.flush()
}

// ── Concise error presentation ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureKind {
    Usage,
    Operational,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Failure {
    kind: FailureKind,
    message: String,
}

impl Failure {
    fn usage(message: impl std::fmt::Display) -> Self {
        Self {
            kind: FailureKind::Usage,
            message: message.to_string(),
        }
    }

    fn operational(message: impl std::fmt::Display) -> Self {
        Self {
            kind: FailureKind::Operational,
            message: message.to_string(),
        }
    }
}

impl From<UsageError> for Failure {
    fn from(error: UsageError) -> Self {
        Self::usage(error.0)
    }
}

fn report_failure(error: &Failure, diagnostics: &mut dyn Write) -> u8 {
    let _ = writeln!(diagnostics, "mant-cli: {}", error.message);
    if error.kind == FailureKind::Usage {
        let _ = writeln!(diagnostics, "Try 'mant-cli --help' for more information.");
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use mant_ast::{QueryBundle, QueryRequest, QuerySchema, TldrCacheAction, TldrCacheUpdate};

    use super::{CLI_PROTOCOL_VERSION, CliHost, Failure, run_with_host};

    struct FakeHost {
        query_calls: Cell<usize>,
        update_calls: Cell<usize>,
    }

    impl FakeHost {
        fn new() -> Self {
            Self {
                query_calls: Cell::new(0),
                update_calls: Cell::new(0),
            }
        }
    }

    impl CliHost for FakeHost {
        fn query(&self, request: &QueryRequest) -> Result<QueryBundle, Failure> {
            self.query_calls.set(self.query_calls.get() + 1);
            Ok(QueryBundle {
                schema: QuerySchema::V1,
                topic: request.topic.trim().to_owned(),
                section: request.section.clone(),
                manual: None,
                tldr: None,
            })
        }

        fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure> {
            self.update_calls.set(self.update_calls.get() + 1);
            Ok(TldrCacheUpdate {
                action: TldrCacheAction::Updated,
                cache_dir: Some("/cache/tldr".to_owned()),
                client: None,
                output: None,
                revision: Some("abc123".to_owned()),
            })
        }
    }

    fn invoke(arguments: &[&str], input: &[u8], host: &FakeHost) -> (u8, String, String) {
        let arguments = arguments
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let mut input = input;
        let mut output = Vec::new();
        let mut diagnostics = Vec::new();
        let status = run_with_host(&arguments, &mut input, &mut output, &mut diagnostics, host);
        (
            status,
            String::from_utf8(output).expect("UTF-8 output"),
            String::from_utf8(diagnostics).expect("UTF-8 diagnostics"),
        )
    }

    #[test]
    fn stdin_protocol_emits_only_compact_query_json() {
        let host = FakeHost::new();
        let (status, output, diagnostics) = invoke(
            &["--request-json", "--json", "--compact"],
            br#"{"topic":"git","section":"1"}"#,
            &host,
        );

        assert_eq!(status, 0);
        assert_eq!(
            output,
            "{\"schema\":\"mant.query/v1\",\"topic\":\"git\",\"section\":\"1\"}\n"
        );
        assert!(diagnostics.is_empty());
        assert_eq!(host.query_calls.get(), 1);
    }

    #[test]
    fn malformed_or_extended_requests_fail_before_querying_the_host() {
        for input in [
            br"not-json".as_slice(),
            br#"{"topic":"git","renderer":"html"}"#.as_slice(),
            br#"{"topic":"   "}"#.as_slice(),
        ] {
            let host = FakeHost::new();
            let (status, output, diagnostics) =
                invoke(&["--request-json", "--json", "--compact"], input, &host);
            assert_eq!(status, 2);
            assert!(output.is_empty());
            assert!(diagnostics.starts_with("mant-cli: "));
            assert_eq!(host.query_calls.get(), 0);
        }
    }

    #[test]
    fn update_and_protocol_results_are_stable_json_documents() {
        let host = FakeHost::new();
        let (status, output, diagnostics) = invoke(&["update", "tldr", "--compact"], b"", &host);
        assert_eq!(status, 0);
        assert_eq!(
            output,
            "{\"action\":\"updated\",\"cacheDir\":\"/cache/tldr\",\"revision\":\"abc123\"}\n"
        );
        assert!(diagnostics.is_empty());
        assert_eq!(host.update_calls.get(), 1);

        let (status, output, diagnostics) = invoke(&["protocol-version", "--compact"], b"", &host);
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("protocol JSON");
        assert_eq!(value["protocol"], CLI_PROTOCOL_VERSION);
        assert_eq!(value["nativeApiVersion"], "1");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn usage_errors_are_concise_and_never_trigger_side_effects() {
        let host = FakeHost::new();
        let (status, output, diagnostics) = invoke(&["--unknown"], b"", &host);
        assert_eq!(status, 2);
        assert!(output.is_empty());
        assert_eq!(
            diagnostics,
            "mant-cli: unknown option '--unknown'\nTry 'mant-cli --help' for more information.\n"
        );
        assert_eq!(host.query_calls.get(), 0);
        assert_eq!(host.update_calls.get(), 0);
    }
}
