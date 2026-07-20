//! Defines and validates the public `mant-cli` command line with clap.
//!
//! The interface intentionally has one positional value: the manual topic.
//! Every action, projection, input mode, and output choice is a long option so
//! humans and agents do not have to distinguish ad-hoc subcommand grammars.

use std::iter;

use clap::{ArgAction, ArgGroup, CommandFactory, Parser, ValueEnum, error::ErrorKind};
use mant_ast::{OutlineDetail, QueryRequest, QueryView, RequestSchema};

// ── Public command model ───────────────────────────────────────────────────

/// The output selected for one manual query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum QueryFormat {
    Markdown,
    Text,
    Json,
}

/// A discoverable JSON Schema exposed by the native process boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum SchemaContract {
    Request,
    Query,
    Outline,
    Excerpt,
    All,
}

/// Semantic entries included beneath the ordinary section outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutlineMode {
    Sections,
    Options,
}

impl From<OutlineMode> for OutlineDetail {
    fn from(value: OutlineMode) -> Self {
        match value {
            OutlineMode::Sections => Self::Sections,
            OutlineMode::Options => Self::Options,
        }
    }
}

/// Where a query request comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum QuerySource {
    Arguments(QueryRequest),
    StdinJson,
}

/// One validated invocation of the native CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Help(String),
    Query {
        source: QuerySource,
        format: QueryFormat,
        pretty: bool,
    },
    UpdateTldr {
        pretty: bool,
    },
    ProtocolVersion {
        pretty: bool,
    },
    Schema {
        contract: SchemaContract,
        pretty: bool,
    },
}

// ── Declarative command line ───────────────────────────────────────────────

#[derive(Debug, Parser)]
// These booleans are declarative CLI switches, not coupled domain state; clap
// validates their relationships before `Cli` is normalized into `Command`.
#[allow(clippy::struct_excessive_bools)]
#[command(
    name = "mant-cli",
    about = "Query local manual pages for scripts and agents",
    disable_help_flag = true,
    disable_version_flag = true,
    override_usage = "mant-cli <TOPIC> [OPTIONS]\n       mant-cli --request-json [--format <FORMAT>] [--compact]\n       mant-cli --schema <CONTRACT> [--compact]\n       mant-cli --update-tldr [--compact]\n       mant-cli --protocol-version [--compact]",
    after_help = "Examples:\n  mant-cli git\n  mant-cli printf --section 3 --format json\n  mant-cli gcc --outline\n  mant-cli tar --outline options\n  mant-cli tar --node acls --format markdown\n  mant-cli gcc --node 4.2 --format markdown\n  mant-cli --request-json --format json --compact\n  mant-cli --schema request\n  mant-cli --update-tldr",
    group = ArgGroup::new("source")
        .args(["topic", "request_json", "update_tldr", "protocol_version", "schema"])
        .required(true)
        .multiple(false)
)]
struct Cli {
    /// Manual page topic. This is the command line's only positional value.
    #[arg(value_name = "TOPIC", value_parser = non_empty)]
    topic: Option<String>,

    /// Select a manual section such as 1 or 3p.
    #[arg(long, value_name = "SECTION", value_parser = non_empty, requires = "topic")]
    section: Option<String>,

    /// Print selectable sections, optionally including command-line options.
    #[arg(long, value_name = "DETAIL", value_enum, num_args = 0..=1, default_missing_value = "sections", requires = "topic", conflicts_with = "node")]
    outline: Option<OutlineMode>,

    /// Print a node by outline path, document ID, or option alias; repeatable.
    #[arg(long, value_name = "NODE", value_parser = non_empty, requires = "topic")]
    node: Vec<String>,

    /// Read a versioned `QueryRequest` JSON object from standard input.
    #[arg(long, conflicts_with_all = ["section", "outline", "node"])]
    request_json: bool,

    /// Update tldr data through the installed client or `ManT` cache.
    #[arg(long, conflicts_with_all = ["section", "outline", "node", "format"])]
    update_tldr: bool,

    /// Print the native protocol description as JSON.
    #[arg(long, conflicts_with_all = ["section", "outline", "node", "format"])]
    protocol_version: bool,

    /// Print a generated JSON Schema contract (`request`, `query`, `outline`, `excerpt`, or `all`).
    #[arg(long, value_name = "CONTRACT", value_enum, conflicts_with_all = ["section", "outline", "node", "format"])]
    schema: Option<SchemaContract>,

    /// Output format. Content defaults to markdown; outlines default to text.
    #[arg(long, value_name = "FORMAT", value_enum)]
    format: Option<QueryFormat>,

    /// Omit JSON indentation. Query output also requires `--format json`.
    #[arg(long)]
    compact: bool,

    /// Print help.
    #[arg(long, action = ArgAction::Help)]
    help: Option<bool>,
}

// ── Normalization and semantic validation ─────────────────────────────────

pub(crate) fn parse(arguments: &[String]) -> Result<Command, clap::Error> {
    let parsed = match Cli::try_parse_from(
        iter::once("mant-cli").chain(arguments.iter().map(String::as_str)),
    ) {
        Ok(parsed) => parsed,
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            return Ok(Command::Help(error.to_string()));
        }
        Err(error) => return Err(error),
    };

    normalize(parsed)
}

