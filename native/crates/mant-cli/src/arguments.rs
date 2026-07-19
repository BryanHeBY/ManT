//! Parses the public `mant-cli` command line without pulling in a CLI framework.

use mant_ast::QueryRequest;

// ── Public command model ───────────────────────────────────────────────────

/// The machine-readable output selected for one manual query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryFormat {
    Markdown,
    Text,
    Json,
}

/// Projection applied after the complete manual has been queried once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum QueryView {
    Full,
    Outline,
    Excerpt(Vec<String>),
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
    Help,
    Query {
        source: QuerySource,
        view: QueryView,
        format: QueryFormat,
        pretty: bool,
    },
    UpdateTldr {
        pretty: bool,
    },
    ProtocolVersion {
        pretty: bool,
    },
}

/// A command-line error that should terminate with exit status 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UsageError(pub(crate) String);

// ── Help text ──────────────────────────────────────────────────────────────

pub(crate) const HELP: &str = r#"Mant CLI — query local manual pages for scripts and agents

Usage:
  mant-cli <topic> [--section <section>] [--outline | --node <node>...]
             [--json | --markdown | --text] [--compact]
  mant-cli --request-json [--json | --markdown | --text] [--compact]
  mant-cli update tldr [--compact]
  mant-cli protocol-version [--compact]
  mant-cli --help

Query options:
  -j, --json          Print the versioned query document as JSON
      --md, --markdown
                      Print Markdown (the default for content)
      --text          Print unstyled text (the default for outlines)
  -s, --section       Select a manual section, such as 1 or 3p
      --outline       Print the manual's selectable section tree
  -n, --node          Print one outline node by path or ID; repeatable
      --request-json  Read a mant QueryRequest object from stdin
      --compact       Omit JSON indentation; requires --json
  --                  Treat all remaining arguments as the topic

Examples:
  mant-cli git
  mant-cli printf --section 3 --json
  mant-cli gcc --outline
  mant-cli gcc --node 4.2 --markdown
  printf '%s' '{"topic":"git"}' | mant-cli --request-json --json --compact
  mant-cli update tldr"#;

// ── Argument parser ────────────────────────────────────────────────────────

pub(crate) fn parse(arguments: &[String]) -> Result<Command, UsageError> {
    if arguments
        .iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| argument == "--help" || argument == "-h")
    {
        return Ok(Command::Help);
    }

    if arguments
        .first()
        .is_some_and(|value| value == "protocol-version")
    {
        return parse_json_only_command(arguments, "protocol-version", |pretty| {
            Command::ProtocolVersion { pretty }
        });
    }

    if arguments.first().is_some_and(|value| value == "update")
        && arguments.get(1).is_some_and(|value| value == "tldr")
    {
        return parse_json_only_command(&arguments[1..], "tldr", |pretty| Command::UpdateTldr {
            pretty,
        });
    }

    parse_query(arguments)
}

fn parse_json_only_command(
    arguments: &[String],
    command_name: &str,
    build: impl FnOnce(bool) -> Command,
) -> Result<Command, UsageError> {
    let mut pretty = true;
    for argument in &arguments[1..] {
        match argument.as_str() {
            "--compact" => pretty = false,
            value => {
                return Err(UsageError(format!(
                    "unknown option or argument '{value}' after {command_name}"
                )));
            }
        }
    }
    Ok(build(pretty))
}

fn parse_query(arguments: &[String]) -> Result<Command, UsageError> {
    let mut format = None;
    let mut pretty = true;
    let mut request_json = false;
    let mut section = None;
    let mut outline = false;
    let mut nodes = Vec::new();
    let mut topic_parts = Vec::new();
    let mut parse_options = true;
    let mut index = 0;

    while index < arguments.len() {
        let argument = &arguments[index];
        if parse_options && argument == "--" {
            parse_options = false;
        } else if parse_options && (argument == "--json" || argument == "-j") {
            select_format(&mut format, QueryFormat::Json)?;
        } else if parse_options && (argument == "--markdown" || argument == "--md") {
            select_format(&mut format, QueryFormat::Markdown)?;
        } else if parse_options && argument == "--text" {
            select_format(&mut format, QueryFormat::Text)?;
        } else if parse_options && argument == "--compact" {
            pretty = false;
        } else if parse_options && argument == "--request-json" {
            request_json = true;
        } else if parse_options && argument == "--outline" {
            if std::mem::replace(&mut outline, true) {
                return Err(UsageError("--outline may only be supplied once".to_owned()));
            }
        } else if parse_options && (argument == "--node" || argument == "-n") {
            index += 1;
            let value = arguments
                .get(index)
                .ok_or_else(|| UsageError("--node requires a value".to_owned()))?;
            if value.trim().is_empty() {
                return Err(UsageError("--node must not be empty".to_owned()));
            }
            nodes.push(value.clone());
        } else if parse_options && (argument == "--section" || argument == "-s") {
            index += 1;
            let value = arguments
                .get(index)
                .ok_or_else(|| UsageError("--section requires a value".to_owned()))?;
            if section.replace(value.clone()).is_some() {
                return Err(UsageError("--section may only be supplied once".to_owned()));
            }
        } else if parse_options && argument.starts_with('-') {
            return Err(UsageError(format!("unknown option '{argument}'")));
        } else {
            topic_parts.push(argument.clone());
        }
        index += 1;
    }

    if outline && !nodes.is_empty() {
        return Err(UsageError(
            "--outline and --node cannot be combined".to_owned(),
        ));
    }
    let view = if outline {
        QueryView::Outline
    } else if nodes.is_empty() {
        QueryView::Full
    } else {
        QueryView::Excerpt(nodes)
    };
    let format = format.unwrap_or(match &view {
        QueryView::Outline => QueryFormat::Text,
        QueryView::Full | QueryView::Excerpt(_) => QueryFormat::Markdown,
    });
    if !pretty && format != QueryFormat::Json {
        return Err(UsageError("--compact requires --json".to_owned()));
    }

    let source = if request_json {
        if !topic_parts.is_empty() || section.is_some() || view != QueryView::Full {
            return Err(UsageError(
                "--request-json cannot be combined with a topic, --section, --outline, or --node"
                    .to_owned(),
            ));
        }
        QuerySource::StdinJson
    } else {
        let topic = topic_parts.join(" ").trim().to_owned();
        if topic.is_empty() {
            return Err(UsageError("a manual topic is required".to_owned()));
        }
        QuerySource::Arguments(QueryRequest { topic, section })
    };

    Ok(Command::Query {
        source,
        view,
        format,
        pretty,
    })
}

