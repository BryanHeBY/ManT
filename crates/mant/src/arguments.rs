//! Defines and validates the public `mant` command line with clap.
//!
//! The interface intentionally has one positional value: the document name.
//! Every action, projection, input mode, and output choice is a long option so
//! humans and agents do not have to distinguish ad-hoc subcommand grammars.

use std::{iter, path::Path};

use clap::{ArgAction, ArgGroup, CommandFactory, Parser, ValueEnum, error::ErrorKind};
use mant_ast::{
    OutlineDetail, QueryInput, QueryRequest, QueryView, RequestSchema, SearchCase, SearchScope,
    SearchSyntax, default_search_limit,
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

/// How a complete native query is presented to its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryPresentation {
    /// Use the interactive reader when the process owns a terminal, otherwise
    /// retain the conventional Markdown output.
    Auto,
    /// Require the Ratatui reader and a usable terminal.
    Interactive,
    /// Render a deterministic representation to standard output.
    Output(QueryFormat),
}

/// A discoverable JSON Schema exposed by the native process boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum SchemaContract {
    Request,
    Query,
    Outline,
    Excerpt,
    Search,
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

/// Case policy exposed without coupling the AST crate to clap.
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
    MarkdownStdin { view: QueryView },
}

/// One validated invocation of the native CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Help(String),
    Query {
        source: QuerySource,
        presentation: QueryPresentation,
        pretty: bool,
        manual_only: bool,
        preserve_anchors: bool,
    },
    UpdateTldr {
        pretty: bool,
    },
    UpdateDocs {
        pretty: bool,
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

#[derive(Debug, Parser)]
// These booleans are declarative CLI switches, not coupled domain state; clap
// validates their relationships before `Cli` is normalized into `Command`.
#[allow(clippy::struct_excessive_bools)]
#[command(
    name = "mant",
    about = "Read or query structured local manuals and Markdown",
    disable_help_flag = true,
    version,
    override_usage = "mant <NAME|MARKDOWN|-> [OPTIONS]\n       mant --request-json [--format <FORMAT>] [--compact]\n       mant --schema <CONTRACT> [--compact]\n       mant --update-docs [--compact]\n       mant --update-tldr [--compact]\n       mant --protocol-version [--compact]\n       mant --mcp",
    after_help = "Examples:\n  mant git\n  mant README.md\n  mant tool --source team\n  mant printf --manual\n  mant printf --section 3\n  mant git --format markdown\n  cat guide.md | mant -\n  mant gcc --outline\n  mant tar --explain=--exclude\n  mant tar --node acls --format markdown\n  mant tar --search=--acls --context 1\n  mant git --format json --compact\n  mant --schema request\n  mant --update-docs\n  mant --update-tldr\n  mant --mcp",
    group = ArgGroup::new("action")
        .args(["name", "request_json", "update_docs", "update_tldr", "protocol_version", "schema", "mcp"])
        .required(true)
        .multiple(false)
)]
struct Cli {
    /// Document name, local Markdown path, or `-` for standard input.
    #[arg(value_name = "NAME|MARKDOWN|-", value_parser = non_empty)]
    name: Option<String>,

