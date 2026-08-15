//! Defines and validates the public `mant` command line with clap.
//!
//! The interface intentionally has one positional value: the document name.
//! Every action, projection, input mode, and output choice is a long option so
//! humans and agents do not have to distinguish ad-hoc subcommand grammars.

use std::iter;

use clap::{
    ArgAction, ArgGroup, CommandFactory, FromArgMatches, ValueEnum,
    builder::styling::{AnsiColor, Styles},
    error::ErrorKind,
};
use mant_engine::{
    QueryPolicy, is_manual_section, normalize_tldr_topic, parenthesized_manual_reference,
};
use mant_protocol::{
    CatalogDocumentKind, CatalogQuery, InputFormat, NodeSelector, OutlineDetail, QueryInput,
    QueryRequest, QueryView, RequestSchema, SearchCase, SearchScope, SearchSyntax,
    default_search_limit,
};

// ── Public command model ───────────────────────────────────────────────────

/// The output selected for one manual query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum QueryFormat {
    Markdown,
    Text,
    // `man(1)`-faithful plain text of the full page (no tldr, no page noise).
    Man,
    Json,
}

/// Source family selected by document-catalog commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CatalogKindMode {
    Markdown,
    Manual,
}

impl From<CatalogKindMode> for CatalogDocumentKind {
    fn from(value: CatalogKindMode) -> Self {
        match value {
            CatalogKindMode::Markdown => Self::Markdown,
            CatalogKindMode::Manual => Self::Manual,
        }
    }
}

/// How a complete native query is presented to its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryPresentation {
    /// Use the interactive reader when the process owns a terminal, otherwise
    /// retain the conventional Markdown output.
    Auto,
    /// Require the Ratatui reader and a usable terminal.
    Interactive,
    /// Render a deterministic representation to standard output, with
    /// terminal styling enabled only for human-readable text.
    Output {
        /// Selected serialization or text format.
        format: QueryFormat,
        /// Requested terminal colour policy.
        color: ColorMode,
    },
    /// Render the tldr semantic layout directly to a terminal.
    Tldr(ColorMode),
}

/// Whether process-owned catalog text may use the terminal pager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CatalogPaging {
    Auto,
    Disabled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl From<ColorMode> for clap::ColorChoice {
    fn from(value: ColorMode) -> Self {
        match value {
            ColorMode::Auto => Self::Auto,
            ColorMode::Always => Self::Always,
            ColorMode::Never => Self::Never,
        }
    }
}

impl From<ColorMode> for anstream::ColorChoice {
    fn from(value: ColorMode) -> Self {
        match value {
            ColorMode::Auto => Self::Auto,
            ColorMode::Always => Self::Always,
            ColorMode::Never => Self::Never,
        }
    }
}

/// A discoverable JSON Schema exposed by the native process boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum SchemaContract {
    Doctor,
    Request,
    Query,
    Outline,
    Excerpt,
    Search,
    Catalog,
    All,
}

/// Semantic entries included beneath the ordinary section outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutlineMode {
    Sections,
    #[value(alias = "options")]
    Entries,
}

impl From<OutlineMode> for OutlineDetail {
    fn from(value: OutlineMode) -> Self {
        match value {
            OutlineMode::Sections => Self::Sections,
            OutlineMode::Entries => Self::Entries,
        }
    }
}

/// Case policy exposed without coupling the protocol crate to clap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum SearchCaseMode {
    Insensitive,
    Sensitive,
    Smart,
}

impl From<SearchCaseMode> for SearchCase {
    fn from(value: SearchCaseMode) -> Self {
        match value {
            SearchCaseMode::Insensitive => Self::Insensitive,
            SearchCaseMode::Sensitive => Self::Sensitive,
            SearchCaseMode::Smart => Self::Smart,
        }
    }
}

/// Representation searched while results retain full-Markdown coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum SearchScopeMode {
    Visible,
    Markdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum InputFormatMode {
    Auto,
    Markdown,
    Roff,
}

impl From<InputFormatMode> for InputFormat {
    fn from(value: InputFormatMode) -> Self {
        match value {
            InputFormatMode::Auto => Self::Auto,
            InputFormatMode::Markdown => Self::Markdown,
            InputFormatMode::Roff => Self::Roff,
        }
    }
}

impl From<SearchScopeMode> for SearchScope {
    fn from(value: SearchScopeMode) -> Self {
        match value {
            SearchScopeMode::Visible => Self::Visible,
            SearchScopeMode::Markdown => Self::Markdown,
        }
    }
}

/// Where a query request comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum QuerySource {
    Arguments(QueryRequest),
    StdinJson,
    InputStdin {
        format: InputFormat,
        view: QueryView,
    },
}

/// One validated invocation of the native CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Help(String),
    Query {
        source: QuerySource,
        presentation: QueryPresentation,
        pretty: bool,
        policy: QueryPolicy,
        preserve_anchors: bool,
    },
    Catalog {
        query: CatalogQuery,
        grouped: bool,
        format: QueryFormat,
        pretty: bool,
        paging: CatalogPaging,
    },
    Doctor {
        format: QueryFormat,
        pretty: bool,
        color: ColorMode,
    },
    UpdateTldr {
        pretty: bool,
    },
    UpdateDocs {
        pretty: bool,
    },
    PruneDocs {
        pretty: bool,
        dry_run: bool,
    },
    ProtocolVersion {
        pretty: bool,
    },
    Schema {
        contract: SchemaContract,
        pretty: bool,
    },
    /// Run the read-only MCP server over standard input and output.
    Mcp,
}

// ── Declarative command line ───────────────────────────────────────────────

const CLI_STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().bold())
    .usage(AnsiColor::Green.on_default().bold())
    .literal(AnsiColor::Cyan.on_default().bold())
    .placeholder(AnsiColor::Cyan.on_default())
    .error(AnsiColor::Red.on_default().bold())
    .valid(AnsiColor::Green.on_default())
    .invalid(AnsiColor::Yellow.on_default());

