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
use mant_ir::is_manual_section;
use mant_ir::{EntryKind, ParameterKind};
use mant_loader::{LoadPolicy, normalize_tldr_topic, parenthesized_manual_reference};
use mant_protocol::{
    CatalogDocumentKind, CatalogQuery, ContentSelector, DocumentScope, DocumentSelector,
    DocumentTraversal, EntryProjection, InputFormat, QueryInput, QueryRequest, QueryView,
    RequestSchema, ScopeQueryView, SearchCase, SearchScope, SearchSyntax, default_search_limit,
};

mod capabilities;
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

/// Independent output choices; terminal policy resolves display and colour once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct OutputOptions {
    /// None means default text, while preserving eligibility for automatic
    /// full-document TUI reading or the specialized tldr text layout.
    pub(crate) format: Option<QueryFormat>,
    /// Fixed to Always/Never by process policy before rendering.
    pub(crate) color: ColorMode,
    /// Fixed to Direct/Pager/Tui by process policy before execution.
    pub(crate) display: DisplayMode,
}

impl OutputOptions {
    pub(crate) fn format(self) -> QueryFormat {
        self.format.unwrap_or(QueryFormat::Text)
    }
}

/// Where a result is presented, independently from its content format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum DisplayMode {
    #[default]
    Auto,
    Direct,
    #[cfg_attr(not(feature = "pager"), value(skip))]
    Pager,
    #[cfg_attr(not(feature = "tui"), value(skip))]
    Tui,
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
    Explanation,
    Search,
    ScopeRequest,
    ScopeQuery,
    Catalog,
    All,
}

/// Semantic entries included beneath the ordinary section outline.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OutlineEntries(EntryProjection);

fn parse_reference_mode(value: &str) -> Result<mant_protocol::ReferenceProjectionMode, String> {
    match value {
        "none" => Ok(mant_protocol::ReferenceProjectionMode::None),
        "summary" => Ok(mant_protocol::ReferenceProjectionMode::Summary),
        "all" => Ok(mant_protocol::ReferenceProjectionMode::All),
        _ => Err("expected none, summary, or all".to_owned()),
    }
}

fn parse_reference_type(value: &str) -> Result<mant_protocol::ReferenceTargetType, String> {
    match value {
        "document" => Ok(mant_protocol::ReferenceTargetType::Document),
        "manual" => Ok(mant_protocol::ReferenceTargetType::Manual),
        "local" => Ok(mant_protocol::ReferenceTargetType::Local),
        "external" => Ok(mant_protocol::ReferenceTargetType::External),
        "email" => Ok(mant_protocol::ReferenceTargetType::Email),
        _ => Err("expected document, manual, local, external, or email".to_owned()),
    }
}

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
    #[cfg(feature = "roff")]
    Roff,
}

