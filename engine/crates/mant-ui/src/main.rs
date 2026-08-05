//! Temporary executable for exercising the Rust UI before it becomes `mant`.

use std::{env, path::Path, process::ExitCode};

use mant_ast::{QueryInput, QueryRequest, QueryView, RequestSchema};

fn main() -> ExitCode {
    let Some(input) = env::args().nth(1) else {
        eprintln!("Usage: mantui-rs <TOPIC|MARKDOWN>");
        return ExitCode::from(2);
    };
    if matches!(input.as_str(), "-h" | "--help") {
        println!("Usage: mantui-rs <TOPIC|MARKDOWN>");
        println!("\nDevelopment Ratatui frontend for ManT.");
        return ExitCode::SUCCESS;
    }

    let query_input = if is_markdown_path(&input) {
        QueryInput::MarkdownFile { path: input }
    } else {
        QueryInput::Manual {
            topic: input,
            section: None,
        }
    };
    let request = QueryRequest {
        schema: RequestSchema::V3,
        input: query_input,
        view: QueryView::Full {},
    };
    let bundle = match mant_core::query(&request) {
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

fn is_markdown_path(input: &str) -> bool {
    let path = Path::new(input);
    path.is_file()
        || path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
        || input.contains(std::path::MAIN_SEPARATOR)
}
