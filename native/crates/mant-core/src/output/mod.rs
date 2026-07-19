//! Deterministic textual serializers owned by the native document engine.

mod json;
mod markdown;

pub use json::{render_query_json, render_update_json};
pub use markdown::render_markdown;