fn normalize(parsed: Cli) -> Result<Command, clap::Error> {
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

    let view = if let Some(detail) = parsed.outline {
        QueryView::Outline {
            detail: detail.into(),
        }
    } else if parsed.node.is_empty() {
        QueryView::Full {}
    } else {
        QueryView::Excerpt { nodes: parsed.node }
    };
    let format = parsed.format.unwrap_or(match &view {
        QueryView::Outline { .. } => QueryFormat::Text,
        QueryView::Full { .. } | QueryView::Excerpt { .. } => QueryFormat::Markdown,
    });
    if parsed.compact && format != QueryFormat::Json {
        return Err(command_error(
            ErrorKind::ArgumentConflict,
            "--compact requires --format json for manual queries",
        ));
    }

    let source = if parsed.request_json {
        QuerySource::StdinJson
    } else {
        QuerySource::Arguments(QueryRequest {
            schema: RequestSchema::V2,
            topic: parsed.topic.expect("clap requires one input source"),
            section: parsed.section,
            view,
        })
    };

    Ok(Command::Query {
        source,
        format,
        pretty: !parsed.compact,
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

fn command_error(kind: ErrorKind, message: impl std::fmt::Display) -> clap::Error {
    Cli::command().error(kind, message)
}

#[cfg(test)]
mod tests {
    use mant_ast::{OutlineDetail, QueryRequest, QueryView, RequestSchema};

    use super::{Command, QueryFormat, QuerySource, SchemaContract, parse};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn defaults_direct_queries_to_markdown() {
        assert_eq!(
            parse(&args(&["git"])).expect("query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V2,
                    topic: "git".to_owned(),
                    section: None,
                    view: QueryView::Full {},
                }),
                format: QueryFormat::Markdown,
                pretty: true,
            }
        );
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
                    schema: RequestSchema::V2,
                    topic: "printf".to_owned(),
                    section: Some("3".to_owned()),
                    view: QueryView::Full {},
                }),
                format: QueryFormat::Json,
                pretty: false,
            }
        );
    }

    #[test]
    fn parses_the_closed_stdin_request_mode_used_by_the_tui() {
        assert_eq!(
            parse(&args(&["--request-json", "--format", "json", "--compact",]))
                .expect("stdin query"),
            Command::Query {
                source: QuerySource::StdinJson,
                format: QueryFormat::Json,
                pretty: false,
            }
        );
    }

    #[test]
    fn parses_outline_and_repeatable_node_views_with_contextual_defaults() {
        assert_eq!(
            parse(&args(&["gcc", "--outline"])).expect("outline"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V2,
                    topic: "gcc".to_owned(),
                    section: None,
                    view: QueryView::Outline {
                        detail: OutlineDetail::Sections,
                    },
                }),
                format: QueryFormat::Text,
                pretty: true,
            }
        );
        assert_eq!(
            parse(&args(&["tar", "--outline", "options", "--format", "json"]))
                .expect("option outline"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V2,
                    topic: "tar".to_owned(),
                    section: None,
                    view: QueryView::Outline {
                        detail: OutlineDetail::Options,
                    },
                }),
                format: QueryFormat::Json,
                pretty: true,
            }
        );
        assert_eq!(
            parse(&args(&[
                "gcc", "--node", "4.2", "--node", "files-8", "--format", "text",
            ]))
            .expect("excerpt"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V2,
                    topic: "gcc".to_owned(),
                    section: None,
                    view: QueryView::Excerpt {
                        nodes: vec!["4.2".to_owned(), "files-8".to_owned()],
                    },
                }),
                format: QueryFormat::Text,
                pretty: true,
            }
        );
    }

    #[test]
    fn parses_long_option_actions_without_ad_hoc_subcommands() {
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
    }

    #[test]
    fn rejects_ambiguous_or_incompatible_inputs() {
        let cases = [
            vec!["git", "--format", "json", "--format", "text"],
            vec!["git", "--compact"],
            vec!["--request-json", "git", "--format", "json"],
            vec!["--request-json", "--section", "1", "--format", "json"],
            vec!["--request-json", "--outline", "--format", "json"],
            vec!["git", "--outline", "--node", "1"],
            vec!["git", "--node"],
            vec!["--section", "1"],
            vec!["--update-tldr", "--format", "json"],
            vec!["--schema", "request", "--format", "json"],
            vec!["--schema", "unknown"],
            vec!["update", "tldr"],
            vec!["git", "--json"],
            vec!["git", "--md"],
            vec!["git", "--markdown"],
            vec!["git", "--text"],
            vec!["-h"],
            vec!["git", "-s", "1"],
            vec!["git", "-n", "1"],
            vec!["--unknown", "git"],
        ];
        for values in cases {
            assert!(parse(&args(&values)).is_err(), "accepted {values:?}");
        }
    }

    #[test]
    fn help_is_side_effect_free_and_the_option_terminator_preserves_a_topic() {
        let help = parse(&args(&["--help"])).expect("help");
        assert!(matches!(help, Command::Help(text) if text.contains("Usage: mant-cli")));
        assert_eq!(
            parse(&args(&["--", "--help"])).expect("query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    schema: RequestSchema::V2,
                    topic: "--help".to_owned(),
                    section: None,
                    view: QueryView::Full {},
                }),
                format: QueryFormat::Markdown,
                pretty: true,
            }
        );
    }
}
