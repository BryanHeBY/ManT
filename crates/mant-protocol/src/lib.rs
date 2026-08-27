#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod catalog;
mod doctor;
mod document;
mod outline;
mod presentation;
mod query;
mod schema;
mod scope;
mod search;
mod selector;
mod update;

pub use catalog::*;
pub use doctor::*;
pub use document::*;
pub use outline::*;
pub use presentation::*;
pub use query::*;
pub use schema::*;
pub use scope::*;
pub use search::*;
pub use selector::*;
pub use update::*;

/// Pre-stable native API release line shared by the query protocol family.
pub const NATIVE_API_VERSION: &str = "0.10";

/// Exact process protocol reported by the native CLI boundary.
pub const CLI_PROTOCOL_VERSION: &str = "mant.cli/v0.10";

#[cfg(test)]
mod tests {
    use super::{
        CLI_PROTOCOL_VERSION, CatalogSchema, DocumentSchema, ExcerptSchema, NATIVE_API_VERSION,
        OutlineSchema, QuerySchema, RequestSchema, ScopeQuerySchema, ScopeRequestSchema,
        SearchSchema,
    };

    #[test]
    fn native_api_version_is_explicit() {
        assert_eq!(NATIVE_API_VERSION, "0.10");
        assert_eq!(CLI_PROTOCOL_VERSION, "mant.cli/v0.10");
    }

    #[test]
    fn advertised_schema_ids_match_their_serialized_markers() {
        for (value, expected) in [
            (
                serde_json::to_value(RequestSchema::V0Dot10),
                RequestSchema::ID,
            ),
            (serde_json::to_value(QuerySchema::V0Dot10), QuerySchema::ID),
            (
                serde_json::to_value(DocumentSchema::V0Dot10),
                DocumentSchema::ID,
            ),
            (
                serde_json::to_value(OutlineSchema::V0Dot10),
                OutlineSchema::ID,
            ),
            (
                serde_json::to_value(ExcerptSchema::V0Dot10),
                ExcerptSchema::ID,
            ),
            (
                serde_json::to_value(SearchSchema::V0Dot10),
                SearchSchema::ID,
            ),
            (
                serde_json::to_value(ScopeRequestSchema::V0Dot10),
                ScopeRequestSchema::ID,
            ),
            (
                serde_json::to_value(ScopeQuerySchema::V0Dot10),
                ScopeQuerySchema::ID,
            ),
            (
                serde_json::to_value(CatalogSchema::V0Dot10),
                CatalogSchema::ID,
            ),
        ] {
            assert_eq!(value.expect("serialize schema marker"), expected);
        }
    }
}