#[derive(Debug, clap::Parser)]
// These booleans are declarative CLI switches, not coupled domain state; clap
// validates their relationships before `Cli` is normalized into `Command`.
#[allow(clippy::struct_excessive_bools)]
#[command(
    name = "mant",
    about = "Read or query structured local manuals and Markdown",
    styles = CLI_STYLES,
    disable_help_flag = true,
    version,
    override_usage = "mant <SELECTOR> [OPTIONS]\n       mant <MAN_SECTION> <NAME> [OPTIONS]\n       mant --input <PATH|-> [--input-format <FORMAT>] [OPTIONS]\n       mant --list [FILTERS]\n       mant --find <PATTERN> [FILTERS]\n       mant --request-json [--format <FORMAT>] [--compact]\n       mant --doctor [--format <text|json>] [--compact]\n       mant --schema <CONTRACT> [--compact]\n       mant --update-docs [--compact]\n       mant --prune-docs [--dry-run] [--compact]\n       mant --update-tldr [--compact]\n       mant --protocol-version [--compact]\n       mant --mcp",
    after_help = "Examples:\n  mant git\n  mant 1 git\n  mant 'git(1)'\n  mant manual/1/git\n  mant --input README.md\n  mant --input /usr/share/man/man1/git.1.gz\n  cat guide.md | mant --input - --input-format markdown\n  mant --list\n  mant --find process --source pwsh7\n  mant git --tldr\n  mant 1 tar --tldr\n  mant gcc --outline\n  mant tar --explain=--exclude\n  mant git --format json --compact\n  mant --doctor\n  mant --update-docs\n  mant --mcp",
    group = ArgGroup::new("action")
        .args(["selector", "input", "list", "find", "request_json", "doctor", "update_docs", "prune_docs", "update_tldr", "protocol_version", "schema", "mcp"])
        .required(true)
        .multiple(false)
)]
struct Cli {
    /// Document selector, or a man-style `MAN_SECTION NAME` pair.
    #[arg(value_name = "SELECTOR", value_parser = non_empty, num_args = 0..)]
    selector: Vec<String>,

    /// Read one explicit Markdown or roff file; use `-` for standard input.
    #[arg(long, value_name = "PATH|-", value_parser = non_empty, help_heading = "Input")]
    input: Option<String>,

    /// Select the parser for `--input`; auto uses the filename suffix.
    #[arg(
        long,
        value_name = "FORMAT",
        value_enum,
        requires = "input",
        help_heading = "Input"
    )]
    input_format: Option<InputFormatMode>,

    /// List locally available documents grouped by source and manual section.
    #[arg(long, help_heading = "Discovery")]
    list: bool,

    /// Find document names using a literal substring or regular expression.
    #[arg(long, value_name = "PATTERN", value_parser = non_empty, help_heading = "Discovery")]
    find: Option<String>,

    /// Restrict document discovery to Markdown or native manuals.
    #[arg(long, value_name = "KIND", value_enum, help_heading = "Discovery")]
    kind: Option<CatalogKindMode>,

    /// Select the full document from a native manual category such as 1 or 3p.
    #[arg(
        long = "man-section",
        value_name = "MAN_SECTION",
        value_parser = non_empty,
        conflicts_with = "input",
        help_heading = "Document selection"
    )]
    man_section: Option<String>,

    /// Select exactly one configured Markdown source.
    #[arg(
        long,
        value_name = "SOURCE",
        value_parser = non_empty,
        conflicts_with_all = ["man_section", "manual", "input"],
        help_heading = "Document selection"
    )]
    source: Option<String>,

    /// Print only a native manual, bypassing Markdown and tldr content.
    #[arg(
        long,
        requires = "selector",
        conflicts_with_all = ["tldr", "input"],
        help_heading = "Document selection"
    )]
    manual: bool,

    /// Print only the available tldr quick reference.
    #[arg(
        long,
        requires = "selector",
        conflicts_with_all = ["manual", "outline", "node", "explain", "search", "ui", "input"],
        help_heading = "Document selection"
    )]
    tldr: bool,

    /// Print the addressable outline tree; semantic entries are included by default.
    #[arg(
        long,
        value_name = "DETAIL",
        value_enum,
        num_args = 0..=1,
        default_missing_value = "entries",
        conflicts_with_all = ["node", "explain"],
        help_heading = "Document selection"
    )]
    outline: Option<OutlineMode>,

    /// Print an outline node selected by path, stable ID, or semantic-entry alias; repeatable.
    #[arg(
        long,
        value_name = "SELECTOR",
        value_parser = non_empty,
        conflicts_with = "explain",
        help_heading = "Document selection"
    )]
    node: Vec<String>,

    /// Explain one option, command, variable, or environment variable by alias, ID, or outline path.
    #[arg(
        long,
        value_name = "ENTRY",
        value_parser = non_empty,
        allow_hyphen_values = true,
        conflicts_with_all = ["outline", "node", "search"],
        help_heading = "Document selection"
    )]
    explain: Option<String>,

    /// Search visible document text and report Markdown lines plus outline nodes.
    #[arg(
        long,
        visible_alias = "grep",
        value_name = "PATTERN",
        value_parser = non_empty,
        conflicts_with_all = ["outline", "node", "explain"],
        help_heading = "Search"
    )]
    search: Option<String>,

    /// Interpret the search pattern as a regular expression instead of a literal.
    #[arg(long, help_heading = "Search")]
    regex: bool,

    /// Select case handling for search matches.
    #[arg(
        long = "case",
        value_name = "POLICY",
        value_enum,
        help_heading = "Search"
    )]
    search_case: Option<SearchCaseMode>,

    /// Match the pattern only at Unicode-aware word boundaries.
    #[arg(long, requires = "search", help_heading = "Search")]
    word: bool,

    /// Search visible text or the generated Markdown source.
    #[arg(
        long = "scope",
        value_name = "SCOPE",
        value_enum,
        requires = "search",
        help_heading = "Search"
    )]
    search_scope: Option<SearchScopeMode>,

    /// Include this many full Markdown lines before and after each match.
    #[arg(
        long,
        value_name = "LINES",
        requires = "search",
        help_heading = "Search"
    )]
    context: Option<u16>,

    /// Return at most this many matches.
    #[arg(long, value_name = "COUNT", help_heading = "Search")]
    limit: Option<u32>,

    /// Skip this many matches for deterministic pagination.
    #[arg(long, value_name = "COUNT", help_heading = "Search")]
    offset: Option<u32>,

    /// Read a versioned `QueryRequest` JSON object from standard input.
    #[arg(
        long,
        conflicts_with_all = [
            "man_section",
            "tldr",
            "outline",
            "node",
            "explain",
            "search",
            "regex",
            "search_case",
            "word",
            "search_scope",
            "context",
            "limit",
            "offset"
        ],
        help_heading = "Integration"
    )]
    request_json: bool,

    /// Open the interactive terminal reader explicitly.
    #[arg(
        long,
        conflicts_with_all = [
            "outline",
            "tldr",
            "node",
            "explain",
            "search",
            "request_json",
            "update_tldr",
            "protocol_version",
            "schema",
            "mcp",
            "format",
            "compact",
            "preserve_anchors"
        ],
        help_heading = "Reading"
    )]
    ui: bool,

    /// Diagnose local paths, sources, manuals, and tldr caches without changing them.
    #[arg(
        long,
        conflicts_with_all = [
            "selector",
            "input",
            "input_format",
            "list",
            "find",
            "kind",
            "man_section",
            "source",
            "manual",
            "tldr",
            "outline",
            "node",
            "explain",
            "search",
            "regex",
            "search_case",
            "word",
            "search_scope",
            "context",
            "limit",
            "offset",
            "request_json",
            "ui",
            "dry_run",
            "preserve_anchors",
            "no_pager"
        ],
        help_heading = "Diagnostics"
    )]
    doctor: bool,

    /// Update tldr data through the installed client or `ManT` cache.
    #[arg(
        long,
        conflicts_with_all = ["man_section", "outline", "node", "search", "format"],
        help_heading = "Data"
    )]
    update_tldr: bool,

    /// Update configured Markdown repositories from sources.toml.
    #[arg(
        long,
        conflicts_with_all = ["man_section", "source", "outline", "node", "search", "format"],
        help_heading = "Data"
    )]
    update_docs: bool,

    /// Remove installed document sources absent from sources.toml.
    #[arg(
        long,
        conflicts_with_all = ["man_section", "source", "outline", "node", "search", "format"],
        help_heading = "Data"
    )]
    prune_docs: bool,

    /// Report exact orphaned source targets without removing them.
    #[arg(long, requires = "prune_docs", help_heading = "Data")]
    dry_run: bool,

    /// Print the native protocol description as JSON.
    #[arg(
        long,
        conflicts_with_all = ["man_section", "outline", "node", "search", "format"],
        help_heading = "Integration"
    )]
    protocol_version: bool,

    /// Print a generated JSON Schema contract (`doctor`, `request`, `query`, `outline`, `excerpt`, `search`, `catalog`, or `all`).
    #[arg(
        long,
        value_name = "CONTRACT",
        value_enum,
        conflicts_with_all = ["man_section", "outline", "node", "search", "format"],
        help_heading = "Integration"
    )]
    schema: Option<SchemaContract>,

    /// Serve read-only manual queries through the MCP stdio transport.
    #[arg(
        long,
        conflicts_with_all = [
            "selector",
            "input",
            "input_format",
            "man_section",
            "source",
            "outline",
            "node",
            "explain",
            "search",
            "regex",
            "search_case",
            "word",
            "search_scope",
            "context",
            "limit",
            "offset",
            "request_json",
            "manual",
            "tldr",
            "update_tldr",
            "update_docs",
            "prune_docs",
            "doctor",
            "dry_run",
            "protocol_version",
            "schema",
            "format",
            "compact",
            "preserve_anchors"
        ],
        help_heading = "Integration"
    )]
    mcp: bool,

    /// Output format. Full content defaults to markdown; outlines and search default to text.
    #[arg(long, value_name = "FORMAT", value_enum, help_heading = "Output")]
    format: Option<QueryFormat>,

    /// Control colors in human-readable terminal output.
    #[arg(long, value_enum, help_heading = "Output")]
    color: Option<ColorMode>,

    /// Omit JSON indentation. Query output also requires `--format json`.
    #[arg(long, help_heading = "Output")]
    compact: bool,

    /// Print discovery text directly instead of opening the terminal pager.
    #[arg(long, help_heading = "Output")]
    no_pager: bool,

    /// Preserve raw HTML anchors and document-local links in Markdown output.
    #[arg(
        long,
        conflicts_with_all = ["update_docs", "prune_docs", "update_tldr", "protocol_version", "schema", "mcp"],
        help_heading = "Output"
    )]
    preserve_anchors: bool,

    /// Print help.
    #[arg(short = 'h', long, action = ArgAction::Help, help_heading = "General")]
    help: Option<bool>,
}

