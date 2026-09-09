//! Typed local document-opening data for explicitly authorized interactive hosts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::DocumentAddress;

/// An interactive request to load a local document, not permission to execute it.
///
/// An unresolved manual remains explicitly manual-only: a same-named Markdown
/// document must not shadow it, and the UI must not invent a manual section.
/// Fragment validation is performed against the loaded document before the
/// interactive host commits a navigation change. MCP never executes this action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DocumentOpenTarget {
    /// An exact already-qualified local catalog address.
    Address {
        /// Logical address, never an arbitrary filesystem path.
        address: DocumentAddress,
    },
    /// A manual topic resolved with manual-only policy by the embedding host.
    Manual {
        /// Original manual name, not its displayed link label.
        name: String,
        /// Exact requested manual section, or unresolved when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        manual_section: Option<String>,
    },
}

impl From<DocumentAddress> for DocumentOpenTarget {
    fn from(address: DocumentAddress) -> Self {
        Self::Address { address }
    }
}
