//! Temporary executable for exercising the Rust UI before it becomes `mant`.

mod arguments;

use std::{
    io::{self, IsTerminal},
    process::ExitCode,
};

use clap::Parser;
use mant_ast::{Diagnostic, QueryBundle, QueryRequest, QueryView, RequestSchema, SourceFormat};
use mant_core::query_with_policy;

use crate::arguments::Arguments;

fn main() -> ExitCode {
    let arguments = Arguments::parse();
    let invocation = match arguments.invocation() {
        Ok(invocation) => invocation,
        Err(error) => error.exit(),
    };
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!(
            "mantui-rs: interactive view requires a terminal; use mant for Markdown or JSON output"
        );
        return ExitCode::FAILURE;
    }
    let request = QueryRequest {
        schema: RequestSchema::V3,
        input: invocation.input,
        view: QueryView::Full {},
    };
    let bundle = match query_with_policy(&request, invocation.policy) {
        Ok(bundle) => bundle,
        Err(error) => {
            eprintln!("mantui-rs: {error}");
            return ExitCode::FAILURE;
        }
    };
    if invocation.policy.force_libmandoc || invocation.policy.force_groff {
        report_manual_diagnostics(&bundle);
    }
    if let Err(error) = mant_ui::run(&bundle) {
        eprintln!("mantui-rs: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn report_manual_diagnostics(bundle: &QueryBundle) {
    let Some(manual) = &bundle.document else {
        return;
    };
    let engine = match manual.source.format {
        SourceFormat::GroffHtml => "groff HTML",
        SourceFormat::Man | SourceFormat::Mdoc | SourceFormat::MandocHtml => "libmandoc",
        SourceFormat::Markdown => "Markdown",
    };
    for diagnostic in &manual.diagnostics {
        eprintln!("mantui-rs: {engine} {}", format_diagnostic(diagnostic));
    }
}

fn format_diagnostic(diagnostic: &Diagnostic) -> String {
    let location = diagnostic.source.map_or_else(String::new, |source| {
        format!(" at {}:{}", source.line, source.column)
    });
    format!("{:?}{location}: {}", diagnostic.level, diagnostic.message)
}

#[cfg(test)]
mod tests {
    use mant_ast::{Diagnostic, DiagnosticLevel, SourceSpan};

    use super::format_diagnostic;

    #[test]
    fn parser_diagnostics_keep_their_severity_and_source_location() {
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some("TEST".to_owned()),
            message: "renderer warning".to_owned(),
            source: Some(SourceSpan {
                line: 7,
                column: 3,
                end_line: None,
                end_column: None,
            }),
        };

        assert_eq!(
            format_diagnostic(&diagnostic),
            "Warning at 7:3: renderer warning"
        );
    }
}