// ── Normalization and semantic validation ─────────────────────────────────

pub(crate) fn parse(arguments: &[String]) -> Result<Command, clap::Error> {
    parse_with_help(arguments, HelpBehavior::Capture)
}

/// Parse one native process invocation while preserving clap's styled help or
/// diagnostic for its terminal-aware stdout/stderr printer.
pub(crate) fn parse_process(arguments: &[String]) -> Result<Command, clap::Error> {
    parse_with_help(arguments, HelpBehavior::Return)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelpBehavior {
    Capture,
    Return,
}

fn parse_with_help(
    arguments: &[String],
    help_behavior: HelpBehavior,
) -> Result<Command, clap::Error> {
    let color = requested_color(arguments);
    if uses_removed_section_option(arguments) {
        return Err(command_error(
            ErrorKind::UnknownArgument,
            "--section was removed in ManT 0.7.0 because \"section\" is ambiguous\n\n  select a Unix manual category:\n    mant <NAME> --man-section <MAN_SECTION>\n\n  select a document heading or outline node:\n    mant <NAME> --node <SELECTOR>\n\n  inspect available outline nodes:\n    mant <NAME> --outline",
            color,
        ));
    }
    let parsed = match parse_cli(arguments, color) {
        Ok(parsed) => parsed,
        Err(error)
            if help_behavior == HelpBehavior::Capture
                && matches!(
                    error.kind(),
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                ) =>
        {
            return Ok(Command::Help(error.to_string()));
        }
        Err(error) => return Err(error),
    };

    normalize(parsed, color)
}

fn parse_cli(arguments: &[String], color: ColorMode) -> Result<Cli, clap::Error> {
    let mut command = Cli::command().color(color.into());
    let mut matches = command
        .try_get_matches_from_mut(iter::once("mant").chain(arguments.iter().map(String::as_str)))?;
    Cli::from_arg_matches_mut(&mut matches).map_err(|error| error.format(&mut command))
}

pub(crate) fn requested_color(arguments: &[String]) -> ColorMode {
    let mut arguments = arguments.iter();
    let mut color = ColorMode::Auto;
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            break;
        }
        if argument == "--color" {
            if let Some(value) = arguments.next() {
                color = color_value(value).unwrap_or(ColorMode::Auto);
            }
        } else if let Some(value) = argument.strip_prefix("--color=") {
            color = color_value(value).unwrap_or(ColorMode::Auto);
        }
    }
    color
}

fn color_value(value: &str) -> Option<ColorMode> {
    match value {
        "auto" => Some(ColorMode::Auto),
        "always" => Some(ColorMode::Always),
        "never" => Some(ColorMode::Never),
        _ => None,
    }
}

fn uses_removed_section_option(arguments: &[String]) -> bool {
    let mut explain_value = false;
    for argument in arguments {
        if explain_value {
            explain_value = false;
            continue;
        }
        if argument == "--" {
            break;
        }
        if argument == "--explain" {
            explain_value = true;
            continue;
        }
        if argument == "--section" || argument.starts_with("--section=") {
            return true;
        }
    }
    false
}

