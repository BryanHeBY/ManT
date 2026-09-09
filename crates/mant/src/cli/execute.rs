//! CLI command outcomes, rendering dispatch and direct stream delivery.
use super::query::{QueryExecution, QueryOutput, execute_query};
use crate::{
    arguments::{ColorMode, Command, DisplayMode, OutputOptions, QueryFormat},
    doctor,
    error::{Failure, report_failure},
    host::CliHost,
    presentation::{self, render_json},
    schema_output,
};
use std::io::{self, Read, Write};

/// Rendered data and its business outcome remain paired through delivery.
struct CommandOutcome {
    rendered: String,
    status: u8,
}

pub(crate) fn run_command(
    command: Command,
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
    diagnostics_color: bool,
    output_target: presentation::OutputTarget,
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
            presentation: OutputOptions {
                display: DisplayMode::Tui,
                ..
            },
            ..
        }
    ) {
        return report_failure(
            &Failure::usage("interactive mode requires the native terminal process boundary"),
            diagnostics,
            diagnostics_color,
        );
    }

    let outcome = match command {
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
            CommandOutcome { rendered, status }
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
            CommandOutcome { rendered, status }
        }
        Command::Doctor {
            presentation,
            pretty,
        } => {
            let format = presentation.format();
            let color = presentation.color;
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
            CommandOutcome { rendered, status }
        }
        command => match execute(command, input, host, output_target) {
            Ok(rendered) => CommandOutcome {
                rendered,
                status: 0,
            },
            Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
        },
    };

    match write_output(output, &outcome.rendered) {
        Ok(()) => outcome.status,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => outcome.status,
        Err(error) => report_failure(&Failure::operational(error), diagnostics, diagnostics_color),
    }
}

fn execute(
    command: Command,
    input: &mut dyn Read,
    host: &dyn CliHost,
    output_target: presentation::OutputTarget,
) -> Result<String, Failure> {
    match command {
        Command::Help(help) => Ok(help),
        Command::ProtocolVersion { pretty } => schema_output::render_protocol_description(pretty),
        Command::Schema { contract, pretty } => schema_output::render_schema(contract, pretty),
        Command::Catalog {
            query,
            grouped,
            presentation,
            pretty,
            ..
        } => {
            let format = presentation.format();
            let catalog = host.discover(&query)?;
            match format {
                QueryFormat::Json => render_json(&catalog, pretty),
                QueryFormat::Text => Ok(presentation::render_catalog_output(
                    &catalog,
                    grouped,
                    presentation.color == ColorMode::Always,
                )),
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
            mant_render::render_update_json(&update, pretty).map_err(Failure::operational)
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
                policy,
                output: QueryOutput {
                    presentation,
                    pretty,
                    preserve_anchors,
                    target: output_target,
                },
            },
            input,
            host,
        ),
    }
}

fn write_output(output: &mut dyn Write, rendered: &str) -> io::Result<()> {
    output.write_all(rendered.as_bytes())?;
    if !rendered.ends_with('\n') {
        output.write_all(b"\n")?;
    }
    output.flush()
}
