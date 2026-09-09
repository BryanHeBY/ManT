//! Cross-crate query-to-report contracts; renderer unit tests need no query engine.

#[path = "report_composition/markdown.rs"]
mod markdown;
#[path = "report_composition/text.rs"]
mod text;
