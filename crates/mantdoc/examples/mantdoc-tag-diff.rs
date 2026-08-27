//! Compare native AST destination tags against upstream mandoc tag output.

#[path = "support/mantdoc-tag-diff.rs"]
mod tag_diff;

fn main() -> std::process::ExitCode {
    tag_diff::main()
}