impl From<InputFormatMode> for InputFormat {
    fn from(value: InputFormatMode) -> Self {
        match value {
            InputFormatMode::Auto => Self::Auto,
            InputFormatMode::Markdown => Self::Markdown,
            #[cfg(feature = "roff")]
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
        presentation: OutputOptions,
        pretty: bool,
        policy: LoadPolicy,
        preserve_anchors: bool,
    },
    Catalog {
        query: CatalogQuery,
        grouped: bool,
        presentation: OutputOptions,
        pretty: bool,
    },
    Doctor {
        presentation: OutputOptions,
        pretty: bool,
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

#[cfg(test)]
const CLI_USAGE: &str = "mant <SELECTOR> [OPTIONS]\n       mant <MAN_SECTION> <NAME> [OPTIONS]\n       mant --document <SELECTOR>... [--follow-links] [OPTIONS]\n       mant --input <PATH|-> [--input-format <FORMAT>] [OPTIONS]\n       mant --list [FILTERS]\n       mant --find <PATTERN> [FILTERS]\n       mant --request-json [--format <FORMAT>] [--compact]\n       mant --doctor [--format <text|json>] [--compact]\n       mant --schema <CONTRACT> [--compact]\n       mant --update-docs [--compact]\n       mant --prune-docs [--dry-run] [--compact]\n       mant --update-tldr [--compact]\n       mant --protocol-version [--compact]\n       mant --mcp";

mod help;

#[derive(Debug, clap::Parser)]
// These booleans are declarative CLI switches, not coupled domain state; clap
// validates their relationships before `Cli` is normalized into `Command`.
#[allow(clippy::struct_excessive_bools)]
#[command(
    name = "mant",
    about = "Read or query structured local manuals and Markdown",
    help_template = "{about-with-newline}\n{usage-heading} {usage}\n\n{all-args}{after-help}",
    styles = CLI_STYLES,
    disable_help_flag = true,
    version,
    override_usage = capabilities::usage(),
    after_help = help::footer(),
    group = ArgGroup::new("action")
        .args(capabilities::argument_ids(["selector", "document", "input", "list", "find", "request_json", "doctor", "update_docs", "prune_docs", "update_tldr", "protocol_version", "schema", "mcp"]))
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
    #[cfg_attr(
        not(feature = "roff"),
        arg(help = "Read one explicit Markdown file; use `-` for standard input.")
    )]
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
    #[cfg_attr(feature = "roff", arg(
        long = "man-section",
        value_name = "MAN_SECTION",
        value_parser = non_empty,
        conflicts_with = "input",
        help_heading = "Document selection"
    ))]
    #[cfg_attr(not(feature = "roff"), arg(skip))]
    man_section: Option<String>,

    /// Select exactly one configured Markdown source.
    #[arg(
        long,
        value_name = "SOURCE",
        value_parser = non_empty,
        conflicts_with_all = capabilities::argument_ids(["man_section", "manual", "input"]),
        help_heading = "Document selection"
    )]
    source: Option<String>,

    /// Print only a native manual, bypassing Markdown and tldr content.
    #[cfg_attr(feature = "roff", arg(
        long,
        requires = "selector",
        conflicts_with_all = capabilities::argument_ids(["tldr", "input"]),
        help_heading = "Document selection"
    ))]
    #[cfg_attr(not(feature = "roff"), arg(skip))]
    manual: bool,

    /// Print only the available tldr quick reference.
    #[arg(
        long,
        requires = "selector",
        conflicts_with_all = capabilities::argument_ids(["manual", "outline", "node", "explain", "search", "input"]),
        help_heading = "Document selection"
    )]
    tldr: bool,

    /// Print the addressable outline tree with compact semantic summaries.
    #[arg(
        long,
        conflicts_with_all = capabilities::argument_ids(["node", "explain"]),
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

    /// Discover references independently of entries: none, summary (default), or all.
    #[arg(long = "outline-references", value_name = "MODE", requires = "outline", value_parser = parse_reference_mode, help_heading = "Document selection")]
    outline_references: Option<mant_protocol::ReferenceProjectionMode>,

    /// Select reference kinds: document,manual (default),local,external,email.
    #[arg(long = "reference-types", value_name = "KINDS", requires = "outline", value_delimiter = ',', value_parser = parse_reference_type, help_heading = "Document selection")]
    reference_types: Vec<mant_protocol::ReferenceTargetType>,

    /// Skip selected link occurrences before the reference page; work remains bounded.
    #[arg(
        long = "reference-offset",
        value_name = "N",
        requires = "outline",
        help_heading = "Document selection"
    )]
    reference_offset: Option<u32>,

    /// Maximum reference records with --outline-references all (default 100, maximum 1000).
    #[arg(long = "reference-limit", value_name = "N", requires = "outline", value_parser = clap::value_parser!(u32).range(1..=1000), help_heading = "Document selection")]
    reference_limit: Option<u32>,

    /// Start at an exact local node: path:1.2/e3, id:node-id, or a bare canonical path.
    #[arg(
        long,
        value_name = "SELECTOR",
        allow_hyphen_values = true,
        requires = "outline",
        help_heading = "Document selection"
    )]
    outline_root: Option<ContentSelector>,

    /// Read exact local content by path:1.2/e3, id:node-id, or a bare canonical path; repeatable.
    #[arg(
        long,
        value_name = "SELECTOR",
        conflicts_with = "explain",
        help_heading = "Document selection"
    )]
    node: Vec<ContentSelector>,

    /// Collect definitions, declared relations, then literal mentions; use --node for full content.
    #[arg(
        long,
        value_name = "ENTRY",
        value_parser = non_empty,
        allow_hyphen_values = true,
        conflicts_with_all = capabilities::argument_ids(["outline", "node", "search"]),
        help_heading = "Document selection"
    )]
    explain: Option<String>,

    /// Bound copied facts/previews/body bytes (default 1 MiB, maximum 4 MiB).
    #[arg(
        long,
        value_name = "BYTES",
        requires = "explain",
        help_heading = "Document selection"
    )]
    explain_content_bytes: Option<u32>,

    /// Search visible document text and report Markdown lines plus outline nodes.
    #[arg(
        long,
        visible_alias = "grep",
        value_name = "PATTERN",
        value_parser = non_empty,
        conflicts_with_all = capabilities::argument_ids(["outline", "node", "explain"]),
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

    /// Return at most this many search lines, catalog rows, or explanation owners.
    #[arg(long, value_name = "COUNT", help_heading = "Search")]
    limit: Option<u32>,

    /// Skip this many search lines, catalog rows, or explanation owners.
    #[arg(long, value_name = "COUNT", help_heading = "Search")]
    offset: Option<u32>,

    /// Read a versioned single- or multi-document request JSON object from standard input.
    #[arg(
        long,
        conflicts_with_all = capabilities::argument_ids([
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
        ]),
        help_heading = "Integration"
    )]
    request_json: bool,

    /// Diagnose local paths, sources, manuals, and tldr caches without changing them.
    #[arg(
        long,
        conflicts_with_all = capabilities::argument_ids([
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
            "dry_run",
            "preserve_anchors"
        ]),
        help_heading = "Diagnostics"
    )]
    doctor: bool,

    /// Update tldr data through the installed client or `ManT` cache.
    #[cfg_attr(feature = "update", arg(
        long,
        conflicts_with_all = capabilities::argument_ids(["man_section", "outline", "node", "search", "format"]),
        help_heading = "Data"
    ))]
    #[cfg_attr(not(feature = "update"), arg(skip))]
    update_tldr: bool,

    /// Update configured Markdown repositories from sources.toml.
    #[cfg_attr(feature = "update", arg(
        long,
        conflicts_with_all = capabilities::argument_ids(["man_section", "source", "outline", "node", "search", "format"]),
        help_heading = "Data"
    ))]
    #[cfg_attr(not(feature = "update"), arg(skip))]
    update_docs: bool,

    /// Remove installed document sources absent from sources.toml.
    #[cfg_attr(feature = "update", arg(
        long,
        conflicts_with_all = capabilities::argument_ids(["man_section", "source", "outline", "node", "search", "format"]),
        help_heading = "Data"
    ))]
    #[cfg_attr(not(feature = "update"), arg(skip))]
    prune_docs: bool,

    /// Report exact orphaned source targets without removing them.
    #[cfg_attr(
        feature = "update",
        arg(long, requires = "prune_docs", help_heading = "Data")
    )]
    #[cfg_attr(not(feature = "update"), arg(skip))]
    dry_run: bool,

    /// Print the native protocol description as JSON.
    #[arg(
        long,
        conflicts_with_all = capabilities::argument_ids(["man_section", "outline", "node", "search", "format"]),
        help_heading = "Integration"
    )]
    protocol_version: bool,

    /// Print a generated JSON Schema contract, including single- or multi-document requests and results.
    #[arg(
        long,
        value_name = "CONTRACT",
        value_enum,
        conflicts_with_all = capabilities::argument_ids(["man_section", "outline", "node", "search", "format"]),
        help_heading = "Integration"
    )]
    schema: Option<SchemaContract>,

    /// Serve read-only manual queries through the MCP stdio transport.
    #[cfg_attr(feature = "mcp", arg(
        long,
        conflicts_with_all = capabilities::argument_ids([
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
        ]),
        help_heading = "Integration"
    ))]
    #[cfg_attr(not(feature = "mcp"), arg(skip))]
    mcp: bool,

    /// Output format. Document queries default to text; interactive full reading opens the TUI.
    #[arg(long, value_name = "FORMAT", value_enum, help_heading = "Output")]
    #[cfg_attr(
        not(feature = "tui"),
        arg(help = "Output format. Document queries default to text.")
    )]
    format: Option<QueryFormat>,

    /// Control colors in human-readable terminal output.
    #[arg(long, value_enum, help_heading = "Output")]
    color: Option<ColorMode>,

    /// Omit JSON indentation. Query output also requires `--format json`.
    #[arg(long, help_heading = "Output")]
    compact: bool,

    /// Choose automatic presentation, direct printing, a pager, or the document TUI.
    #[arg(long, value_enum, help_heading = "Output")]
    #[cfg_attr(
        not(all(feature = "tui", feature = "pager")),
        arg(help = "Choose automatic presentation or an available explicit display mode.")
    )]
    display: Option<DisplayMode>,

    /// Preserve raw HTML anchors and document-local links in Markdown output.
    #[arg(
        long,
        conflicts_with_all = capabilities::argument_ids(["update_docs", "prune_docs", "update_tldr", "protocol_version", "schema", "mcp"]),
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
    if arguments.is_empty() {
        let mut command = Cli::command().color(color.into());
        // Preserve the usage-error status/stream without clap appending a hint
        // after the manual footer. Full option help remains an explicit action.
        return Err(clap::Error::raw(
            ErrorKind::MissingRequiredArgument,
            format!(
                "choose a document or action; use 'mant --help' for all options\n\n{}\n\n{}",
                command.render_usage().ansi(),
                help::footer().ansi(),
            ),
        )
        .with_cmd(&command));
    }
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
        Err(error) => {
            if error.kind() == ErrorKind::UnknownArgument
                && let Some(clap::error::ContextValue::String(old)) =
                    error.get(clap::error::ContextKind::InvalidArg)
                && matches!(old.as_str(), "--ui" | "--no-pager")
            {
                let replacement = if old == "--ui" { "tui" } else { "direct" };
                return Err(command_error(
                    ErrorKind::UnknownArgument,
                    format!("{old} was removed; use --display {replacement}"),
                    color,
                ));
            }
            return Err(error);
        }
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
