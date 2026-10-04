#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod catalog;
mod content_selector;
mod coverage;
mod doctor;
mod document;
mod explanation;
mod kind_labels;
mod labels;
mod navigation;
mod outline;
mod query;
mod references;
mod schema;
mod scope;
mod search;
mod selector;
mod update;

pub use catalog::*;
pub use content_selector::*;
pub use doctor::*;
pub use document::*;
pub use explanation::*;
pub use kind_labels::entry_kind_label;
pub use labels::{EntryLabelMode, entry_label};
pub use navigation::DocumentOpenTarget;
pub use outline::*;
pub use query::*;
pub use references::*;
pub use schema::*;
pub use scope::*;
pub use search::*;
pub use selector::*;
pub use update::*;

/// Pre-stable native API release line shared by the query protocol family.
pub const NATIVE_API_VERSION: &str = "0.12";

/// Exact process protocol reported by the native CLI boundary.
pub const CLI_PROTOCOL_VERSION: &str = "mant.cli/v0.12";

#[cfg(test)]
mod tests {
    use super::{
        CLI_PROTOCOL_VERSION, CatalogSchema, DocumentSchema, ExcerptSchema, NATIVE_API_VERSION,
        OutlineSchema, QuerySchema, RequestSchema, ScopeQuerySchema, ScopeRequestSchema,
        SearchSchema, TldrCacheUpdateSchema,
    };

    #[test]
    fn native_api_version_is_explicit() {
        assert_eq!(NATIVE_API_VERSION, "0.12");
        assert_eq!(CLI_PROTOCOL_VERSION, "mant.cli/v0.12");
    }

    #[test]
    fn advertised_schema_ids_match_their_serialized_markers() {
        for (value, expected) in [
            (
                serde_json::to_value(RequestSchema::V0Dot12),
                RequestSchema::ID,
            ),
            (serde_json::to_value(QuerySchema::V0Dot12), QuerySchema::ID),
            (
                serde_json::to_value(DocumentSchema::V0Dot12),
                DocumentSchema::ID,
            ),
            (
                serde_json::to_value(OutlineSchema::V0Dot12),
                OutlineSchema::ID,
            ),
            (
                serde_json::to_value(ExcerptSchema::V0Dot12),
                ExcerptSchema::ID,
            ),
            (
                serde_json::to_value(SearchSchema::V0Dot12),
                SearchSchema::ID,
            ),
            (
                serde_json::to_value(ScopeRequestSchema::V0Dot12),
                ScopeRequestSchema::ID,
            ),
            (
                serde_json::to_value(ScopeQuerySchema::V0Dot12),
                ScopeQuerySchema::ID,
            ),
            (
                serde_json::to_value(CatalogSchema::V0Dot12),
                CatalogSchema::ID,
            ),
            (
                serde_json::to_value(TldrCacheUpdateSchema::V1),
                TldrCacheUpdateSchema::ID,
            ),
        ] {
            assert_eq!(value.expect("serialize schema marker"), expected);
        }
    }
}

#[cfg(test)]
mod inline_contract_tests;

#[cfg(test)]
mod inline_layout_contract_tests;

#[cfg(test)]
mod definition_contract_tests;
