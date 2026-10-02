//! Black-box checks for stdout, stderr, exit codes and process isolation.

mod support;

#[path = "process/support.rs"]
mod process_support;

#[path = "process/command_surface.rs"]
mod command_surface;

#[path = "process/display_output.rs"]
mod display_output;

#[path = "process/requests.rs"]
mod requests;

#[path = "process/semantic_queries.rs"]
mod semantic_queries;

#[path = "process/document_resolution.rs"]
mod document_resolution;

#[path = "process/quick_references.rs"]
mod quick_references;

#[path = "process/document_scopes.rs"]
mod document_scopes;

#[cfg(feature = "update")]
#[path = "process/source_updates.rs"]
mod source_updates;

#[cfg(windows)]
#[path = "process/windows_resolution.rs"]
mod windows_resolution;

const PROTOCOL_REFERENCE: &str = include_str!("../../../docs/manuals/mant-protocol.md");
