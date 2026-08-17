#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod arguments;
mod doctor;
mod error;
mod mcp;
mod presentation;
mod terminal;

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    io::{self, IsTerminal, Read, Write},
};

use arguments::{
    CatalogPaging, ColorMode, Command, QueryFormat, QueryPresentation, QuerySource, SchemaContract,
};
use error::{
    Failure, query_execution_failure, query_failure, report_argument_error, report_failure,
    report_process_argument_error,
};
use mant_engine::QueryPolicy;
use mant_ir::ResolvedContent;
use mant_protocol::{
    CatalogQuery, DoctorReport, DocumentAddress, DocumentCatalog, InputFormat, MarkdownOrigin,
    QueryInput, QueryRequest, QueryView, RequestSchema, TldrCacheUpdate,
};
use mant_sources::{DocumentSourcesPrune, DocumentSourcesUpdate};
use presentation::{render_json, render_query_result};
use serde::Serialize;

// ── Stable process protocol ────────────────────────────────────────────────

/// Exact stdio protocol exposed to external process clients.
pub const CLI_PROTOCOL_VERSION: &str = "mant.cli/v0.8";

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
struct QueryExecution {
    source: QuerySource,
    presentation: QueryPresentation,
    pretty: bool,
    policy: QueryPolicy,
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
    kind: TerminalKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalKind {
    Capable,
    Dumb,
}

// ── Host boundary ─────────────────────────────────────────────────────────

trait CliHost {
    fn doctor(&self) -> Result<DoctorReport, Failure>;
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
    fn doctor(&self) -> Result<DoctorReport, Failure> {
        Ok(doctor::inspect_system())
    }

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
    let requested_color = arguments::requested_color(arguments);
    let mut command = match arguments::parse_process(arguments) {
        Ok(command) => command,
        Err(error) => return report_process_argument_error(&error),
    };

    if matches!(command, Command::Mcp) {
        return mcp::run_stdio().await;
    }
    let host = SystemHost::default();
    let mut diagnostics = anstream::AutoStream::new(io::stderr(), requested_color.into()).lock();
    let output_terminal = io::stdout().is_terminal();
    let input_terminal = io::stdin().is_terminal();
    let terminal_kind = if std::env::var("TERM").ok().as_deref() == Some("dumb") {
        TerminalKind::Dumb
    } else {
        TerminalKind::Capable
    };
    let output_ansi_supported = terminal::prepare_ansi_output(output_terminal);
    let terminal = TerminalCapabilities {
        input: input_terminal,
        output: output_terminal,
        color: terminal::color_enabled(requested_color, output_terminal, output_ansi_supported),
        kind: terminal_kind,
    };

    if let Err(error) = resolve_process_presentation(&mut command, terminal) {
        return report_failure(&error, &mut diagnostics, true);
    }

    if matches!(
        command,
        Command::Query {
            presentation: QueryPresentation::Interactive,
            ..
        }
    ) {
        return run_interactive(command, &mut diagnostics, &host, true);
    }
    if should_page_catalog(&command, terminal) {
        return run_paged_catalog(command, &mut diagnostics, &host, true);
    }

