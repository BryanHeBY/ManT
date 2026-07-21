//! OS entry point for the standalone native `mant-cli` executable.

use std::{env, process::ExitCode};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut arguments = Vec::new();
    for argument in env::args_os().skip(1) {
        let Ok(argument) = argument.into_string() else {
            eprintln!("mant-cli: command-line arguments must be UTF-8");
            eprintln!("Try 'mant-cli --help' for more information.");
            return ExitCode::from(2);
        };
        arguments.push(argument);
    }

    let status = mant_cli::run_process(&arguments).await;
    ExitCode::from(status)
}
