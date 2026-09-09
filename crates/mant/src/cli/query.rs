//! CLI input adaptation and presentation around complete document requests.
use crate::{
    application,
    arguments::{ColorMode, OutputOptions, QuerySource},
    delivery,
    error::{Failure, query_execution_failure, query_failure},
    host::CliHost,
    presentation::{self, render_query_result},
    request_input::{
        NativeRequest, read_input_bytes, read_native_request, read_query_request, read_utf8_input,
    },
};
use mant_engine::LoadPolicy;
use mant_protocol::{InputFormat, ScopeQueryRequest, ScopeRequestSchema};
use std::io::Read;

/// Normalized fields of a conventional CLI document query.
pub(super) struct QueryExecution {
    pub(super) source: QuerySource,
    pub(super) policy: LoadPolicy,
    pub(super) output: QueryOutput,
}

/// Resolved presentation policy shared by single- and multi-document queries.
#[derive(Debug, Clone, Copy)]
pub(super) struct QueryOutput {
    pub(super) presentation: OutputOptions,
    pub(super) pretty: bool,
    pub(super) preserve_anchors: bool,
    pub(super) target: presentation::OutputTarget,
}

impl QueryOutput {
    fn render_options(self) -> presentation::RenderOptions {
        presentation::RenderOptions {
            format: self.presentation.format(),
            pretty: self.pretty,
            preserve_anchors: self.preserve_anchors,
            color: self.presentation.color == ColorMode::Always,
            target: self.target,
        }
    }
}

/// Load one manual query and render the projection encoded in its request.
pub(super) fn execute_query(
    command: QueryExecution,
    input: &mut dyn Read,
    host: &dyn CliHost,
) -> Result<String, Failure> {
    let QueryExecution {
        source,
        policy,
        output,
    } = command;
    let source = match source {
        QuerySource::ScopeArguments { scope, view } => {
            return execute_scope_arguments(scope, view, output, policy, host);
        }
        QuerySource::StdinJson => match read_native_request(input)? {
            NativeRequest::Query(request) => QuerySource::Arguments(request),
            NativeRequest::Scope(request) => {
                return execute_scope_request(&request, output, host);
            }
        },
        source => source,
    };
    let result = match source {
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
            application::execute_query(&request, policy, host)?
        }
    };
    if policy == LoadPolicy::TldrOnly && output.presentation.format.is_none() {
        let color = output.presentation.color;
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
                Ok(delivery::tldr::render_tldr_terminal(
                    document,
                    color == ColorMode::Always,
                ))
            },
        );
    }
    render_query_result(&result, output.render_options())
}

fn execute_scope_arguments(
    scope: mant_protocol::DocumentScope,
    view: Option<mant_protocol::ScopeQueryView>,
    output: QueryOutput,
    policy: LoadPolicy,
    host: &dyn CliHost,
) -> Result<String, Failure> {
    let Some(view) = view else {
        return Err(Failure::usage(
            "multi-document output requires --search or --explain; use --display tui for interactive reading",
        ));
    };
    if policy != LoadPolicy::Combined {
        return Err(Failure::usage(
            "--manual and --tldr do not apply to multi-document scopes",
        ));
    }
    let request = ScopeQueryRequest {
        schema: ScopeRequestSchema::V0Dot11,
        scope,
        view,
    };
    execute_scope_request(&request, output, host)
}

fn execute_scope_request(
    request: &ScopeQueryRequest,
    output: QueryOutput,
    host: &dyn CliHost,
) -> Result<String, Failure> {
    let response = application::execute_scope_query(request, host)?;
    presentation::render_scope_query_result(&response, output.render_options())
}

fn validate_markdown_policy(policy: LoadPolicy) -> Result<(), Failure> {
    if policy != LoadPolicy::Combined {
        return Err(Failure::usage(
            "content-only policies do not apply to Markdown input",
        ));
    }
    Ok(())
}
