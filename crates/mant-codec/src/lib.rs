#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod definitions;
/// Mathematical/readable document encoding and source-bound Markdown artifacts.
pub mod encode;
mod heading_attribute;
#[cfg(feature = "roff")]
mod mandoc;
mod markdown;
/// Source-coordinate mapping for parsed CommonMark inline events.
pub mod markdown_mapping;
/// Bounded MathML Core and restricted LaTeX encoding of resolved formula IR.
pub mod math;
mod native_link_snapshot;
mod producer_identity;
mod text_safety;
mod tldr;

pub use mant_ir::ResolvedContent;
pub use markdown::{MarkdownParseError, ParsedMarkdown, TldrDirectiveError, parse_markdown};
pub use native_link_snapshot::{
    DecodedLinkDocument, NativeLinkLayout, NativeLinkRestriction, SourceBoundLinkLayout,
};
pub use tldr::{TldrPageLocation, TldrParseError, parse_tldr_command, parse_tldr_page};

#[cfg(feature = "roff")]
#[doc(hidden)]
pub use mandoc::{
    RoffSectionHeading, RoffSectionTargets, RoffWordEffect, RoffWordEvidence, SectionLabelEvidence,
    observe_roff_link_operand, observe_roff_section_label_evidence,
    observe_roff_section_label_glyphs, observe_roff_section_phrase, roff_vertical_space_lines,
};

#[cfg(feature = "roff")]
pub use mandoc::{
    RedirectSyntaxError, lower_mandoc_document, parse_plain_manual as parse_roff_bytes,
    parse_plain_manual_report as parse_roff_bytes_with_report,
    parse_plain_manual_with_link_layout as parse_roff_bytes_with_link_layout, redirect_target,
};
