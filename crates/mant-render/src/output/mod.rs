//! Deterministic body and report renderers over existing IR and protocol facts.

mod explanation;
mod json;
pub use explanation::{
    render_explanation_markdown, render_explanation_text, render_explanation_text_with,
    render_scope_explanation_markdown, render_scope_explanation_text,
    render_scope_explanation_text_with,
};
mod markdown_report;
mod outline;
mod scope;
pub use scope::{
    ScopeTextRole, render_scope_query_markdown, render_scope_query_markdown_with,
    render_scope_query_text, render_scope_query_text_with,
};
mod search;
mod styles;
mod table;
mod text;
pub use outline::{
    render_outline_entry_summary, render_outline_relationships, render_outline_text,
    render_outline_text_with,
};

use mant_protocol::{EntryProjection, QueryOutline};

pub use json::{
    render_excerpt_json, render_outline_json, render_query_json, render_search_json,
    render_update_json,
};
pub use markdown_report::{
    render_excerpt_markdown, render_excerpt_markdown_with_options, render_outline_markdown,
};
pub use search::{
    SearchTextRole, render_search_markdown, render_search_text, render_search_text_with,
};
pub use text::{
    render_excerpt_text, render_excerpt_text_with, render_query_man, render_query_text,
    render_query_text_with,
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
        .map(|kind| outline::entry_kind_label(*kind, false))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!("0 matching semantic entries for: {labels}"))
}