    /// Select a manual section such as 1 or 3p.
    #[arg(
        long,
        value_name = "SECTION",
        value_parser = non_empty,
        requires = "name",
        help_heading = "Document selection"
    )]
    section: Option<String>,

    /// Select exactly one configured Markdown source.
    #[arg(
        long,
        value_name = "SOURCE",
        value_parser = non_empty,
        requires = "name",
        conflicts_with_all = ["section", "manual"],
        help_heading = "Document selection"
    )]
    source: Option<String>,

    /// Bypass registered Markdown and require a native manual page.
    #[arg(long, requires = "name", help_heading = "Document selection")]
    manual: bool,

    /// Print selectable sections and semantic entries by default.
    #[arg(
        long,
        value_name = "DETAIL",
        value_enum,
        num_args = 0..=1,
        default_missing_value = "entries",
        requires = "name",
        conflicts_with_all = ["node", "explain"],
        help_heading = "Document selection"
    )]
    outline: Option<OutlineMode>,

    /// Print a node by outline path, document ID, or option alias; repeatable.
    #[arg(
        long,
        value_name = "NODE",
        value_parser = non_empty,
        requires = "name",
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
        requires = "name",
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
        requires = "name",
        conflicts_with_all = ["outline", "node", "explain"],
        help_heading = "Search"
    )]
    search: Option<String>,

    /// Interpret the search pattern as a regular expression instead of a literal.
    #[arg(long, requires = "search", help_heading = "Search")]
    regex: bool,

    /// Select case handling for search matches.
    #[arg(
        long = "case",
        value_name = "POLICY",
        value_enum,
        requires = "search",
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
    #[arg(
        long,
        value_name = "COUNT",
        requires = "search",
        help_heading = "Search"
    )]
    limit: Option<u32>,

    /// Skip this many matches for deterministic pagination.
    #[arg(
        long,
        value_name = "COUNT",
        requires = "search",
        help_heading = "Search"
    )]
    offset: Option<u32>,

    /// Read a versioned `QueryRequest` JSON object from standard input.
    #[arg(
        long,
        conflicts_with_all = [
            "section",
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
        requires = "name",
        conflicts_with_all = [
            "outline",
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

    /// Update tldr data through the installed client or `ManT` cache.
    #[arg(
        long,
        conflicts_with_all = ["section", "outline", "node", "search", "format"],
        help_heading = "Data"
    )]
    update_tldr: bool,

    /// Update configured Markdown repositories from sources.toml.
    #[arg(
        long,
        conflicts_with_all = ["section", "source", "outline", "node", "search", "format"],
        help_heading = "Data"
    )]
    update_docs: bool,

    /// Print the native protocol description as JSON.
    #[arg(
        long,
        conflicts_with_all = ["section", "outline", "node", "search", "format"],
        help_heading = "Integration"
    )]
    protocol_version: bool,

    /// Print a generated JSON Schema contract (`request`, `query`, `outline`, `excerpt`, `search`, or `all`).
    #[arg(
        long,
        value_name = "CONTRACT",
        value_enum,
        conflicts_with_all = ["section", "outline", "node", "search", "format"],
        help_heading = "Integration"
    )]
    schema: Option<SchemaContract>,

    /// Serve read-only manual queries through the MCP stdio transport.
    #[arg(
        long,
        conflicts_with_all = [
            "name",
            "section",
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
            "update_tldr",
            "update_docs",
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

    /// Omit JSON indentation. Query output also requires `--format json`.
    #[arg(long, help_heading = "Output")]
    compact: bool,

    /// Preserve raw HTML anchors and document-local links in Markdown output.
    #[arg(
        long,
        conflicts_with_all = ["update_docs", "update_tldr", "protocol_version", "schema", "mcp"],
        help_heading = "Output"
    )]
    preserve_anchors: bool,

    /// Print help.
    #[arg(short = 'h', long, action = ArgAction::Help, help_heading = "General")]
    help: Option<bool>,
}

// ── Normalization and semantic validation ─────────────────────────────────

pub(crate) fn parse(arguments: &[String]) -> Result<Command, clap::Error> {
    let parsed =
        match Cli::try_parse_from(iter::once("mant").chain(arguments.iter().map(String::as_str))) {
            Ok(parsed) => parsed,
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                ) =>
            {
                return Ok(Command::Help(error.to_string()));
            }
            Err(error) => return Err(error),
        };

    normalize(parsed)
}