fn normalize(mut parsed: Cli, color: ColorMode) -> Result<Command, clap::Error> {
    if parsed.no_pager && !parsed.list && parsed.find.is_none() {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--no-pager applies only to --list and --find",
            color,
        ));
    }
    if parsed.mcp {
        return Ok(Command::Mcp);
    }
    if parsed.doctor {
        return normalize_doctor(&parsed, color);
    }
    if parsed.update_docs {
        return Ok(Command::UpdateDocs {
            pretty: !parsed.compact,
        });
    }
    if parsed.prune_docs {
        return Ok(Command::PruneDocs {
            pretty: !parsed.compact,
            dry_run: parsed.dry_run,
        });
    }
    if parsed.update_tldr {
        return Ok(Command::UpdateTldr {
            pretty: !parsed.compact,
        });
    }
    if parsed.protocol_version {
        return Ok(Command::ProtocolVersion {
            pretty: !parsed.compact,
        });
    }
    if let Some(contract) = parsed.schema {
        return Ok(Command::Schema {
            contract,
            pretty: !parsed.compact,
        });
    }
    if parsed.list || parsed.find.is_some() {
        return normalize_catalog(parsed, color);
    }
    validate_query_search_options(&parsed, color)?;

    let view = normalize_query_view(&mut parsed);
    validate_output_options(
        parsed.compact,
        parsed.format,
        parsed.preserve_anchors,
        &view,
        color,
    )?;
    let source = normalize_query_source(
        QuerySourceOptions {
            request_json: parsed.request_json,
            selectors: parsed.selector,
            input_path: parsed.input,
            input_format: parsed.input_format,
            configured_source: parsed.source,
            manual_section: parsed.man_section,
            tldr: parsed.tldr,
        },
        view,
        color,
    )?;
    validate_manual_source(parsed.manual, &source, color)?;
    let presentation = normalize_presentation(
        parsed.ui,
        parsed.format,
        parsed.preserve_anchors,
        &source,
        parsed.tldr,
        parsed.color,
    );

    Ok(Command::Query {
        source,
        presentation,
        pretty: !parsed.compact,
        policy: if parsed.manual {
            QueryPolicy::ManualOnly
        } else if parsed.tldr {
            QueryPolicy::TldrOnly
        } else {
            QueryPolicy::Combined
        },
        preserve_anchors: parsed.preserve_anchors,
    })
}

fn normalize_doctor(parsed: &Cli, color: ColorMode) -> Result<Command, clap::Error> {
    let format = parsed.format.unwrap_or(QueryFormat::Text);
    if !matches!(format, QueryFormat::Text | QueryFormat::Json) {
        return Err(command_error(
            ErrorKind::InvalidValue,
            "doctor supports only text and json formats",
            color,
        ));
    }
    if parsed.compact && format != QueryFormat::Json {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--compact requires --format json",
            color,
        ));
    }
    Ok(Command::Doctor {
        format,
        pretty: !parsed.compact,
        color: parsed.color.unwrap_or_default(),
    })
}

fn normalize_catalog(parsed: Cli, color: ColorMode) -> Result<Command, clap::Error> {
    if parsed.list && (parsed.regex || parsed.search_case.is_some()) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--regex and --case require --find",
            color,
        ));
    }
    if parsed.word || parsed.search_scope.is_some() || parsed.context.is_some() {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--word, --scope, and --context apply only to document-content search",
            color,
        ));
    }
    if parsed.preserve_anchors {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--preserve-anchors does not apply to document discovery",
            color,
        ));
    }
    if parsed.source.is_some() && parsed.kind == Some(CatalogKindMode::Manual)
        || parsed.man_section.is_some() && parsed.kind == Some(CatalogKindMode::Markdown)
    {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--source selects Markdown while --man-section selects native manuals",
            color,
        ));
    }
    let format = parsed.format.unwrap_or(QueryFormat::Text);
    if !matches!(format, QueryFormat::Text | QueryFormat::Json) {
        return Err(command_error(
            ErrorKind::InvalidValue,
            "document discovery supports only text and json formats",
            color,
        ));
    }
    if parsed.compact && format != QueryFormat::Json {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--compact requires --format json",
            color,
        ));
    }
    Ok(Command::Catalog {
        query: CatalogQuery {
            pattern: parsed.find,
            syntax: if parsed.regex {
                SearchSyntax::Regex
            } else {
                SearchSyntax::Literal
            },
            case: parsed
                .search_case
                .map_or(SearchCase::Insensitive, Into::into),
            kind: parsed.kind.map(Into::into),
            source: parsed.source,
            manual_section: parsed.man_section,
            limit: parsed.limit.unwrap_or(10_000),
            offset: parsed.offset.unwrap_or(0),
        },
        grouped: parsed.list,
        format,
        pretty: !parsed.compact,
        paging: if parsed.no_pager {
            CatalogPaging::Disabled
        } else {
            CatalogPaging::Auto
        },
    })
}

fn validate_query_search_options(parsed: &Cli, color: ColorMode) -> Result<(), clap::Error> {
    if parsed.search.is_none()
        && (parsed.regex
            || parsed.search_case.is_some()
            || parsed.limit.is_some()
            || parsed.offset.is_some())
    {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--regex, --case, --limit, and --offset require --search or --find",
            color,
        ));
    }
    if parsed.kind.is_some() {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--kind requires --list or --find",
            color,
        ));
    }
    Ok(())
}

fn normalize_query_view(parsed: &mut Cli) -> QueryView {
    if parsed.tldr {
        QueryView::Excerpt {
            selectors: vec![NodeSelector::from("tldr")],
        }
    } else if let Some(detail) = parsed.outline.take() {
        QueryView::Outline {
            detail: detail.into(),
        }
    } else if let Some(pattern) = parsed.search.take() {
        QueryView::Search {
            pattern,
            syntax: if parsed.regex {
                SearchSyntax::Regex
            } else {
                SearchSyntax::Literal
            },
            case: parsed
                .search_case
                .take()
                .map_or(SearchCase::Insensitive, Into::into),
            scope: parsed
                .search_scope
                .take()
                .map_or(SearchScope::Visible, Into::into),
            word: parsed.word,
            context_lines: parsed.context.take().unwrap_or(0),
            limit: parsed.limit.take().unwrap_or_else(default_search_limit),
            offset: parsed.offset.take().unwrap_or(0),
        }
    } else if let Some(selector) = parsed.explain.take() {
        QueryView::Explain { entry: selector }
    } else if parsed.node.is_empty() {
        QueryView::Full {}
    } else {
        QueryView::Excerpt {
            selectors: std::mem::take(&mut parsed.node)
                .into_iter()
                .map(NodeSelector::from)
                .collect(),
        }
    }
}

fn validate_output_options(
    compact: bool,
    format: Option<QueryFormat>,
    preserve_anchors: bool,
    view: &QueryView,
    color: ColorMode,
) -> Result<(), clap::Error> {
    if compact && format != Some(QueryFormat::Json) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--compact requires --format json for manual queries",
            color,
        ));
    }
    if preserve_anchors && format.is_some_and(|format| format != QueryFormat::Markdown) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--preserve-anchors requires Markdown output",
            color,
        ));
    }
    if preserve_anchors && matches!(view, QueryView::Outline { .. } | QueryView::Search { .. }) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--preserve-anchors applies only to full documents and excerpts",
            color,
        ));
    }
    if format == Some(QueryFormat::Man) && !matches!(view, QueryView::Full {}) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--format man applies only to full documents",
            color,
        ));
    }
    Ok(())
}

fn validate_manual_source(
    manual: bool,
    source: &QuerySource,
    color: ColorMode,
) -> Result<(), clap::Error> {
    if manual
        && !matches!(
            source,
            QuerySource::Arguments(QueryRequest {
                input: QueryInput::Document { .. },
                ..
            })
        )
    {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--manual requires a document name rather than Markdown input",
            color,
        ));
    }
    Ok(())
}

