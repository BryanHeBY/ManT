#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod arguments;
mod error;
mod mcp;
mod presentation;

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    io::{self, IsTerminal, Read, Write},
};

use arguments::{ColorMode, Command, QueryFormat, QueryPresentation, QuerySource, SchemaContract};
use error::{
    Failure, query_execution_failure, query_failure, report_argument_error, report_failure,
};
use mant_engine::QueryPolicy;
use mant_ir::ResolvedContent;
use mant_protocol::{
    CatalogQuery, DocumentAddress, DocumentCatalog, InputFormat, MarkdownOrigin, QueryInput,
    QueryRequest, QueryView, RequestSchema, TldrCacheUpdate,
};
use mant_sources::{DocumentSourcesPrune, DocumentSourcesUpdate};
use presentation::{render_json, render_query_result};
use serde::Serialize;

// ── Stable process protocol ────────────────────────────────────────────────

/// Exact stdio protocol exposed to external process clients.
pub const CLI_PROTOCOL_VERSION: &str = "mant.cli/v7";

const MAX_REQUEST_BYTES: u64 = 64 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolDescription<'a> {
    protocol: &'a str,
    native_api_version: &'a str,
    request_schema: &'a str,
    query_schema: &'a str,
    document_schema: &'a str,
    outline_schema: &'a str,
    excerpt_schema: &'a str,
    search_schema: &'a str,
    catalog_schema: &'a str,
}

/// Normalized fields of a conventional CLI document query.
#[allow(clippy::struct_excessive_bools)]
struct QueryExecution {
    source: QuerySource,
    presentation: QueryPresentation,
    pretty: bool,
    manual_only: bool,
    preserve_anchors: bool,
}

/// Terminal capabilities consulted only by the OS process entry point.
///
/// The injectable [`run`] boundary intentionally remains deterministic and
/// treats `Auto` as conventional Markdown output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerminalCapabilities {
    input: bool,
    output: bool,
    color: bool,
}

// ── Host boundary ─────────────────────────────────────────────────────────

trait CliHost {
    fn discover(&self, query: &CatalogQuery) -> Result<DocumentCatalog, Failure>;
    fn query(
        &self,
        request: &QueryRequest,
        policy: QueryPolicy,
    ) -> Result<ResolvedContent, Failure>;
    fn query_markdown(&self, source: &str) -> Result<ResolvedContent, Failure>;
    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure>;
    fn update_docs(&self) -> Result<DocumentSourcesUpdate, Failure>;
    fn prune_docs(&self, dry_run: bool) -> Result<DocumentSourcesPrune, Failure>;
}

struct SystemHost {
    resolver: mant_engine::DocumentResolver,
}

impl Default for SystemHost {
    fn default() -> Self {
        Self {
            resolver: mant_engine::DocumentResolver::from_system(),
        }
    }
}

impl CliHost for SystemHost {
    fn discover(&self, query: &CatalogQuery) -> Result<DocumentCatalog, Failure> {
        self.resolver.discover(query).map_err(Failure::operational)
    }

    fn query(
        &self,
        request: &QueryRequest,
        policy: QueryPolicy,
    ) -> Result<ResolvedContent, Failure> {
        self.resolver
            .resolve(request, policy)
            .map_err(query_failure)
    }

    fn query_markdown(&self, source: &str) -> Result<ResolvedContent, Failure> {
        mant_engine::query_markdown_text(source, None).map_err(Failure::operational)
    }

    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure> {
        mant_engine::update_tldr_cache().map_err(Failure::operational)
    }

    fn update_docs(&self) -> Result<DocumentSourcesUpdate, Failure> {
        mant_sources::update_document_sources().map_err(Failure::operational)
    }

    fn prune_docs(&self, dry_run: bool) -> Result<DocumentSourcesPrune, Failure> {
        mant_sources::prune_document_sources(dry_run).map_err(Failure::operational)
    }
}

// ── Process execution ─────────────────────────────────────────────────────

/// Run one CLI invocation using explicit streams and return its exit status.
///
/// Keeping the process streams injectable makes malformed protocol requests
/// testable without consulting host manual sources or a tldr client.
pub fn run(
    arguments: &[String],
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> u8 {
    run_with_host(
        arguments,
        input,
        output,
        diagnostics,
        &SystemHost::default(),
    )
}

/// Run one native-process invocation, including the long-lived MCP mode.
///
/// The conventional CLI keeps injectable streams through [`run`], while MCP
/// owns operating-system stdio because the protocol reserves it exclusively
/// for newline-delimited JSON-RPC messages.
pub async fn run_process(arguments: &[String]) -> u8 {
    let mut command = match arguments::parse(arguments) {
        Ok(command) => command,
        Err(error) => return report_argument_error(&error, &mut io::stderr().lock()),
    };

    if matches!(command, Command::Mcp) {
        return mcp::run_stdio().await;
    }
    let host = SystemHost::default();

    if let Err(error) = resolve_process_presentation(
        &mut command,
        TerminalCapabilities {
            input: io::stdin().is_terminal(),
            output: io::stdout().is_terminal(),
            color: std::env::var_os("NO_COLOR").is_none()
                && std::env::var("TERM").ok().as_deref() != Some("dumb"),
        },
    ) {
        return report_failure(&error, &mut io::stderr().lock());
    }

    if matches!(
        command,
        Command::Query {
            presentation: QueryPresentation::Interactive,
            ..
        }
    ) {
        return run_interactive(command, &mut io::stderr().lock(), &host);
    }

    run_command(
        command,
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
        &host,
    )
}

/// Resolve terminal-sensitive defaults without coupling argument parsing to
/// operating-system streams.
fn resolve_process_presentation(
    command: &mut Command,
    terminal: TerminalCapabilities,
) -> Result<(), Failure> {
    let Command::Query { presentation, .. } = command else {
        return Ok(());
    };
    match *presentation {
        QueryPresentation::Auto if terminal.input && terminal.output => {
            *presentation = QueryPresentation::Interactive;
        }
        QueryPresentation::Auto => {
            *presentation = QueryPresentation::Output(QueryFormat::Markdown);
        }
        QueryPresentation::Interactive if !terminal.input || !terminal.output => {
            return Err(Failure::usage(
                "interactive view requires an input and output terminal; omit --ui or select --format",
            ));
        }
        QueryPresentation::Tldr(ColorMode::Auto) => {
            *presentation = QueryPresentation::Tldr(if terminal.output && terminal.color {
                ColorMode::Always
            } else {
                ColorMode::Never
            });
        }
        QueryPresentation::Interactive
        | QueryPresentation::Output(_)
        | QueryPresentation::Tldr(ColorMode::Always | ColorMode::Never) => {}
    }
    Ok(())
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
        Err(error) => return report_argument_error(&error, diagnostics),
    };

    run_command(command, input, output, diagnostics, host)
}

