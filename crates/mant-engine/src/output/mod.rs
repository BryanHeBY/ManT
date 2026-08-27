//! Deterministic textual serializers owned by the native document engine.

mod json;
mod markdown;
mod search;
mod text;

use mant_protocol::{EntryProjection, QueryOutline};

pub use json::{
    render_excerpt_json, render_outline_json, render_query_json, render_search_json,
    render_update_json,
};
pub(crate) use markdown::{
    MarkdownArtifact, MarkdownNode, MarkdownNodeRange, MarkdownSection, render_addressable_markdown,
};
pub use markdown::{
    MarkdownOptions, render_excerpt_markdown, render_excerpt_markdown_with_options,
    render_markdown, render_markdown_with_options, render_outline_markdown,
};
pub use search::{
    SearchTextRole, render_search_markdown, render_search_text, render_search_text_with,
};
pub use text::{
    render_excerpt_text, render_outline_entry_summary, render_outline_text, render_query_man,
    render_query_text,
};

fn outline_empty_message(outline: &QueryOutline) -> Option<String> {
    if !outline.nodes.is_empty() {
        return None;
    }
    let EntryProjection::Kinds { kinds } = &outline.entries else {
        return None;
    };
    let labels = kinds
        .iter()
        .map(|kind| text::entry_kind_label(*kind, false))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!("0 matching semantic entries for: {labels}"))
}
