//! Page-local original context, explicitly distinct from alias evidence.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Original content supporting directly matched declarations. IDs are indices
/// in the containing document response's pool, never persistent identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ExplanationSupport {
    /// Recovered adjacency supplies reading context, not proof that each
    /// sentence applies to every member or that members are interchangeable.
    DeclarationGroup {
        /// Exact containing list in the queried document snapshot.
        block_path: String,
        /// Half-open range in the original list, before response slicing.
        group: mant_ir::DeclarationGroup,
        /// Original members in source order; the last supplies the description.
        members: Vec<crate::OutlineTrail>,
        /// Complete original heads and final description. Local group indices
        /// are rebased; original item sources and identities remain unchanged.
        block: mant_ir::Block,
    },
}