fn run_command(
    command: Command,
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
) -> u8 {
    if matches!(command, Command::Mcp) {
        return report_failure(
            &Failure::usage("MCP mode must be launched through the native process entry point"),
            diagnostics,
        );
    }
    if matches!(
        command,
        Command::Query {
            presentation: QueryPresentation::Interactive,
            ..
        }
    ) {
        return report_failure(
            &Failure::usage("interactive mode requires the native terminal process boundary"),
            diagnostics,
        );
    }

    let (rendered, success_status) = match command {
        Command::UpdateDocs { pretty } => {
            let update = match host.update_docs() {
                Ok(update) => update,
                Err(error) => return report_failure(&error, diagnostics),
            };
            let status = u8::from(update.has_failures());
            let rendered = match render_json(&update, pretty) {
                Ok(rendered) => rendered,
                Err(error) => return report_failure(&error, diagnostics),
            };
            (rendered, status)
        }
        Command::PruneDocs { pretty, dry_run } => {
            let prune = match host.prune_docs(dry_run) {
                Ok(prune) => prune,
                Err(error) => return report_failure(&error, diagnostics),
            };
            let status = u8::from(prune.has_failures());
            let rendered = match render_json(&prune, pretty) {
                Ok(rendered) => rendered,
                Err(error) => return report_failure(&error, diagnostics),
            };
            (rendered, status)
        }
        command => match execute(command, input, host) {
            Ok(rendered) => (rendered, 0),
            Err(error) => return report_failure(&error, diagnostics),
        },
    };

    match write_output(output, &rendered) {
        Ok(()) => success_status,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => success_status,
        Err(error) => report_failure(&Failure::operational(error), diagnostics),
    }
}

fn execute(command: Command, input: &mut dyn Read, host: &dyn CliHost) -> Result<String, Failure> {
    match command {
        Command::Help(help) => Ok(help),
        Command::ProtocolVersion { pretty } => render_json(
            &ProtocolDescription {
                protocol: CLI_PROTOCOL_VERSION,
                native_api_version: mant_engine::native_api_version(),
                request_schema: "mant.request/v7",
                query_schema: "mant.query/v7",
                document_schema: "mant.document/v7",
                outline_schema: "mant.outline/v7",
                excerpt_schema: "mant.excerpt/v7",
                search_schema: "mant.search/v7",
                catalog_schema: "mant.catalog/v7",
            },
            pretty,
        ),
        Command::Schema { contract, pretty } => match contract {
            SchemaContract::Request => {
                render_json(&mant_protocol::query_request_json_schema(), pretty)
            }
            SchemaContract::Query => {
                render_json(&mant_protocol::query_bundle_json_schema(), pretty)
            }
            SchemaContract::Outline => {
                render_json(&mant_protocol::query_outline_json_schema(), pretty)
            }
            SchemaContract::Excerpt => {
                render_json(&mant_protocol::query_excerpt_json_schema(), pretty)
            }
            SchemaContract::Search => {
                render_json(&mant_protocol::query_search_json_schema(), pretty)
            }
            SchemaContract::Catalog => {
                render_json(&mant_protocol::document_catalog_json_schema(), pretty)
            }
            SchemaContract::All => render_json(&mant_protocol::query_json_schema_catalog(), pretty),
        },
        Command::Catalog {
            query,
            grouped,
            format,
            pretty,
        } => {
            let catalog = host.discover(&query)?;
            match format {
                QueryFormat::Json => render_json(&catalog, pretty),
                QueryFormat::Text => Ok(render_catalog_text(&catalog, grouped)),
                QueryFormat::Markdown | QueryFormat::Man => {
                    unreachable!("argument validation limits catalog formats")
                }
            }
        }
        Command::Mcp => unreachable!("MCP mode is dispatched before normal CLI execution"),
        Command::UpdateDocs { .. } => {
            unreachable!("document updates are dispatched before normal execution")
        }
        Command::PruneDocs { .. } => {
            unreachable!("document source pruning is dispatched before normal execution")
        }
        Command::UpdateTldr { pretty } => {
            let update = host.update_tldr()?;
            mant_engine::render_update_json(&update, pretty).map_err(Failure::operational)
        }
        Command::Query {
            source,
            presentation,
            pretty,
            manual_only,
            preserve_anchors,
        } => execute_query(
            QueryExecution {
                source,
                presentation,
                pretty,
                manual_only,
                preserve_anchors,
            },
            input,
            host,
        ),
    }
}