fn select_format(
    current: &mut Option<QueryFormat>,
    requested: QueryFormat,
) -> Result<(), UsageError> {
    if current.is_some_and(|current| current != requested) {
        return Err(UsageError(
            "--json, --markdown, and --text cannot be combined".to_owned(),
        ));
    }
    *current = Some(requested);
    Ok(())
}

#[cfg(test)]
mod tests {
    use mant_ast::QueryRequest;

    use super::{Command, QueryFormat, QuerySource, QueryView, parse};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn defaults_direct_queries_to_markdown() {
        assert_eq!(
            parse(&args(&["git"])).expect("query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    topic: "git".to_owned(),
                    section: None,
                }),
                view: QueryView::Full,
                format: QueryFormat::Markdown,
                pretty: true,
            }
        );
    }

    #[test]
    fn parses_agent_facing_json_and_section_options() {
        assert_eq!(
            parse(&args(&["printf", "--section", "3", "--json", "--compact"])).expect("query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    topic: "printf".to_owned(),
                    section: Some("3".to_owned()),
                }),
                view: QueryView::Full,
                format: QueryFormat::Json,
                pretty: false,
            }
        );
    }

    #[test]
    fn parses_the_closed_stdin_request_mode_used_by_the_tui() {
        assert_eq!(
            parse(&args(&["--request-json", "--json", "--compact"])).expect("stdin query"),
            Command::Query {
                source: QuerySource::StdinJson,
                view: QueryView::Full,
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
                    topic: "gcc".to_owned(),
                    section: None,
                }),
                view: QueryView::Outline,
                format: QueryFormat::Text,
                pretty: true,
            }
        );
        assert_eq!(
            parse(&args(&["gcc", "--node", "4.2", "-n", "files-8", "--text"])).expect("excerpt"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    topic: "gcc".to_owned(),
                    section: None,
                }),
                view: QueryView::Excerpt(vec!["4.2".to_owned(), "files-8".to_owned()]),
                format: QueryFormat::Text,
                pretty: true,
            }
        );
    }

    #[test]
    fn parses_update_and_protocol_commands() {
        assert_eq!(
            parse(&args(&["update", "tldr"])).expect("update"),
            Command::UpdateTldr { pretty: true }
        );
        assert_eq!(
            parse(&args(&["protocol-version", "--compact"])).expect("version"),
            Command::ProtocolVersion { pretty: false }
        );
    }

    #[test]
    fn rejects_ambiguous_or_incompatible_query_inputs() {
        let cases = [
            vec!["--json", "--markdown", "git"],
            vec!["--json", "--text", "git"],
            vec!["--compact", "git"],
            vec!["--request-json", "git", "--json"],
            vec!["--request-json", "--section", "1", "--json"],
            vec!["--request-json", "--outline", "--json"],
            vec!["git", "--outline", "--node", "1"],
            vec!["git", "--node"],
            vec!["--section"],
            vec!["--unknown", "git"],
        ];
        for values in cases {
            assert!(parse(&args(&values)).is_err(), "accepted {values:?}");
        }
    }

    #[test]
    fn help_is_side_effect_free_and_the_option_terminator_preserves_topics() {
        assert_eq!(
            parse(&args(&["git", "--help", "--json"])).expect("help"),
            Command::Help
        );
        assert_eq!(
            parse(&args(&["--", "--help"])).expect("query"),
            Command::Query {
                source: QuerySource::Arguments(QueryRequest {
                    topic: "--help".to_owned(),
                    section: None,
                }),
                view: QueryView::Full,
                format: QueryFormat::Markdown,
                pretty: true,
            }
        );
    }
}
