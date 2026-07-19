//! Deterministic textual serializers owned by the native document engine.

mod json;

pub use json::{render_query_json, render_update_json};