fn render_catalog_text(catalog: &DocumentCatalog, grouped: bool) -> String {
    if !grouped {
        let mut output = String::new();
        for document in &catalog.documents {
            let (_, kind) = catalog_category(&document.address);
            writeln!(output, "{}\t{kind}", document.catalog_path)
                .expect("writing to String cannot fail");
        }
        return output;
    }

    let mut categories = BTreeMap::<String, Vec<&str>>::new();
    for document in &catalog.documents {
        let (category, _) = catalog_category(&document.address);
        categories
            .entry(category)
            .or_default()
            .push(match &document.address {
                DocumentAddress::Markdown { path, .. } => path,
                DocumentAddress::Manual { name, .. } => name,
            });
    }
    let mut output = String::new();
    for (index, (category, names)) in categories.into_iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        output.push_str(&category);
        output.push('\n');
        for name in names {
            output.push_str("  ");
            output.push_str(name);
            output.push('\n');
        }
    }
    output
}

fn catalog_category(address: &DocumentAddress) -> (String, &'static str) {
    match address {
        DocumentAddress::Markdown {
            origin: MarkdownOrigin::Documents,
            ..
        } => ("documents".to_owned(), "markdown"),
        DocumentAddress::Markdown {
            origin: MarkdownOrigin::Source { name },
            ..
        } => (format!("sources/{name}"), "markdown"),
        DocumentAddress::Manual { manual_section, .. } => {
            (format!("manual/{manual_section}"), "manual")
        }
    }
}

/// Load one manual query and render the projection encoded in its request.
fn execute_query(
    command: QueryExecution,
    input: &mut dyn Read,
    host: &dyn CliHost,
) -> Result<String, Failure> {
    let policy = QueryPolicy {
        manual_only: command.manual_only,
        allow_tldr_only: matches!(command.presentation, QueryPresentation::Tldr(_)),
    };
    let result = match command.source {
        QuerySource::InputStdin { format, view } => {
            validate_markdown_policy(policy)?;
            let query = match format {
                InputFormat::Markdown => {
                    let source =
                        read_utf8_input(input, mant_engine::MAX_MARKDOWN_BYTES, "Markdown input")?;
                    host.query_markdown(&source)?
                }
                InputFormat::Roff => {
                    let source =
                        read_input_bytes(input, mant_engine::MAX_MANUAL_BYTES, "roff input")?;
                    mant_engine::query_roff_bytes(&source).map_err(query_failure)?
                }
                InputFormat::Auto => unreachable!("stdin input format is validated by clap"),
            };
            mant_engine::project_query_view(query, &view).map_err(query_execution_failure)?
        }
        source => {
            let request = read_query_request(source, input)?;
            mant_engine::validate_query_request(&request, policy).map_err(query_failure)?;
            let query = host.query(&request, policy)?;
            mant_engine::project_query_view(query, &request.view)
                .map_err(query_execution_failure)?
        }
    };
    if let QueryPresentation::Tldr(color) = command.presentation {
        let mant_engine::QueryViewResult::Excerpt(mant_protocol::QueryExcerpt {
            selections, ..
        }) = &result
        else {
            return Err(Failure::operational(
                "the tldr terminal presentation requires a tldr excerpt",
            ));
        };
        let document = selections.iter().find_map(|selection| match selection {
            mant_protocol::ExcerptSelection::Tldr { document, .. } => Some(document),
            _ => None,
        });
        return document.map_or_else(
            || Err(Failure::operational("no tldr quick reference is available")),
            |document| {
                Ok(mant_ui::render_tldr_terminal(
                    document,
                    color == ColorMode::Always,
                ))
            },
        );
    }
    let format = match command.presentation {
        QueryPresentation::Auto => QueryFormat::Markdown,
        QueryPresentation::Output(format) => format,
        QueryPresentation::Interactive => {
            return Err(Failure::usage(
                "interactive mode requires the native terminal process boundary",
            ));
        }
        QueryPresentation::Tldr(_) => unreachable!("tldr presentation returned above"),
    };
    render_query_result(&result, format, command.pretty, command.preserve_anchors)
}

/// Load one full query and hand the normalized document directly to Ratatui.
fn run_interactive(command: Command, diagnostics: &mut dyn Write, host: &dyn CliHost) -> u8 {
    let Command::Query {
        source,
        presentation: QueryPresentation::Interactive,
        manual_only,
        ..
    } = command
    else {
        return report_failure(
            &Failure::usage("interactive mode requires a document query"),
            diagnostics,
        );
    };
    let QuerySource::Arguments(request) = source else {
        return report_failure(
            &Failure::usage("interactive mode requires a document selector or explicit input"),
            diagnostics,
        );
    };
    if !matches!(request.view, QueryView::Full {}) {
        return report_failure(
            &Failure::usage("interactive mode requires the complete document view"),
            diagnostics,
        );
    }
    let policy = QueryPolicy {
        manual_only,
        allow_tldr_only: false,
    };
    if let Err(error) = mant_engine::validate_query_request(&request, policy).map_err(query_failure)
    {
        return report_failure(&error, diagnostics);
    }
    let query = match host.query(&request, policy) {
        Ok(query) => query,
        Err(error) => return report_failure(&error, diagnostics),
    };
    let catalog = match host.discover(&CatalogQuery::default()) {
        Ok(catalog) => catalog,
        Err(error) => return report_failure(&error, diagnostics),
    };
    match mant_ui::run_with_catalog(
        &query,
        catalog,
        |catalog_query| host.discover(catalog_query).map_err(Failure::into_message),
        |address| {
            let (request, policy) = request_for_address(address);
            host.query(&request, policy).map_err(Failure::into_message)
        },
        open_external_uri,
    ) {
        Ok(()) => 0,
        Err(error) => report_failure(&Failure::operational(error), diagnostics),
    }
}