fn normalize(mut parsed: Cli) -> Result<Command, clap::Error> {
    if parsed.mcp {
        return Ok(Command::Mcp);
    }
    if parsed.update_docs {
        return Ok(Command::UpdateDocs {
            pretty: !parsed.compact,
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

    let view = normalize_query_view(&mut parsed);
    validate_output_options(
        parsed.compact,
        parsed.format,
        parsed.preserve_anchors,
        &view,
    )?;
    let source = normalize_query_source(
        parsed.request_json,
        parsed.name,
        parsed.source,
        parsed.section,
        view,
    )?;
    validate_manual_source(parsed.manual, &source)?;
    let presentation =
        normalize_presentation(parsed.ui, parsed.format, parsed.preserve_anchors, &source);

    Ok(Command::Query {
        source,
        presentation,
        pretty: !parsed.compact,
        manual_only: parsed.manual,
        preserve_anchors: parsed.preserve_anchors,
    })
}

fn normalize_query_view(parsed: &mut Cli) -> QueryView {
    if let Some(detail) = parsed.outline.take() {
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
            nodes: std::mem::take(&mut parsed.node),
        }
    }
}

fn validate_output_options(
    compact: bool,
    format: Option<QueryFormat>,
    preserve_anchors: bool,
    view: &QueryView,
) -> Result<(), clap::Error> {
    if compact && format != Some(QueryFormat::Json) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--compact requires --format json for manual queries",
        ));
    }
    if preserve_anchors && format.is_some_and(|format| format != QueryFormat::Markdown) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--preserve-anchors requires Markdown output",
        ));
    }
    if preserve_anchors && matches!(view, QueryView::Outline { .. } | QueryView::Search { .. }) {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--preserve-anchors applies only to full documents and excerpts",
        ));
    }
    Ok(())
}

fn validate_manual_source(manual: bool, source: &QuerySource) -> Result<(), clap::Error> {
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
        ));
    }
    Ok(())
}

fn normalize_presentation(
    ui: bool,
    format: Option<QueryFormat>,
    preserve_anchors: bool,
    source: &QuerySource,
) -> QueryPresentation {
    let view = match source {
        QuerySource::Arguments(request) => Some(&request.view),
        QuerySource::MarkdownStdin { view } => Some(view),
        QuerySource::StdinJson => None,
    };
    let default_format = if view
        .is_some_and(|view| matches!(view, QueryView::Outline { .. } | QueryView::Search { .. }))
    {
        QueryFormat::Text
    } else {
        QueryFormat::Markdown
    };
    if ui {
        QueryPresentation::Interactive
    } else if let Some(format) = format {
        QueryPresentation::Output(format)
    } else if preserve_anchors {
        QueryPresentation::Output(QueryFormat::Markdown)
    } else if matches!(
        source,
        QuerySource::Arguments(QueryRequest {
            view: QueryView::Full {},
            ..
        })
    ) {
        QueryPresentation::Auto
    } else {
        QueryPresentation::Output(default_format)
    }
}

fn normalize_query_source(
    request_json: bool,
    name: Option<String>,
    configured_source: Option<String>,
    section: Option<String>,
    view: QueryView,
) -> Result<QuerySource, clap::Error> {
    let source = if request_json {
        QuerySource::StdinJson
    } else {
        let value = name.expect("clap requires one input source");
        if value == "-" {
            if section.is_some() {
                return Err(command_error(
                    ErrorKind::ArgumentConflict,
                    "--section applies only to document names",
                ));
            }
            if configured_source.is_some() {
                return Err(command_error(
                    ErrorKind::ArgumentConflict,
                    "--source applies only to document names",
                ));
            }
            QuerySource::MarkdownStdin { view }
        } else {
            let input = if is_markdown_path(&value) {
                if section.is_some() {
                    return Err(command_error(
                        ErrorKind::ArgumentConflict,
                        "--section applies only to document names",
                    ));
                }
                if configured_source.is_some() {
                    return Err(command_error(
                        ErrorKind::ArgumentConflict,
                        "--source applies only to document names",
                    ));
                }
                QueryInput::MarkdownFile { path: value }
            } else {
                QueryInput::Document {
                    name: value,
                    source: configured_source,
                    section,
                }
            };
            QuerySource::Arguments(QueryRequest {
                schema: RequestSchema::V6,
                input,
                view,
            })
        }
    };
    Ok(source)
}

fn is_markdown_path(value: &str) -> bool {
    let markdown_extension = Path::new(value)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        });
    markdown_extension
        || value.starts_with('.')
        || value.contains('/')
        || value.contains('\\')
        || Path::new(value).is_absolute()
}

fn non_empty(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err("value must not be empty".to_owned())
    } else {
        Ok(value.to_owned())
    }
}

fn command_error(kind: ErrorKind, message: impl std::fmt::Display) -> clap::Error {
    Cli::command().error(kind, message)
}

