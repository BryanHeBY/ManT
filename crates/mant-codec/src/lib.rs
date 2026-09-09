#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod definitions;
/// Portable document encoding and source-bound Markdown artifacts.
pub mod encode;
#[cfg(feature = "roff")]
mod mandoc;
mod markdown;
/// Source-coordinate mapping for parsed CommonMark inline events.
pub mod markdown_mapping;
mod producer_identity;
mod text_safety;
mod tldr;

pub use mant_ir::ResolvedContent;
pub use markdown::{MarkdownParseError, ParsedMarkdown, TldrDirectiveError, parse_markdown};
pub use tldr::{TldrPageLocation, TldrParseError, parse_tldr_command, parse_tldr_page};

#[cfg(feature = "roff")]
pub use mandoc::{
    RedirectSyntaxError, lower_mandoc_document, parse_plain_manual as parse_roff_bytes,
    parse_plain_manual_report as parse_roff_bytes_with_report, redirect_target,
};