    run_command(
        command,
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        &mut diagnostics,
        &host,
        true,
    )
}

fn should_page_catalog(command: &Command, terminal: TerminalCapabilities) -> bool {
    terminal.input
        && terminal.output
        && terminal.kind == TerminalKind::Capable
        && matches!(
            command,
            Command::Catalog {
                format: QueryFormat::Text,
                paging: CatalogPaging::Auto,
                ..
            }
        )
}

fn run_paged_catalog(
    command: Command,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
    diagnostics_color: bool,
) -> u8 {
    let prompt = match &command {
        Command::Catalog { grouped: true, .. } => "mant --list",
        Command::Catalog { grouped: false, .. } => "mant --find",
        _ => unreachable!("pager accepts only catalog commands"),
    };
    let rendered = match execute(command, &mut io::empty(), host) {
        Ok(rendered) => rendered,
        Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
    };
    match mant_ui::page_text(rendered, prompt) {
        Ok(()) => 0,
        Err(error) => report_failure(&Failure::operational(error), diagnostics, diagnostics_color),
    }
}

/// Resolve terminal-sensitive defaults without coupling argument parsing to
/// operating-system streams.
fn resolve_process_presentation(
    command: &mut Command,
    terminal: TerminalCapabilities,
) -> Result<(), Failure> {
    if let Command::Doctor { color, .. } = command {
        if *color == ColorMode::Auto {
            *color = if terminal.output && terminal.color {
                ColorMode::Always
            } else {
                ColorMode::Never
            };
        }
        return Ok(());
    }
    let Command::Query { presentation, .. } = command else {
        return Ok(());
    };
    match *presentation {
        QueryPresentation::Auto if terminal.input && terminal.output => {
            *presentation = QueryPresentation::Interactive;
        }
        QueryPresentation::Auto => {
            *presentation = QueryPresentation::Output {
                format: QueryFormat::Markdown,
                color: ColorMode::Never,
            };
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
        QueryPresentation::Output {
            format,
            color: ColorMode::Auto,
        } => {
            *presentation = QueryPresentation::Output {
                format,
                color: if terminal.output && terminal.color {
                    ColorMode::Always
                } else {
                    ColorMode::Never
                },
            };
        }
        QueryPresentation::Interactive
        | QueryPresentation::Output {
            color: ColorMode::Always | ColorMode::Never,
            ..
        }
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
    let diagnostics_color = arguments::requested_color(arguments) == ColorMode::Always;
    let command = match arguments::parse(arguments) {
        Ok(command) => command,
        Err(error) => return report_argument_error(&error, diagnostics),
    };

    run_command(command, input, output, diagnostics, host, diagnostics_color)
}

fn run_command(
    command: Command,
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
    diagnostics_color: bool,
) -> u8 {
    if matches!(command, Command::Mcp) {
        return report_failure(
            &Failure::usage("MCP mode must be launched through the native process entry point"),
            diagnostics,
            diagnostics_color,
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
            diagnostics_color,
        );
    }

    let (rendered, success_status) = match command {
        Command::UpdateDocs { pretty } => {
            let update = match host.update_docs() {
                Ok(update) => update,
                Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
            };
            let status = u8::from(update.has_failures());
            let rendered = match render_json(&update, pretty) {
                Ok(rendered) => rendered,
                Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
            };
            (rendered, status)
        }
        Command::PruneDocs { pretty, dry_run } => {
            let prune = match host.prune_docs(dry_run) {
                Ok(prune) => prune,
                Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
            };
            let status = u8::from(prune.has_failures());
            let rendered = match render_json(&prune, pretty) {
                Ok(rendered) => rendered,
                Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
            };
            (rendered, status)
        }
        Command::Doctor {
            format,
            pretty,
            color,
        } => {
            let report = match host.doctor() {
                Ok(report) => report,
                Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
            };
            let status = u8::from(report.has_errors());
            let rendered = match format {
                QueryFormat::Text => doctor::render_text(&report, color == ColorMode::Always),
                QueryFormat::Json => match render_json(&report, pretty) {
                    Ok(rendered) => rendered,
                    Err(error) => {
                        return report_failure(&error, diagnostics, diagnostics_color);
                    }
                },
                QueryFormat::Markdown | QueryFormat::Man => {
                    unreachable!("argument validation limits doctor formats")
                }
            };
            (rendered, status)
        }
        command => match execute(command, input, host) {
            Ok(rendered) => (rendered, 0),
            Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
        },
    };

    match write_output(output, &rendered) {
        Ok(()) => success_status,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => success_status,
        Err(error) => report_failure(&Failure::operational(error), diagnostics, diagnostics_color),
    }
}

fn execute(command: Command, input: &mut dyn Read, host: &dyn CliHost) -> Result<String, Failure> {
    match command {
        Command::Help(help) => Ok(help),
        Command::ProtocolVersion { pretty } => render_json(
            &ProtocolDescription {
                protocol: CLI_PROTOCOL_VERSION,
                native_api_version: mant_engine::native_api_version(),
                request_schema: "mant.request/v0.8",
                query_schema: "mant.query/v0.8",
                document_schema: "mant.document/v0.8",
                outline_schema: "mant.outline/v0.8",
                excerpt_schema: "mant.excerpt/v0.8",
                search_schema: "mant.search/v0.8",
                catalog_schema: "mant.catalog/v0.8",
            },
            pretty,
        ),
        Command::Schema { contract, pretty } => match contract {
            SchemaContract::Doctor => {
                render_json(&mant_protocol::doctor_report_json_schema(), pretty)
            }
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
            ..
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
        Command::Doctor { .. } => {
            unreachable!("doctor is dispatched before normal execution")
        }
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
            policy,
            preserve_anchors,
        } => execute_query(
            QueryExecution {
                source,
                presentation,
                pretty,
                policy,
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
    let policy = command.policy;
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
    let (format, color) = match command.presentation {
        QueryPresentation::Auto => (QueryFormat::Markdown, ColorMode::Never),
        QueryPresentation::Output { format, color } => (format, color),
        QueryPresentation::Interactive => {
            return Err(Failure::usage(
                "interactive mode requires the native terminal process boundary",
            ));
        }
        QueryPresentation::Tldr(_) => unreachable!("tldr presentation returned above"),
    };
    render_query_result(
        &result,
        format,
        command.pretty,
        command.preserve_anchors,
        color == ColorMode::Always,
    )
}

/// Load one full query and hand the normalized document directly to Ratatui.
fn run_interactive(
    command: Command,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
    diagnostics_color: bool,
) -> u8 {
    let Command::Query {
        source,
        presentation: QueryPresentation::Interactive,
        policy,
        ..
    } = command
    else {
        return report_failure(
            &Failure::usage("interactive mode requires a document query"),
            diagnostics,
            diagnostics_color,
        );
    };
    let QuerySource::Arguments(request) = source else {
        return report_failure(
            &Failure::usage("interactive mode requires a document selector or explicit input"),
            diagnostics,
            diagnostics_color,
        );
    };
    if !matches!(request.view, QueryView::Full {}) {
        return report_failure(
            &Failure::usage("interactive mode requires the complete document view"),
            diagnostics,
            diagnostics_color,
        );
    }
    if let Err(error) = mant_engine::validate_query_request(&request, policy).map_err(query_failure)
    {
        return report_failure(&error, diagnostics, diagnostics_color);
    }
    let query = match host.query(&request, policy) {
        Ok(query) => query,
        Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
    };
    let catalog = match host.discover(&CatalogQuery::default()) {
        Ok(catalog) => catalog,
        Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
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
        Err(error) => report_failure(&Failure::operational(error), diagnostics, diagnostics_color),
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
    let (name, source, manual_section, policy) = match address {
        DocumentAddress::Markdown { path, origin } => (
            path.clone(),
            match origin {
                MarkdownOrigin::Documents => None,
                MarkdownOrigin::Source { name } => Some(name.clone()),
            },
            None,
            QueryPolicy::Combined,
        ),
        DocumentAddress::Manual {
            name,
            manual_section,
        } => (
            name.clone(),
            None,
            Some(manual_section.clone()),
            QueryPolicy::Combined,
        ),
    };
    (
        QueryRequest {
            schema: RequestSchema::V0Dot8,
            input: QueryInput::Document {
                selector: name,
                source,
                manual_section,
            },
            view: QueryView::Full {},
        },
        policy,
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
    if policy != QueryPolicy::Combined {
        return Err(Failure::usage(
            "content-only policies do not apply to Markdown input",
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
mod tests;