#[cfg(test)]
mod tests {
    use mant_ast::{
        OutlineDetail, QueryInput, QueryRequest, QueryView, RequestSchema, SearchCase, SearchScope,
        SearchSyntax,
    };

    use super::{Command, QueryFormat, QueryPresentation, QuerySource, SchemaContract, parse};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn defaults_direct_queries_to_markdown() {
        assert_eq!(
            parse(&args(&["git"])).expect("query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V6,
                    input: QueryInput::Document {
                        name: "git".to_owned(),
                        source: None,
                        section: None,
                    },
                    view: QueryView::Full {},
                }),
                presentation: QueryPresentation::Auto,
                pretty: true,
                manual_only: false,
                preserve_anchors: false,
            }
        );
    }

    #[test]
    fn parses_an_explicit_interactive_query_without_an_output_projection() {
        assert!(matches!(
            parse(&args(&["git", "--ui"])).expect("interactive query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    input: QueryInput::Document { ref name, .. },
                    view: QueryView::Full {},
                    ..
                }),
                presentation: QueryPresentation::Interactive,
                ..
            } if name == "git"
        ));

        for conflicting in ["--outline", "--search=git", "--format=json"] {
            assert!(
                parse(&args(&["git", "--ui", conflicting])).is_err(),
                "accepted {conflicting}"
            );
        }
    }

    #[test]
    fn dispatches_markdown_files_and_direct_stdin_without_embedding_content() {
        for path in ["README.md", "docs/guide", "./notes"] {
            assert!(matches!(
                parse(&args(&[path])).expect("Markdown file query"),
                Command::Query {
                    source: QuerySource::Arguments(QueryRequest {
                        input: QueryInput::MarkdownFile { path: parsed },
                        ..
                    }),
                    ..
                } if parsed == path
            ));
        }

        assert!(matches!(
            parse(&args(&["-", "--outline"])).expect("piped Markdown outline"),
            Command::Query {
                source: QuerySource::MarkdownStdin {
                    view: QueryView::Outline {
                        detail: OutlineDetail::Entries
                    }
                },
                presentation: QueryPresentation::Output(QueryFormat::Text),
                ..
            }
        ));
        assert!(
            parse(&args(&["README.md", "--section", "1"]))
                .expect_err("Markdown has no man section selector")
                .to_string()
                .contains("--section applies only to document names")
        );
    }

    #[test]
    fn preserves_markdown_anchors_only_when_requested() {
        assert!(matches!(
            parse(&args(&["git", "--preserve-anchors"])).expect("addressable Markdown"),
            Command::Query {
                presentation: QueryPresentation::Output(QueryFormat::Markdown),
                preserve_anchors: true,
                ..
            }
        ));
    }

    #[test]
    fn parses_format_section_and_compact_json_options() {
        assert_eq!(
            parse(&args(&[
                "printf",
                "--section",
                "3",
                "--format",
                "json",
                "--compact",
            ]))
            .expect("query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V6,
                    input: QueryInput::Document {
                        name: "printf".to_owned(),
                        source: None,
                        section: Some("3".to_owned()),
                    },
                    view: QueryView::Full {},
                }),
                presentation: QueryPresentation::Output(QueryFormat::Json),
                pretty: false,
                manual_only: false,
                preserve_anchors: false,
            }
        );
        assert!(matches!(
            parse(&args(&["printf", "--source", "team"])).expect("source query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    input: QueryInput::Document {
                        ref source,
                        section: None,
                        ..
                    },
                    ..
                }),
                ..
            } if source.as_deref() == Some("team")
        ));
    }

    #[test]
    fn parses_the_closed_stdin_request_mode_used_by_the_tui() {
        assert_eq!(
            parse(&args(&["--request-json", "--format", "json", "--compact",]))
                .expect("stdin query"),
            Command::Query {
                source: QuerySource::StdinJson,
                presentation: QueryPresentation::Output(QueryFormat::Json),
                pretty: false,
                manual_only: false,
                preserve_anchors: false,
            }
        );
    }

    #[test]
    fn parses_the_explicit_manual_source_policy() {
        assert!(matches!(
            parse(&args(&["tar", "--manual", "--format", "json"])).expect("manual-only query"),
            Command::Query {
                manual_only: true,
                ..
            }
        ));
    }

    #[test]
    fn parses_outline_and_repeatable_node_views_with_contextual_defaults() {
        assert_eq!(
            parse(&args(&["gcc", "--outline"])).expect("outline"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V6,
                    input: QueryInput::Document {
                        name: "gcc".to_owned(),
                        source: None,
                        section: None,
                    },
                    view: QueryView::Outline {
                        detail: OutlineDetail::Entries,
                    },
                }),
                presentation: QueryPresentation::Output(QueryFormat::Text),
                pretty: true,
                manual_only: false,
                preserve_anchors: false,
            }
        );
        assert_eq!(
            parse(&args(&["tar", "--outline", "options", "--format", "json"]))
                .expect("option outline"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V6,
                    input: QueryInput::Document {
                        name: "tar".to_owned(),
                        source: None,
                        section: None,
                    },
                    view: QueryView::Outline {
                        detail: OutlineDetail::Entries,
                    },
                }),
                presentation: QueryPresentation::Output(QueryFormat::Json),
                pretty: true,
                manual_only: false,
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
                    schema: RequestSchema::V6,
                    input: QueryInput::Document {
                        name: "gcc".to_owned(),
                        source: None,
                        section: None,
                    },
                    view: QueryView::Excerpt {
                        nodes: vec!["4.2".to_owned(), "files-8".to_owned()],
                    },
                }),
                presentation: QueryPresentation::Output(QueryFormat::Text),
                pretty: true,
                manual_only: false,
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
                        schema: RequestSchema::V6,
                        input: QueryInput::Document {
                            name: "tar".to_owned(),
                            source: None,
                            section: None,
                        },
                        view: QueryView::Explain {
                            entry: selector.to_owned(),
                        },
                    }),
                    presentation: QueryPresentation::Output(QueryFormat::Markdown),
                    pretty: true,
                    manual_only: false,
                    preserve_anchors: false,
                }
            );
        }
    }

    #[test]
    fn parses_literal_and_regex_searches_with_text_as_the_default() {
        assert_eq!(
            parse(&args(&["tar", "--search=--acls"])).expect("literal search"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V6,
                    input: QueryInput::Document {
                        name: "tar".to_owned(),
                        source: None,
                        section: None,
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
                presentation: QueryPresentation::Output(QueryFormat::Text),
                pretty: true,
                manual_only: false,
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
                    schema: RequestSchema::V6,
                    input: QueryInput::Document {
                        name: "git".to_owned(),
                        source: None,
                        section: None,
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
                presentation: QueryPresentation::Output(QueryFormat::Json),
                pretty: true,
                manual_only: false,
                preserve_anchors: false,
            }
        );
    }

    #[test]
    fn parses_long_option_actions_without_ad_hoc_subcommands() {
        assert_eq!(
            parse(&args(&["--update-docs", "--compact"])).expect("document update"),
            Command::UpdateDocs { pretty: false }
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
            vec!["--request-json", "--section", "1", "--format", "json"],
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
            vec!["--section", "1"],
            vec!["--update-tldr", "--format", "json"],
            vec!["--update-docs", "--format", "json"],
            vec!["git", "--source", "team", "--section", "1"],
            vec!["git", "--source", "team", "--manual"],
            vec!["README.md", "--source", "team"],
            vec!["--schema", "request", "--format", "json"],
            vec!["--mcp", "git"],
            vec!["--mcp", "--format", "json"],
            vec!["--mcp", "--manual"],
            vec!["--mcp", "--update-tldr"],
            vec!["--update-tldr", "--preserve-anchors"],
            vec!["README.md", "--manual"],
            vec!["-", "--manual"],
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
                    schema: RequestSchema::V6,
                    input: QueryInput::Document {
                        name: "--help".to_owned(),
                        source: None,
                        section: None,
                    },
                    view: QueryView::Full {},
                }),
                presentation: QueryPresentation::Auto,
                pretty: true,
                manual_only: false,
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
