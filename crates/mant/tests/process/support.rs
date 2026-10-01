use super::support::{configure_registered_documents, registered_documents_dir};
use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

pub(super) fn executable() -> &'static str {
    env!("CARGO_BIN_EXE_mant")
}

pub(super) fn registered_data_root(home: &std::path::Path) -> PathBuf {
    registered_documents_dir(home)
        .parent()
        .expect("registered documents always have an application data root")
        .to_owned()
}

pub(super) fn run_with_registered_documents(
    home: &std::path::Path,
    arguments: &[&str],
) -> std::process::Output {
    let mut command = Command::new(executable());
    configure_registered_documents(&mut command, home);
    command.args(arguments).output().expect("run isolated mant")
}

pub(super) fn run_text_input(arguments: &[&str], input: &str) -> std::process::Output {
    let mut command = Command::new(executable());
    command.args(arguments);
    run_text_command(command, input)
}

pub(super) fn run_text_command(mut command: Command, input: &str) -> std::process::Output {
    let mut child = command
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start text query");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write query input");
    child.wait_with_output().expect("finish query")
}

pub(super) fn scope_document_paths(response: &serde_json::Value) -> Vec<&str> {
    response["scope"]["documents"]
        .as_array()
        .expect("scope documents")
        .iter()
        .map(|document| document["address"]["path"].as_str().expect("Markdown path"))
        .collect()
}
