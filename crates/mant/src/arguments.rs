//! Defines and validates the public `mant` command line with clap.
//!
//! The interface intentionally has one positional value: the document name.
//! Every action, projection, input mode, and output choice is a long option so
//! humans and agents do not have to distinguish ad-hoc subcommand grammars.

use std::{iter, str::FromStr};

use clap::{
    ArgAction, ArgGroup, CommandFactory, FromArgMatches, ValueEnum,
    builder::styling::{AnsiColor, Styles},
    error::ErrorKind,
};
use mant_engine::{
    QueryPolicy, is_manual_section, normalize_tldr_topic, parenthesized_manual_reference,
};
use mant_ir::{EntryKind, ParameterKind};
use mant_protocol::{
    CatalogDocumentKind, CatalogQuery, DocumentScope, DocumentSelector, DocumentTraversal,
    EntryProjection, InputFormat, NodeSelector, QueryInput, QueryRequest, QueryView, RequestSchema,
    ScopeQueryView, SearchCase, SearchScope, SearchSyntax, default_search_limit,
};

mod normalize;

use normalize::{command_error, non_empty, normalize};

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
    TldrUpdate,
    Request,
    Query,
    Outline,
    Excerpt,
    Search,
    ScopeRequest,
    ScopeQuery,
    Catalog,
    All,
}

/// Semantic entries included beneath the ordinary section outline.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OutlineEntries(EntryProjection);

impl FromStr for OutlineEntries {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => return Ok(Self(EntryProjection::None)),
            "summary" => return Ok(Self(EntryProjection::Summary)),
            "all" => return Ok(Self(EntryProjection::All)),
            _ => {}
        }
        let mut kinds = Vec::new();
        for name in value.split(',') {
            let kind = match name {
                "command" => EntryKind::Command,
                "option" => EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                "marker" => EntryKind::Parameter {
                    parameter_kind: ParameterKind::Marker,
                },
                "operand" => EntryKind::Parameter {
                    parameter_kind: ParameterKind::Operand,
                },
                "configuration-key" => EntryKind::ConfigurationKey,
                "environment-variable" => EntryKind::EnvironmentVariable,
                "variable" => EntryKind::Variable,
                "value" => EntryKind::Value,
                "term" => EntryKind::Term,
                _ => {
                    return Err(format!(
                        "unknown entry kind '{name}'; use none, summary, all, or a comma-separated list of command, option, marker, operand, configuration-key, environment-variable, variable, value, and term"
                    ));
                }
            };
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        if kinds.is_empty() {
            return Err("entry kind list must not be empty".to_owned());
        }
        Ok(Self(EntryProjection::Kinds { kinds }))
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
    ScopeArguments {
        scope: DocumentScope,
        view: Option<ScopeQueryView>,
    },
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
    override_usage = "mant <SELECTOR> [OPTIONS]\n       mant <MAN_SECTION> <NAME> [OPTIONS]\n       mant --document <SELECTOR>... [--follow-links] [OPTIONS]\n       mant --input <PATH|-> [--input-format <FORMAT>] [OPTIONS]\n       mant --list [FILTERS]\n       mant --find <PATTERN> [FILTERS]\n       mant --request-json [--format <FORMAT>] [--compact]\n       mant --doctor [--format <text|json>] [--compact]\n       mant --schema <CONTRACT> [--compact]\n       mant --update-docs [--compact]\n       mant --prune-docs [--dry-run] [--compact]\n       mant --update-tldr [--compact]\n       mant --protocol-version [--compact]\n       mant --mcp",
    after_help = "Examples:\n  mant git\n  mant 1 git\n  mant 'git(1)'\n  mant manual/1/git\n  mant git --search worktree --follow-links\n  mant --document git --document git-lfs --explain=--work-tree\n  mant --input README.md\n  mant --input /usr/share/man/man1/git.1.gz\n  cat guide.md | mant --input - --input-format markdown\n  mant --list\n  mant --find process --source pwsh7\n  mant git --tldr\n  mant 1 tar --tldr\n  mant gcc --outline\n  mant tar --explain=--exclude\n  mant git --format json --compact\n  mant --doctor\n  mant --update-docs\n  mant --mcp",
    group = ArgGroup::new("action")
        .args(["selector", "document", "input", "list", "find", "request_json", "doctor", "update_docs", "prune_docs", "update_tldr", "protocol_version", "schema", "mcp"])
        .required(true)
        .multiple(false)
)]
struct Cli {
    /// Document selector, or a man-style `MAN_SECTION NAME` pair.
    #[arg(value_name = "SELECTOR", value_parser = non_empty, num_args = 0..)]
    selector: Vec<String>,

    /// Add one initial document to a bounded multi-document query; repeatable.
    #[arg(
        long,
        value_name = "SELECTOR",
        value_parser = non_empty,
        action = ArgAction::Append,
        help_heading = "Document scope"
    )]
    document: Vec<String>,

    /// Follow typed links between registered Markdown and native manuals.
    #[arg(long, help_heading = "Document scope")]
    follow_links: bool,

    /// Follow at most this many document-link edges from an initial document.
    #[arg(
        long,
        value_name = "DEPTH",
        requires = "follow_links",
        help_heading = "Document scope"
    )]
    max_depth: Option<u16>,

    /// Load at most this many distinct documents, including initial documents.
    #[arg(
        long,
        value_name = "COUNT",
        requires = "follow_links",
        help_heading = "Document scope"
    )]
    max_documents: Option<u32>,

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

    /// Print the addressable outline tree with compact semantic summaries.
    #[arg(
        long,
        conflicts_with_all = ["node", "explain"],
        help_heading = "Document selection"
    )]
    outline: bool,

    /// Select semantic entry expansion: none, summary, all, or comma-separated kinds.
    #[arg(
        long,
        value_name = "MODE|KINDS",
        requires = "outline",
        help_heading = "Document selection"
    )]
    outline_entries: Option<OutlineEntries>,

    /// Start the outline at one section or semantic entry selector.
    #[arg(
        long,
        value_name = "SELECTOR",
        value_parser = non_empty,
        allow_hyphen_values = true,
        requires = "outline",
        help_heading = "Document selection"
    )]
    outline_root: Option<String>,

    /// Print an outline node selected by path, stable ID, or semantic-entry alias; repeatable.
    #[arg(
        long,
        value_name = "SELECTOR",
        value_parser = non_empty,
        conflicts_with = "explain",
        help_heading = "Document selection"
    )]
    node: Vec<String>,

    /// Explain one semantic entry by alias, ID, or outline path.
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

    /// Return at most this many matching lines.
    #[arg(long, value_name = "COUNT", help_heading = "Search")]
    limit: Option<u32>,

    /// Skip this many matching lines for deterministic pagination.
    #[arg(long, value_name = "COUNT", help_heading = "Search")]
    offset: Option<u32>,

    /// Read a versioned single- or multi-document request JSON object from standard input.
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

    /// Print a generated JSON Schema contract, including single- or multi-document requests and results.
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

#[cfg(test)]
mod tests;