fn normalize_presentation(
    ui: bool,
    format: Option<QueryFormat>,
    preserve_anchors: bool,
    source: &QuerySource,
    tldr: bool,
    color: Option<ColorMode>,
) -> QueryPresentation {
    let view = match source {
        QuerySource::Arguments(request) => Some(&request.view),
        QuerySource::InputStdin { view, .. } => Some(view),
        QuerySource::StdinJson => None,
    };
    let default_format = view.map_or(QueryFormat::Markdown, |view| {
        if matches!(view, QueryView::Full {}) {
            QueryFormat::Markdown
        } else {
            QueryFormat::Text
        }
    });
    if ui {
        QueryPresentation::Interactive
    } else if let Some(format) = format {
        QueryPresentation::Output {
            format,
            color: color.unwrap_or_default(),
        }
    } else if tldr {
        QueryPresentation::Tldr(color.unwrap_or_default())
    } else if preserve_anchors {
        QueryPresentation::Output {
            format: QueryFormat::Markdown,
            color: color.unwrap_or_default(),
        }
    } else if matches!(
        source,
        QuerySource::Arguments(QueryRequest {
            view: QueryView::Full {},
            ..
        })
    ) {
        QueryPresentation::Auto
    } else {
        QueryPresentation::Output {
            format: default_format,
            color: color.unwrap_or_default(),
        }
    }
}

struct QuerySourceOptions {
    request_json: bool,
    selectors: Vec<String>,
    input_path: Option<String>,
    input_format: Option<InputFormatMode>,
    configured_source: Option<String>,
    manual_section: Option<String>,
    tldr: bool,
}

struct NormalizedDocumentSelector {
    name: String,
    manual_section: Option<String>,
}

fn normalize_query_source(
    options: QuerySourceOptions,
    view: QueryView,
    color: ColorMode,
) -> Result<QuerySource, clap::Error> {
    let source = if options.request_json {
        QuerySource::StdinJson
    } else if let Some(path) = options.input_path {
        let format = options.input_format.map_or(InputFormat::Auto, Into::into);
        if path == "-" {
            if format == InputFormat::Auto {
                return Err(command_error(
                    ErrorKind::MissingRequiredArgument,
                    "--input - requires --input-format markdown or roff",
                    color,
                ));
            }
            QuerySource::InputStdin { format, view }
        } else {
            QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V7,
                input: QueryInput::File { path, format },
                view,
            })
        }
    } else {
        let normalized = normalize_document_operands(
            &options.selectors,
            options.manual_section,
            options.tldr,
            color,
        )?;
        QuerySource::Arguments(QueryRequest {
            schema: RequestSchema::V7,
            input: QueryInput::Document {
                selector: normalized.name,
                source: options.configured_source,
                manual_section: normalized.manual_section,
            },
            view,
        })
    };
    Ok(source)
}

fn normalize_document_operands(
    operands: &[String],
    explicit_manual_section: Option<String>,
    tldr: bool,
    color: ColorMode,
) -> Result<NormalizedDocumentSelector, clap::Error> {
    if operands.is_empty() {
        return Err(command_error(
            ErrorKind::MissingRequiredArgument,
            if tldr {
                "--tldr requires a page name"
            } else {
                "a document selector or --input is required"
            },
            color,
        ));
    }

    if tldr {
        let inline_manual = match operands {
            [section, name] if is_manual_section(section) => {
                Some((name.as_str(), section.as_str()))
            }
            [selector] => parenthesized_manual_reference(selector),
            _ => None,
        };
        if let Some((name, section)) = inline_manual {
            return merge_manual_section(name, section, explicit_manual_section.is_some(), color);
        }
        return Ok(NormalizedDocumentSelector {
            name: normalize_tldr_topic(&operands.join(" ")),
            manual_section: explicit_manual_section,
        });
    }

    match operands {
        [selector] => {
            if let Some((name, section)) = parenthesized_manual_reference(selector) {
                merge_manual_section(name, section, explicit_manual_section.is_some(), color)
            } else {
                Ok(NormalizedDocumentSelector {
                    name: selector.clone(),
                    manual_section: explicit_manual_section,
                })
            }
        }
        [section, name] if is_manual_section(section) => {
            merge_manual_section(name, section, explicit_manual_section.is_some(), color)
        }
        _ => Err(command_error(
            ErrorKind::TooManyValues,
            "use one document selector, or SECTION NAME for a native manual",
            color,
        )),
    }
}

fn merge_manual_section(
    name: &str,
    inline_section: &str,
    has_explicit_section: bool,
    color: ColorMode,
) -> Result<NormalizedDocumentSelector, clap::Error> {
    if has_explicit_section {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "a man-style section selector cannot be combined with --man-section",
            color,
        ));
    }
    Ok(NormalizedDocumentSelector {
        name: name.to_owned(),
        manual_section: Some(inline_section.to_owned()),
    })
}

fn non_empty(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err("value must not be empty".to_owned())
    } else {
        Ok(value.to_owned())
    }
}

fn command_error(
    kind: ErrorKind,
    message: impl std::fmt::Display,
    color: ColorMode,
) -> clap::Error {
    Cli::command().color(color.into()).error(kind, message)
}

#[cfg(test)]
mod tests {
    use mant_protocol::{
        CatalogDocumentKind, CatalogQuery, InputFormat, OutlineDetail, QueryInput, QueryRequest,
        QueryView, RequestSchema, SearchCase, SearchScope, SearchSyntax,
    };

