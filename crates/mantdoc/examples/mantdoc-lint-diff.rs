//! Compare native parser diagnostics against upstream mandoc lint goldens.

#[path = "support/mantdoc-lint-diff.rs"]
mod lint_diff;

fn main() -> std::process::ExitCode {
    lint_diff::main()
}
