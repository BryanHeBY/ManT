//! OS entry point for the standalone native `mant` executable.

use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let mut arguments = Vec::new();
    for argument in env::args_os().skip(1) {
        let Ok(argument) = argument.into_string() else {
            eprintln!("mant: command-line arguments must be UTF-8");
            eprintln!("Try 'mant --help' for more information.");
            return ExitCode::from(2);
        };
        arguments.push(argument);
    }

    let status = mant::run_process(&arguments);
    ExitCode::from(status)
}
