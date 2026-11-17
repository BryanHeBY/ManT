//! Cross-crate query-to-report contracts; renderer unit tests need no query engine.

#[path = "report_composition/alias_navigation.rs"]
mod alias_navigation;
#[path = "report_composition/detached_navigation.rs"]
mod detached_navigation;
#[path = "report_composition/identity.rs"]
mod identity;
#[path = "report_composition/markdown.rs"]
mod markdown;
#[path = "report_composition/text.rs"]
mod text;
