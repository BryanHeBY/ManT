//! Offline tldr page parsing, cache discovery, and explicit update operations.

mod parser;

pub use parser::{TldrPageLocation, TldrParseError, parse_tldr_command, parse_tldr_page};
