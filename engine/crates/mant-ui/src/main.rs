//! Temporary executable for exercising the Rust UI before it becomes `mant`.

mod arguments;

use std::{
    io::{self, IsTerminal},
    process::ExitCode,
};

use clap::Parser;
use mant_ast::{QueryRequest, QueryView, RequestSchema};
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
    if let Err(error) = mant_ui::run(&bundle) {
        eprintln!("mantui-rs: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