    use super::{
        CatalogPaging, ColorMode, Command, QueryFormat, QueryPolicy, QueryPresentation,
        QuerySource, SchemaContract, parse, parse_process, requested_color,
    };

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn defaults_direct_queries_to_markdown() {
        assert_eq!(
            parse(&args(&["git"])).expect("query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V7,
                    input: QueryInput::Document {
                        selector: "git".to_owned(),
                        source: None,
                        manual_section: None,
                    },
                    view: QueryView::Full {},
                }),
                presentation: QueryPresentation::Auto,
                pretty: true,
                policy: QueryPolicy::Combined,
                preserve_anchors: false,
            }
        );
    }

    #[test]
    fn parses_grouped_lists_and_grep_like_catalog_searches() {
        assert_eq!(
            parse(&args(&["--list", "--source", "pwsh7"])).expect("catalog list"),
            Command::Catalog {
                query: CatalogQuery {
                    pattern: None,
                    source: Some("pwsh7".to_owned()),
                    limit: 10_000,
                    ..CatalogQuery::default()
                },
                grouped: true,
                format: QueryFormat::Text,
                pretty: true,
                paging: CatalogPaging::Auto,
            }
        );
        assert_eq!(
            parse(&args(&[
                "--find",
                "^PRINT",
                "--regex",
                "--case",
                "sensitive",
                "--kind",
                "manual",
                "--man-section",
                "3",
                "--limit",
                "20",
                "--format",
                "json",
                "--compact",
            ]))
            .expect("catalog search"),
            Command::Catalog {
                query: CatalogQuery {
                    pattern: Some("^PRINT".to_owned()),
                    syntax: SearchSyntax::Regex,
                    case: SearchCase::Sensitive,
                    kind: Some(CatalogDocumentKind::Manual),
                    source: None,
                    manual_section: Some("3".to_owned()),
                    limit: 20,
                    offset: 0,
                },
                grouped: false,
                format: QueryFormat::Json,
                pretty: false,
                paging: CatalogPaging::Auto,
            }
        );

        assert!(matches!(
            parse(&args(&["--list", "--no-pager"])).expect("direct catalog list"),
            Command::Catalog {
                paging: CatalogPaging::Disabled,
                ..
            }
        ));

        for invalid in [
            vec!["--list", "--regex"],
            vec!["--list", "--format", "markdown"],
            vec!["git", "--limit", "2"],
            vec!["git", "--kind", "manual"],
            vec!["git", "--no-pager"],
            vec!["git", "--outline", "--format", "man"],
            vec!["git", "--node", "1", "--format", "man"],
            vec!["git", "--explain", "branch", "--format", "man"],
            vec!["git", "--search", "branch", "--format", "man"],
        ] {
            assert!(parse(&args(&invalid)).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn parses_an_explicit_interactive_query_without_an_output_projection() {
        assert!(matches!(
            parse(&args(&["git", "--ui"])).expect("interactive query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    input: QueryInput::Document { ref selector, .. },
                    view: QueryView::Full {},
                    ..
                }),
                presentation: QueryPresentation::Interactive,
                ..
            } if selector == "git"
        ));

        for conflicting in ["--outline", "--search=git", "--format=json"] {
            assert!(
                parse(&args(&["git", "--ui", conflicting])).is_err(),
                "accepted {conflicting}"
            );
        }
    }

    #[test]
    fn dispatches_explicit_files_and_direct_stdin_without_embedding_content() {
        for path in ["README.md", "docs/guide", "./notes"] {
            assert!(matches!(
                parse(&args(&["--input", path])).expect("input file query"),
                Command::Query {
                    source: QuerySource::Arguments(QueryRequest {
                        input: QueryInput::File {
                            path: parsed,
                            format: InputFormat::Auto,
                        },
                        ..
                    }),
                    ..
                } if parsed == path
            ));
        }

        assert!(matches!(
            parse(&args(&[
                "--input",
                "-",
                "--input-format",
                "markdown",
                "--outline"
            ]))
            .expect("piped Markdown outline"),
            Command::Query {
                source: QuerySource::InputStdin {
                    format: InputFormat::Markdown,
                    view: QueryView::Outline {
                        detail: OutlineDetail::Entries
                    }
                },
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Text,
                    color: ColorMode::Auto
                },
                ..
            }
        ));
        assert!(
            parse(&args(&["--input", "README.md", "--man-section", "1"]))
                .expect_err("input has no man section selector")
                .to_string()
                .contains("cannot be used with")
        );
    }

    #[test]
    fn preserves_markdown_anchors_only_when_requested() {
        assert!(matches!(
            parse(&args(&["git", "--preserve-anchors"])).expect("addressable Markdown"),
            Command::Query {
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Markdown,
                    color: ColorMode::Auto
                },
                preserve_anchors: true,
                ..
            }
        ));
    }

    #[test]
    fn parses_format_man_section_and_compact_json_options() {
        assert_eq!(
            parse(&args(&[
                "printf",
                "--man-section",
                "3",
                "--format",
                "json",
                "--compact",
            ]))
            .expect("query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V7,
                    input: QueryInput::Document {
                        selector: "printf".to_owned(),
                        source: None,
                        manual_section: Some("3".to_owned()),
                    },
                    view: QueryView::Full {},
                }),
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Json,
                    color: ColorMode::Auto
                },
                pretty: false,
                policy: QueryPolicy::Combined,
                preserve_anchors: false,
            }
        );
        assert!(matches!(
            parse(&args(&["printf", "--source", "team"])).expect("source query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    input: QueryInput::Document {
                        ref source,
                        manual_section: None,
                        ..
                    },
                    ..
                }),
                ..
            } if source.as_deref() == Some("team")
        ));
    }

    #[test]
    fn removed_section_option_is_hidden_and_explains_both_replacements() {
        let Command::Help(help) = parse(&args(&["--help"])).expect("help") else {
            panic!("expected help output")
        };
        assert!(!help.contains("--section"));

        for arguments in [
            vec!["cmake", "--section", "1"],
            vec!["cmake", "--section=DESCRIPTION"],
            vec!["--section", "1"],
        ] {
            let diagnostic = parse(&args(&arguments))
                .expect_err("removed option")
                .to_string();
            assert!(diagnostic.contains("--section was removed in ManT 0.7.0"));
            assert!(diagnostic.contains("--man-section <MAN_SECTION>"));
            assert!(diagnostic.contains("--node <SELECTOR>"));
            assert!(diagnostic.contains("--outline"));
        }

        assert!(!super::uses_removed_section_option(&args(&[
            "cmake",
            "--explain",
            "--section",
        ])));
    }

    #[test]
    fn process_help_retains_styles_while_injected_help_stays_plain() {
        let arguments = args(&["--help", "--color", "always"]);
        let Command::Help(help) = parse(&arguments).expect("captured help") else {
            panic!("expected captured help")
        };
        assert!(!help.contains('\u{1b}'));

        let styled = parse_process(&arguments).expect_err("process help remains a clap display");
        assert_eq!(styled.kind(), clap::error::ErrorKind::DisplayHelp);
        assert!(styled.render().ansi().to_string().contains('\u{1b}'));
    }

    #[test]
    fn color_policy_is_global_without_changing_deterministic_presentations() {
        assert!(matches!(
            parse(&args(&["git", "--format", "json", "--color", "always"]))
                .expect("JSON query with terminal color policy"),
            Command::Query {
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Json,
                    color: ColorMode::Always
                },
                ..
            }
        ));
        assert!(matches!(
            parse(&args(&["git", "--tldr", "--color", "never"])).expect("plain tldr query"),
            Command::Query {
                presentation: QueryPresentation::Tldr(ColorMode::Never),
                ..
            }
        ));

        assert_eq!(
            requested_color(&args(&["git", "--color=always"])),
            ColorMode::Always
        );
        assert_eq!(
            requested_color(&args(&["git", "--color", "never"])),
            ColorMode::Never
        );
        assert_eq!(
            requested_color(&args(&["--", "--color=always"])),
            ColorMode::Auto
        );
    }

    #[test]
    fn normalizes_man_style_and_hierarchical_selectors() {
        for values in [vec!["1", "git"], vec!["git(1)"]] {
            assert!(matches!(
                parse(&args(&values)).expect("man-style selector"),
                Command::Query {
                    source: QuerySource::Arguments(QueryRequest {
                        input: QueryInput::Document {
                            ref selector,
                            manual_section: Some(ref manual_section),
                            ..
                        },
                        ..
                    }),
                    ..
                } if selector == "git" && manual_section == "1"
            ));
        }
        assert!(matches!(
            parse(&args(&["manual/1/git"])).expect("canonical selector"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    input: QueryInput::Document { ref selector, manual_section: None, .. },
                    ..
                }),
                ..
            } if selector == "manual/1/git"
        ));
        assert!(matches!(
            parse(&args(&["git.1"])).expect("dotted logical name"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    input: QueryInput::Document {
                        ref selector,
                        manual_section: None,
                        ..
                    },
                    ..
                }),
                ..
            } if selector == "git.1"
        ));
    }

    #[test]
    fn tldr_joins_multiword_topics_and_keeps_explicit_formats() {
        assert!(matches!(
            parse(&args(&["git", "checkout", "--tldr"])).expect("multiword tldr topic"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    input: QueryInput::Document { ref selector, .. },
                    ..
                }),
                presentation: QueryPresentation::Tldr(ColorMode::Auto),
                ..
            } if selector == "git-checkout"
        ));
        assert!(matches!(
            parse(&args(&["git", "--tldr", "--format", "json"])).expect("structured tldr output"),
            Command::Query {
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Json,
                    color: ColorMode::Auto
                },
                ..
            }
        ));

        for values in [
            vec!["1", "tar", "--tldr"],
            vec!["tar(1)", "--tldr"],
            vec!["tar", "--man-section", "1", "--tldr"],
        ] {
            assert!(matches!(
                parse(&args(&values)).expect("command section qualifies a tldr topic"),
                Command::Query {
                    source: QuerySource::Arguments(QueryRequest {
                        input: QueryInput::Document {
                            ref selector,
                            manual_section: Some(ref manual_section),
                            ..
                        },
                        ..
                    }),
                    policy: QueryPolicy::TldrOnly,
                    ..
                } if selector == "tar" && manual_section == "1"
            ));
        }

        assert!(matches!(
            parse(&args(&["command.1", "--tldr"]))
                .expect("dots remain part of explicit tldr topics"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    input: QueryInput::Document {
                        ref selector,
                        manual_section: None,
                        ..
                    },
                    ..
                }),
                policy: QueryPolicy::TldrOnly,
                ..
            } if selector == "command.1"
        ));
    }

    #[test]
    fn parses_the_closed_stdin_request_mode_used_by_the_tui() {
        assert_eq!(
            parse(&args(&["--request-json", "--format", "json", "--compact",]))
                .expect("stdin query"),
            Command::Query {
                source: QuerySource::StdinJson,
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Json,
                    color: ColorMode::Auto
                },
                pretty: false,
                policy: QueryPolicy::Combined,
                preserve_anchors: false,
            }
        );
    }

    #[test]
    fn parses_explicit_manual_and_tldr_selections() {
        assert!(matches!(
            parse(&args(&["tar", "--manual", "--format", "json"])).expect("manual-only query"),
            Command::Query {
                policy: QueryPolicy::ManualOnly,
                ..
            }
        ));
        assert!(matches!(
            parse(&args(&["tar", "--tldr"])).expect("tldr-only query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    view: QueryView::Excerpt { ref selectors },
                    ..
                }),
                presentation: QueryPresentation::Tldr(ColorMode::Auto),
                policy: QueryPolicy::TldrOnly,
                ..
            } if selectors == &["tldr"]
        ));
    }

    #[test]
    fn parses_outline_and_repeatable_node_views_with_contextual_defaults() {
        assert_eq!(
            parse(&args(&["gcc", "--outline"])).expect("outline"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V7,
                    input: QueryInput::Document {
                        selector: "gcc".to_owned(),
                        source: None,
                        manual_section: None,
                    },
                    view: QueryView::Outline {
                        detail: OutlineDetail::Entries,
                    },
                }),
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Text,
                    color: ColorMode::Auto
                },
                pretty: true,
                policy: QueryPolicy::Combined,
                preserve_anchors: false,
            }
        );
        assert_eq!(
            parse(&args(&["tar", "--outline", "options", "--format", "json"]))
                .expect("option outline"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V7,
                    input: QueryInput::Document {
                        selector: "tar".to_owned(),
                        source: None,
                        manual_section: None,
                    },
                    view: QueryView::Outline {
                        detail: OutlineDetail::Entries,
                    },
                }),
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Json,
                    color: ColorMode::Auto
                },
                pretty: true,
                policy: QueryPolicy::Combined,
                preserve_anchors: false,
            }
        );
        assert_eq!(
            parse(&args(&[
                "gcc", "--node", "4.2", "--node", "files-8", "--format", "text",
            ]))
            .expect("excerpt"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V7,
                    input: QueryInput::Document {
                        selector: "gcc".to_owned(),
                        source: None,
                        manual_section: None,
                    },
                    view: QueryView::Excerpt {
                        selectors: vec!["4.2".into(), "files-8".into()],
                    },
                }),
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Text,
                    color: ColorMode::Auto
                },
                pretty: true,
                policy: QueryPolicy::Combined,
                preserve_anchors: false,
            }
        );
    }

    #[test]
    fn parses_explain_as_a_first_class_semantic_view() {
        for (values, selector) in [
            (vec!["tar", "--explain=--exclude"], "--exclude"),
            (vec!["tar", "--explain", "--exclude"], "--exclude"),
            (vec!["tar", "--explain", "exclude"], "exclude"),
        ] {
            assert_eq!(
                parse(&args(&values)).expect("explain query"),
                Command::Query {
                    source: QuerySource::Arguments(QueryRequest {
                        schema: RequestSchema::V7,
                        input: QueryInput::Document {
                            selector: "tar".to_owned(),
                            source: None,
                            manual_section: None,
                        },
                        view: QueryView::Explain {
                            entry: selector.to_owned(),
                        },
                    }),
                    presentation: QueryPresentation::Output {
                        format: QueryFormat::Text,
                        color: ColorMode::Auto
                    },
                    pretty: true,
                    policy: QueryPolicy::Combined,
                    preserve_anchors: false,
                }
            );
        }
    }

    #[test]
    fn defaults_all_partial_document_views_to_text() {
        for values in [
            vec!["gcc", "--node", "4.2"],
            vec!["gcc", "--outline"],
            vec!["gcc", "--search", "link"],
        ] {
            assert!(matches!(
                parse(&args(&values)).expect("partial document query"),
                Command::Query {
                    presentation: QueryPresentation::Output {
                        format: QueryFormat::Text,
                        color: ColorMode::Auto
                    },
                    ..
                }
            ));
        }
    }

    #[test]
    fn parses_literal_and_regex_searches_with_text_as_the_default() {
        assert_eq!(
            parse(&args(&["tar", "--search=--acls"])).expect("literal search"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V7,
                    input: QueryInput::Document {
                        selector: "tar".to_owned(),
                        source: None,
                        manual_section: None,
                    },
                    view: QueryView::Search {
                        pattern: "--acls".to_owned(),
                        syntax: SearchSyntax::Literal,
                        case: SearchCase::Insensitive,
                        scope: SearchScope::Visible,
                        word: false,
                        context_lines: 0,
                        limit: 100,
                        offset: 0,
                    },
                }),
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Text,
                    color: ColorMode::Auto
                },
                pretty: true,
                policy: QueryPolicy::Combined,
                preserve_anchors: false,
            }
        );
        assert_eq!(
            parse(&args(&[
                "git",
                "--grep",
                "worktree|branch",
                "--regex",
                "--case",
                "smart",
                "--word",
                "--scope",
                "markdown",
                "--context",
                "2",
                "--limit",
                "20",
                "--offset",
                "5",
                "--format",
                "json",
            ]))
            .expect("regex search"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V7,
                    input: QueryInput::Document {
                        selector: "git".to_owned(),
                        source: None,
                        manual_section: None,
                    },
                    view: QueryView::Search {
                        pattern: "worktree|branch".to_owned(),
                        syntax: SearchSyntax::Regex,
                        case: SearchCase::Smart,
                        scope: SearchScope::Markdown,
                        word: true,
                        context_lines: 2,
                        limit: 20,
                        offset: 5,
                    },
                }),
                presentation: QueryPresentation::Output {
                    format: QueryFormat::Json,
                    color: ColorMode::Auto
                },
                pretty: true,
                policy: QueryPolicy::Combined,
                preserve_anchors: false,
            }
        );
    }

    #[test]
    fn parses_long_option_actions_without_ad_hoc_subcommands() {
        assert_eq!(
            parse(&args(&["--doctor"])).expect("doctor"),
            Command::Doctor {
                format: QueryFormat::Text,
                pretty: true,
                color: ColorMode::Auto,
            }
        );
        assert_eq!(
            parse(&args(&[
                "--doctor",
                "--format",
                "json",
                "--compact",
                "--color",
                "always",
            ]))
            .expect("compact doctor JSON"),
            Command::Doctor {
                format: QueryFormat::Json,
                pretty: false,
                color: ColorMode::Always,
            }
        );
        assert_eq!(
            parse(&args(&["--update-docs", "--compact"])).expect("document update"),
            Command::UpdateDocs { pretty: false }
        );
        assert_eq!(
            parse(&args(&["--prune-docs", "--dry-run", "--compact"]))
                .expect("document source prune"),
            Command::PruneDocs {
                pretty: false,
                dry_run: true,
            }
        );
        assert_eq!(
            parse(&args(&["--update-tldr"])).expect("update"),
            Command::UpdateTldr { pretty: true }
        );
        assert_eq!(
            parse(&args(&["--protocol-version", "--compact"])).expect("version"),
            Command::ProtocolVersion { pretty: false }
        );
        assert_eq!(
            parse(&args(&["--schema", "request", "--compact"])).expect("schema"),
            Command::Schema {
                contract: SchemaContract::Request,
                pretty: false,
            }
        );
        assert_eq!(
            parse(&args(&["--schema", "doctor"])).expect("doctor schema"),
            Command::Schema {
                contract: SchemaContract::Doctor,
                pretty: true,
            }
        );
        assert_eq!(parse(&args(&["--mcp"])).expect("MCP"), Command::Mcp);
    }

    #[test]
    fn rejects_ambiguous_or_incompatible_inputs() {
        let cases = [
            vec!["git", "--format", "json", "--format", "text"],
            vec!["git", "--compact"],
            vec!["git", "--preserve-anchors", "--format", "json"],
            vec!["git", "--outline", "--preserve-anchors"],
            vec!["git", "--search", "branch", "--preserve-anchors"],
            vec!["--request-json", "git", "--format", "json"],
            vec!["--request-json", "--man-section", "1", "--format", "json"],
            vec!["--request-json", "--outline", "--format", "json"],
            vec!["git", "--outline", "--node", "1"],
            vec!["git", "--outline", "--search", "branch"],
            vec!["git", "--node", "1", "--search", "branch"],
            vec!["git", "--explain=--help", "--node", "help"],
            vec!["git", "--explain=--help", "--outline"],
            vec!["git", "--explain=--help", "--search", "help"],
            vec!["git", "--regex"],
            vec!["git", "--search", "branch", "--limit", "many"],
            vec!["git", "--node"],
            vec!["--man-section", "1"],
            vec!["--update-tldr", "--format", "json"],
            vec!["--update-docs", "--format", "json"],
            vec!["--prune-docs", "--format", "json"],
            vec!["--doctor", "--format", "markdown"],
            vec!["--doctor", "--compact"],
            vec!["--doctor", "--source", "team"],
            vec!["--dry-run"],
            vec!["git", "--source", "team", "--man-section", "1"],
            vec!["git", "--source", "team", "--manual"],
            vec!["git", "--manual", "--tldr"],
            vec!["git", "--tldr", "--node", "0"],
            vec!["git", "--tldr", "--ui"],
            vec!["--input", "README.md", "--source", "team"],
            vec!["--schema", "request", "--format", "json"],
            vec!["--mcp", "git"],
            vec!["--mcp", "--format", "json"],
            vec!["--mcp", "--manual"],
            vec!["--mcp", "--tldr"],
            vec!["--mcp", "--update-tldr"],
            vec!["--update-tldr", "--preserve-anchors"],
            vec!["--input", "README.md", "--manual"],
            vec!["--input", "-", "--input-format", "markdown", "--manual"],
            vec!["--request-json", "--manual", "--format", "json"],
            vec!["--schema", "unknown"],
            vec!["update", "tldr"],
            vec!["git", "--json"],
            vec!["git", "--md"],
            vec!["git", "--markdown"],
            vec!["git", "--text"],
            vec!["git", "-s", "1"],
            vec!["git", "-n", "1"],
            vec!["--unknown", "git"],
        ];
        for values in cases {
            assert!(parse(&args(&values)).is_err(), "accepted {values:?}");
        }
    }

    #[test]
    fn help_is_side_effect_free_and_the_option_terminator_preserves_a_name() {
        for flag in ["--help", "-h"] {
            let help = parse(&args(&[flag])).expect("help");
            assert!(matches!(help, Command::Help(text) if text.contains("Usage: mant")));
        }
        assert_eq!(
            parse(&args(&["--", "--help"])).expect("query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V7,
                    input: QueryInput::Document {
                        selector: "--help".to_owned(),
                        source: None,
                        manual_section: None,
                    },
                    view: QueryView::Full {},
                }),
                presentation: QueryPresentation::Auto,
                pretty: true,
                policy: QueryPolicy::Combined,
                preserve_anchors: false,
            }
        );
    }

    #[test]
    fn version_is_side_effect_free() {
        let version = parse(&args(&["--version"])).expect("version");
        assert!(
            matches!(version, Command::Help(text) if text == concat!("mant ", env!("CARGO_PKG_VERSION"), "\n"))
        );
    }
}