fn open_external_uri(uri: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("rundll32.exe");
        command.args(["url.dll,FileProtocolHandler", uri]);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(uri);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(uri);
        command
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not open external link: {error}"))
}

fn request_for_address(address: &DocumentAddress) -> (QueryRequest, QueryPolicy) {
    let (name, source, manual_section, manual_only) = match address {
        DocumentAddress::Markdown { path, origin } => (
            path.clone(),
            match origin {
                MarkdownOrigin::Documents => None,
                MarkdownOrigin::Source { name } => Some(name.clone()),
            },
            None,
            false,
        ),
        DocumentAddress::Manual {
            name,
            manual_section,
        } => (name.clone(), None, Some(manual_section.clone()), true),
    };
    (
        QueryRequest {
            schema: RequestSchema::V7,
            input: QueryInput::Document {
                selector: name,
                source,
                manual_section,
            },
            view: QueryView::Full {},
        },
        QueryPolicy {
            manual_only,
            allow_tldr_only: false,
        },
    )
}

fn read_query_request(source: QuerySource, input: &mut dyn Read) -> Result<QueryRequest, Failure> {
    match source {
        QuerySource::Arguments(request) => return Ok(request),
        QuerySource::StdinJson => {}
        QuerySource::InputStdin { .. } => {
            unreachable!("direct stdin input is consumed before protocol request decoding");
        }
    }

    let request = read_utf8_input(input, MAX_REQUEST_BYTES, "request JSON")?;
    serde_json::from_str(&request)
        .map_err(|error| Failure::usage(format!("invalid query request JSON: {error}")))
}

fn read_utf8_input(input: &mut dyn Read, limit: u64, label: &str) -> Result<String, Failure> {
    let bytes = read_input_bytes(input, limit, label)?;
    String::from_utf8(bytes).map_err(|_| Failure::usage(format!("{label} must be UTF-8")))
}

fn read_input_bytes(input: &mut dyn Read, limit: u64, label: &str) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    input
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Failure::usage(format!("cannot read {label}: {error}")))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(Failure::usage(format!(
            "{label} exceeds the {limit}-byte limit"
        )));
    }
    Ok(bytes)
}

fn validate_markdown_policy(policy: QueryPolicy) -> Result<(), Failure> {
    if policy.manual_only {
        return Err(Failure::usage(
            "the manual-only policy does not apply to Markdown input",
        ));
    }
    Ok(())
}

fn write_output(output: &mut dyn Write, rendered: &str) -> io::Result<()> {
    output.write_all(rendered.as_bytes())?;
    if !rendered.ends_with('\n') {
        output.write_all(b"\n")?;
    }
    output.flush()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use mant_ir::ResolvedContent;

    use mant_sources::{
        DocumentSourcesPrune, DocumentSourcesPruneSchema, DocumentSourcesUpdate,
        DocumentSourcesUpdateSchema,
    };

    use mant_ir::{
        Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Document,
        DocumentMeta, DocumentSource, Inline, LayoutHint, Section, SourceFormat, TldrDocument,
        TldrOrigin,
    };
    use mant_protocol::{
        CatalogSchema, DocumentSummary, QueryInput, QueryRequest, TldrCacheAction, TldrCacheUpdate,
    };

    use super::{
        CLI_PROTOCOL_VERSION, CatalogQuery, CliHost, DocumentAddress, DocumentCatalog, Failure,
        MarkdownOrigin, QueryPolicy, TerminalCapabilities,
        arguments::{self, ColorMode, Command, QueryFormat, QueryPresentation},
        request_for_address, resolve_process_presentation, run_with_host,
    };

    struct FakeHost {
        query_calls: Cell<usize>,
        update_calls: Cell<usize>,
        last_policy: Cell<QueryPolicy>,
        document: Option<Document>,
        tldr: Option<TldrDocument>,
    }

    #[test]
    fn catalog_addresses_reopen_the_exact_source_or_manual_section() {
        let (request, policy) = request_for_address(&DocumentAddress::Markdown {
            path: "Start-Process".to_owned(),
            origin: MarkdownOrigin::Source {
                name: "pwsh7".to_owned(),
            },
        });
        assert_eq!(
            request.input,
            QueryInput::Document {
                selector: "Start-Process".to_owned(),
                source: Some("pwsh7".to_owned()),
                manual_section: None,
            }
        );
        assert!(!policy.manual_only);

        let (request, policy) = request_for_address(&DocumentAddress::Manual {
            name: "printf".to_owned(),
            manual_section: "3".to_owned(),
        });
        assert_eq!(
            request.input,
            QueryInput::Document {
                selector: "printf".to_owned(),
                source: None,
                manual_section: Some("3".to_owned()),
            }
        );
        assert!(policy.manual_only);
    }

    #[test]
    fn terminal_capabilities_resolve_only_automatic_full_queries() {
        let mut terminal_query = arguments::parse(&["git".to_owned()]).expect("automatic query");
        resolve_process_presentation(
            &mut terminal_query,
            TerminalCapabilities {
                input: true,
                output: true,
                color: true,
            },
        )
        .expect("terminal query");
        assert!(matches!(
            terminal_query,
            Command::Query {
                presentation: QueryPresentation::Interactive,
                ..
            }
        ));

        let mut redirected_query = arguments::parse(&["git".to_owned()]).expect("automatic query");
        resolve_process_presentation(
            &mut redirected_query,
            TerminalCapabilities {
                input: true,
                output: false,
                color: true,
            },
        )
        .expect("redirected query");
        assert!(matches!(
            redirected_query,
            Command::Query {
                presentation: QueryPresentation::Output(QueryFormat::Markdown),
                ..
            }
        ));

        let mut outline =
            arguments::parse(&["git".to_owned(), "--outline".to_owned()]).expect("outline query");
        resolve_process_presentation(
            &mut outline,
            TerminalCapabilities {
                input: true,
                output: true,
                color: true,
            },
        )
        .expect("outline remains non-interactive");
        assert!(matches!(
            outline,
            Command::Query {
                presentation: QueryPresentation::Output(QueryFormat::Text),
                ..
            }
        ));

        let mut tldr =
            arguments::parse(&["git".to_owned(), "--tldr".to_owned()]).expect("tldr query");
        resolve_process_presentation(
            &mut tldr,
            TerminalCapabilities {
                input: true,
                output: true,
                color: true,
            },
        )
        .expect("tldr remains non-interactive");
        assert!(matches!(
            tldr,
            Command::Query {
                presentation: QueryPresentation::Tldr(ColorMode::Always),
                ..
            }
        ));
    }

    #[test]
    fn explicit_interactive_queries_require_both_terminal_streams() {
        for terminal in [
            TerminalCapabilities {
                input: false,
                output: true,
                color: true,
            },
            TerminalCapabilities {
                input: true,
                output: false,
                color: true,
            },
        ] {
            let mut command =
                arguments::parse(&["git".to_owned(), "--ui".to_owned()]).expect("UI query");
            let error = resolve_process_presentation(&mut command, terminal)
                .expect_err("incomplete terminal must fail");
            assert!(error.message().contains("interactive view requires"));
        }
    }

    impl FakeHost {
        fn new() -> Self {
            Self {
                query_calls: Cell::new(0),
                update_calls: Cell::new(0),
                last_policy: Cell::new(QueryPolicy::default()),
                document: None,
                tldr: None,
            }
        }

        fn with_manual() -> Self {
            Self {
                document: Some(manual()),
                ..Self::new()
            }
        }

        fn with_manual_and_tldr() -> Self {
            Self {
                document: Some(manual()),
                tldr: Some(tldr()),
                ..Self::new()
            }
        }

        fn with_tldr() -> Self {
            Self {
                tldr: Some(tldr()),
                ..Self::new()
            }
        }

        fn with_explainable_manual() -> Self {
            Self {
                document: Some(explainable_manual()),
                ..Self::new()
            }
        }

        fn with_semantic_markdown() -> Self {
            Self {
                document: Some(semantic_markdown()),
                ..Self::new()
            }
        }
    }

    impl CliHost for FakeHost {
        fn discover(&self, _query: &CatalogQuery) -> Result<DocumentCatalog, Failure> {
            Ok(DocumentCatalog {
                schema: CatalogSchema::V7,
                total: 2,
                returned: 2,
                offset: 0,
                truncated: false,
                next_offset: None,
                documents: vec![
                    DocumentSummary {
                        address: DocumentAddress::Markdown {
                            path: "guide".to_owned(),
                            origin: MarkdownOrigin::Source {
                                name: "team".to_owned(),
                            },
                        },
                        catalog_path: "sources/team/guide".to_owned(),
                    },
                    DocumentSummary {
                        address: DocumentAddress::Manual {
                            name: "printf".to_owned(),
                            manual_section: "3".to_owned(),
                        },
                        catalog_path: "manual/3/printf".to_owned(),
                    },
                ],
            })
        }

        fn query(
            &self,
            request: &QueryRequest,
            policy: QueryPolicy,
        ) -> Result<ResolvedContent, Failure> {
            self.query_calls.set(self.query_calls.get() + 1);
            self.last_policy.set(policy);
            let label = match &request.input {
                QueryInput::Document { selector, .. } => selector.trim().to_owned(),
                QueryInput::File { path, .. } => path.clone(),
            };
            Ok(ResolvedContent {
                address: None,
                label,
                document: self.document.clone(),
                tldr: self.tldr.clone(),
            })
        }

        fn query_markdown(&self, _source: &str) -> Result<ResolvedContent, Failure> {
            self.query_calls.set(self.query_calls.get() + 1);
            Ok(ResolvedContent {
                address: None,
                label: "stdin".to_owned(),
                document: self.document.clone(),
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

        fn update_docs(&self) -> Result<DocumentSourcesUpdate, Failure> {
            Ok(DocumentSourcesUpdate {
                schema: DocumentSourcesUpdateSchema::V2,
                config: "/data/mant/sources.toml".to_owned(),
                sources: Vec::new(),
                orphaned: Vec::new(),
            })
        }

        fn prune_docs(&self, dry_run: bool) -> Result<DocumentSourcesPrune, Failure> {
            Ok(DocumentSourcesPrune {
                schema: DocumentSourcesPruneSchema::V1,
                config: "/data/mant/sources.toml".to_owned(),
                dry_run,
                sources: Vec::new(),
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

    fn manual() -> Document {
        Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Man,
                path: Some("/man/demo.1".to_owned()),
            },
            meta: DocumentMeta {
                manual_section: Some("1".to_owned()),
                ..DocumentMeta::default()
            },
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![
                section("name-1", "NAME", "demo - a test", Vec::new()),
                section(
                    "options-2",
                    "OPTIONS",
                    "all options",
                    vec![section(
                        "common-3",
                        "Common options",
                        "common details",
                        Vec::new(),
                    )],
                ),
            ],
        }
    }

    fn explainable_manual() -> Document {
        let mut manual = manual();
        let options = manual
            .sections
            .iter_mut()
            .find(|section| section.id == "options-2")
            .expect("options section");
        options.blocks.push(Block::DefinitionList {
            items: vec![DefinitionItem {
                inline_term: false,
                identity: Some(DefinitionIdentity {
                    id: "exclude".to_owned().into(),
                    role: DefinitionRole::Option,
                    case: DefinitionCase::Sensitive,
                    names: vec!["--exclude".to_owned()],
                }),
                terms: vec![vec![Inline::Text {
                    value: "--exclude=PATTERN".to_owned(),
                }]],
                description: vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: "Exclude matching files from the archive.".to_owned(),
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                }],
                spacing_before_lines: None,
            }],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        });
        manual
    }

    fn semantic_markdown() -> Document {
        mant_engine::parse_markdown(
            "# Tool\n\n## Query\n\nGeneral query behavior.\n\n<!-- mant:entries role=option case=insensitive -->\n- `/f`: Force a query.\n\n## Commands\n\n<!-- mant:entries role=command case=insensitive -->\n- `query`: Query registry data.\n\n## Options\n\n<!-- mant:entries role=option case=insensitive -->\n- `/S COMPUTER`: Select a remote computer.\n\n## Environment\n\n<!-- mant:entries role=environment-variable case=insensitive -->\n- `PATH`, `$env:PATH`: Control executable discovery.\n\n## Delete\n\n<!-- mant:entries role=option case=insensitive -->\n- `/F`: Force deletion.\n",
            Some("semantic.md".to_owned()),
        )
        .expect("semantic Markdown fixture")
        .document
    }

    fn tldr() -> TldrDocument {
        TldrDocument {
            title: "demo".to_owned(),
            description: vec!["A small demonstration.".to_owned()],
            more_information: None,
            examples: Vec::new(),
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "/cache/tldr/pages/common/demo.md".to_owned(),
            origin: TldrOrigin::TldrPages,
        }
    }

    fn section(id: &str, title: &str, text: &str, children: Vec<Section>) -> Section {
        Section {
            id: id.to_owned().into(),
            title: title.to_owned(),
            spacing_before_lines: 0,
            blocks: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: text.to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            children,
            source: None,
        }
    }

    #[test]
    fn stdin_protocol_emits_only_compact_query_json() {
        let host = FakeHost::new();
        let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"git","manualSection":"1"},"view":{"kind":"full"}}"#,
            &host,
        );

        assert_eq!(status, 0);
        assert_eq!(output, "{\"schema\":\"mant.query/v7\",\"label\":\"git\"}\n");
        assert!(diagnostics.is_empty());
        assert_eq!(host.query_calls.get(), 1);
    }

    #[test]
    fn malformed_or_extended_requests_fail_before_querying_the_host() {
        for input in [
            br"not-json".as_slice(),
            br#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"git"},"view":{"kind":"full"},"futureField":true}"#.as_slice(),
            br#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"   "},"view":{"kind":"full"}}"#.as_slice(),
            br#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"git"},"view":{"kind":"excerpt","nodes":[]}}"#.as_slice(),
            br#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"git"},"view":{"kind":"search","pattern":"","limit":10}}"#.as_slice(),
            br#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"git"},"view":{"kind":"search","pattern":"git","limit":0}}"#.as_slice(),
            br#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"git"},"view":{"kind":"search","pattern":"git","contextLines":101}}"#.as_slice(),
            br#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"git"},"view":{"kind":"search","pattern":"[","syntax":"regex"}}"#.as_slice(),
        ] {
            let host = FakeHost::new();
            let (status, output, diagnostics) = invoke(
                &["--request-json", "--format", "json", "--compact"],
                input,
                &host,
            );
            assert_eq!(status, 2);
            assert!(output.is_empty());
            assert!(diagnostics.starts_with("mant: "));
            assert_eq!(host.query_calls.get(), 0);
        }
    }

    #[test]
    fn stdin_requests_select_outline_and_excerpt_projections() {
        let host = FakeHost::with_manual_and_tldr();
        let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"demo"},"view":{"kind":"outline","detail":"sections"}}"#,
            &host,
        );
        assert_eq!(status, 0);
        let outline: serde_json::Value = serde_json::from_str(&output).expect("outline JSON");
        assert_eq!(outline["schema"], "mant.outline/v7");
        assert_eq!(outline["detail"], "sections");
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"demo"},"view":{"kind":"excerpt","selectors":["2.1"]}}"#,
            &host,
        );
        assert_eq!(status, 0);
        let excerpt: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
        assert_eq!(excerpt["schema"], "mant.excerpt/v7");
        assert_eq!(excerpt["selections"][0]["path"], "2.1");
        assert!(diagnostics.is_empty());
        assert_eq!(host.query_calls.get(), 2);
    }

    #[test]
    fn direct_queries_render_outlines_and_selected_nodes_in_requested_formats() {
        let host = FakeHost::with_manual_and_tldr();
        let (status, output, diagnostics) = invoke(&["demo", "--outline"], b"", &host);
        assert_eq!(status, 0);
        assert!(output.contains("├─ 0 [tldr] TLDR QUICK REFERENCE"));
        assert!(output.contains("├─ 1 [name-1] NAME"));
        assert!(output.contains("└─ 2 [options-2] OPTIONS"));
        assert!(output.contains("└─ 2.1 [common-3] Common options"));
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &["demo", "--node", "2.1", "--format", "json", "--compact"],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
        assert_eq!(value["schema"], "mant.excerpt/v7");
        assert_eq!(value["selections"][0]["path"], "2.1");
        assert_eq!(value["selections"][0]["section"]["title"], "Common options");
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &["demo", "--node", "0", "--format", "json", "--compact"],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("tldr excerpt JSON");
        assert_eq!(value["selections"][0]["kind"], "tldr");
        assert_eq!(value["selections"][0]["path"], "0");
        assert_eq!(value["selections"][0]["document"]["title"], "demo");
        assert!(value.get("producer").is_none());
        assert!(value.get("diagnostics").is_none());
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(&["demo", "--tldr"], b"", &host);
        assert_eq!(status, 0);
        assert!(output.contains("A small demonstration."));
        assert!(!output.contains("## NAME"));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn markdown_is_clean_by_default_and_preserves_anchors_on_request() {
        let host = FakeHost::with_manual();
        let (status, output, diagnostics) = invoke(&["demo"], b"", &host);
        assert_eq!(status, 0);
        assert!(!output.contains("<a "));
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(&["demo", "--preserve-anchors"], b"", &host);
        assert_eq!(status, 0);
        assert!(output.contains("<a id=\"name-1\"></a>"));
        assert!(output.contains("<a id=\"options-2\"></a>"));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn man_format_rejects_a_tldr_only_result() {
        let host = FakeHost::with_tldr();
        let (status, output, diagnostics) = invoke(&["demo", "--format", "man"], b"", &host);

        assert_eq!(status, 1);
        assert!(output.is_empty());
        assert_eq!(
            diagnostics,
            "mant: manual page is unavailable; --format man cannot render tldr-only content\n"
        );
    }

    #[test]
    fn explains_one_semantic_entry_through_the_excerpt_response() {
        let host = FakeHost::with_explainable_manual();
        let (status, output, diagnostics) = invoke(&["demo", "--explain", "--exclude"], b"", &host);

        assert_eq!(status, 0);
        assert!(output.contains("Outline `2/e1`: OPTIONS → --exclude"));
        assert!(output.contains("--exclude=PATTERN"));
        assert!(output.contains("Exclude matching files from the archive."));
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &[
                "demo",
                "--explain=--exclude",
                "--format",
                "json",
                "--compact",
            ],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
        assert_eq!(value["schema"], "mant.excerpt/v7");
        assert_eq!(value["selections"][0]["kind"], "document-entry");
        assert_eq!(value["selections"][0]["id"], "exclude");
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(&["demo", "--explain=2"], b"", &host);
        assert_eq!(status, 2);
        assert!(output.is_empty());
        assert!(diagnostics.contains("is not a semantic entry; use --node for sections"));
    }

    #[test]
    fn semantic_entries_work_through_cli_and_request_json() {
        let host = FakeHost::with_semantic_markdown();
        let (status, output, diagnostics) = invoke(
            &["demo", "--outline=entries", "--format", "json", "--compact"],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let outline: serde_json::Value = serde_json::from_str(&output).expect("outline JSON");
        assert_eq!(outline["schema"], "mant.outline/v7");
        let encoded = outline.to_string();
        for role in ["option", "command", "environment-variable"] {
            assert!(encoded.contains(&format!("\"role\":\"{role}\"")));
        }
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &["demo", "--outline=options", "--format", "json", "--compact"],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let outline: serde_json::Value = serde_json::from_str(&output).expect("alias outline");
        assert_eq!(outline["detail"], "entries");
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &["demo", "--explain=query", "--format", "json", "--compact"],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let excerpt: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
        assert_eq!(excerpt["selections"][0]["kind"], "document-entry");
        assert_eq!(
            excerpt["selections"][0]["entry"]["identity"]["role"],
            "command"
        );
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &["demo", "--node=query", "--format", "json", "--compact"],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let excerpt: serde_json::Value = serde_json::from_str(&output).expect("section excerpt");
        assert_eq!(excerpt["selections"][0]["kind"], "document-section");
        assert_eq!(excerpt["selections"][0]["id"], "query");
        assert!(diagnostics.is_empty());

        for (selector, role) in [("/s", "option"), ("$ENV:PATH", "environment-variable")] {
            let argument = format!("--explain={selector}");
            let (status, output, diagnostics) = invoke(
                &["demo", &argument, "--format", "json", "--compact"],
                b"",
                &host,
            );
            assert_eq!(status, 0);
            let excerpt: serde_json::Value =
                serde_json::from_str(&output).expect("role explanation");
            assert_eq!(excerpt["selections"][0]["entry"]["identity"]["role"], role);
            assert!(diagnostics.is_empty());
        }

        let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"demo"},"view":{"kind":"explain","entry":"query"}}"#,
            &host,
        );
        assert_eq!(status, 0);
        let excerpt: serde_json::Value = serde_json::from_str(&output).expect("request excerpt");
        assert_eq!(
            excerpt["selections"][0]["entry"]["identity"]["role"],
            "command"
        );
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(&["demo", "--explain=/f"], b"", &host);
        assert_eq!(status, 2);
        assert!(output.is_empty());
        assert!(diagnostics.contains("multiple semantic entries"));
        assert!(diagnostics.contains("option-f"));
        assert!(diagnostics.contains("option-f-2"));

        let (status, output, diagnostics) = invoke(
            &[
                "demo",
                "--explain=option-f-2",
                "--format",
                "json",
                "--compact",
            ],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let excerpt: serde_json::Value = serde_json::from_str(&output).expect("qualified entry");
        assert_eq!(
            excerpt["selections"][0]["entry"]["identity"]["id"],
            "option-f-2"
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn manual_option_reaches_the_resolution_policy_without_stderr_noise() {
        let host = FakeHost::with_manual();
        let (status, output, diagnostics) = invoke(&["demo", "--outline", "--manual"], b"", &host);

        assert_eq!(status, 0);
        assert!(output.contains("[name-1] NAME"));
        assert!(diagnostics.is_empty());
        assert_eq!(
            host.last_policy.get(),
            QueryPolicy {
                manual_only: true,
                allow_tldr_only: false,
            }
        );
    }

    #[test]
    fn searches_report_markdown_coordinates_and_reusable_outline_nodes() {
        let host = FakeHost::with_manual_and_tldr();
        let (status, output, diagnostics) = invoke(
            &[
                "demo",
                "--search",
                "common details",
                "--format",
                "json",
                "--compact",
            ],
            b"",
            &host,
        );

        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("search JSON");
        assert_eq!(value["schema"], "mant.search/v7");
        assert_eq!(value["total"], 1);
        assert_eq!(value["matches"][0]["node"]["path"], "2.1");
        assert_eq!(value["matches"][0]["section"]["id"], "common-3");
        assert!(value["matches"][0]["markdown"]["startLine"].as_u64() > Some(1));
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(&["demo", "--grep", "missing"], b"", &host);
        assert_eq!(status, 0);
        assert_eq!(output, "No matches for \"missing\" in demo(1).\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn stdin_search_requests_use_the_same_projection_contract() {
        let host = FakeHost::with_manual();
        let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v7","input":{"kind":"document","selector":"demo"},"view":{"kind":"search","pattern":"options","limit":10}}"#,
            &host,
        );

        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("search JSON");
        assert_eq!(value["schema"], "mant.search/v7");
        assert_eq!(value["query"]["syntax"], "literal");
        assert_eq!(value["query"]["scope"], "visible");
        assert!(
            value["matches"]
                .as_array()
                .is_some_and(|matches| !matches.is_empty())
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn unknown_nodes_are_concise_usage_failures() {
        let host = FakeHost::with_manual();
        let (status, output, diagnostics) =
            invoke(&["demo", "--node", "9", "--format", "text"], b"", &host);

        assert_eq!(status, 2);
        assert!(output.is_empty());
        assert!(diagnostics.contains("document 'demo' has no outline node '9'"));
        assert!(diagnostics.contains("inspect its entries outline as JSON"));
    }

    #[test]
    fn update_and_protocol_results_are_stable_json_documents() {
        let host = FakeHost::new();
        let (status, output, diagnostics) = invoke(&["--update-tldr", "--compact"], b"", &host);
        assert_eq!(status, 0);
        assert_eq!(
            output,
            "{\"action\":\"updated\",\"cacheDir\":\"/cache/tldr\",\"revision\":\"abc123\"}\n"
        );
        assert!(diagnostics.is_empty());
        assert_eq!(host.update_calls.get(), 1);

        let (status, output, diagnostics) =
            invoke(&["--prune-docs", "--dry-run", "--compact"], b"", &host);
        assert_eq!(status, 0);
        assert_eq!(
            output,
            "{\"schema\":\"mant.sources-prune/v1\",\"config\":\"/data/mant/sources.toml\",\"dryRun\":true,\"sources\":[]}\n"
        );
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) =
            invoke(&["--protocol-version", "--compact"], b"", &host);
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("protocol JSON");
        assert_eq!(value["protocol"], CLI_PROTOCOL_VERSION);
        assert_eq!(value["nativeApiVersion"], "7");
        assert_eq!(value["requestSchema"], "mant.request/v7");
        assert_eq!(value["outlineSchema"], "mant.outline/v7");
        assert_eq!(value["excerptSchema"], "mant.excerpt/v7");
        assert_eq!(value["searchSchema"], "mant.search/v7");
        assert_eq!(value["catalogSchema"], "mant.catalog/v7");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn catalog_lists_grouped_documents_and_emits_flat_find_records() {
        let host = FakeHost::new();
        let (status, output, diagnostics) = invoke(&["--list"], b"", &host);
        assert_eq!(status, 0);
        assert_eq!(output, "manual/3\n  printf\n\nsources/team\n  guide\n");
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(&["--find", "guide"], b"", &host);
        assert_eq!(status, 0);
        assert_eq!(
            output,
            "sources/team/guide\tmarkdown\nmanual/3/printf\tmanual\n"
        );
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &["--find", "guide", "--format", "json", "--compact"],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("catalog JSON");
        assert_eq!(value["schema"], "mant.catalog/v7");
        assert_eq!(value["documents"][0]["address"]["path"], "guide");
        assert!(value["documents"][0].get("sourcePath").is_none());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn usage_errors_are_concise_and_never_trigger_side_effects() {
        let host = FakeHost::new();
        let (status, output, diagnostics) = invoke(&["--unknown"], b"", &host);
        assert_eq!(status, 2);
        assert!(output.is_empty());
        assert!(diagnostics.starts_with("error: unexpected argument '--unknown'"));
        assert!(diagnostics.contains("Usage: mant"));
        assert!(diagnostics.contains("For more information, try '--help'."));
        assert_eq!(host.query_calls.get(), 0);
        assert_eq!(host.update_calls.get(), 0);
    }

    #[test]
    fn generated_schemas_are_json_only_and_side_effect_free() {
        let host = FakeHost::new();
        let (status, output, diagnostics) =
            invoke(&["--schema", "request", "--compact"], b"", &host);

        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("request schema");
        assert_eq!(
            value["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(value["additionalProperties"], false);
        assert!(output.contains("mant.request/v7"));
        assert!(diagnostics.is_empty());
        assert_eq!(host.query_calls.get(), 0);
        assert_eq!(host.update_calls.get(), 0);

        let (status, output, diagnostics) = invoke(&["--schema", "all"], b"", &host);
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("schema catalog");
        assert!(value["request"].is_object());
        assert!(value["query"].is_object());
        assert!(value["outline"].is_object());
        assert!(value["excerpt"].is_object());
        assert!(value["search"].is_object());
        assert!(diagnostics.is_empty());
        assert_eq!(host.query_calls.get(), 0);
        assert_eq!(host.update_calls.get(), 0);
    }
}
