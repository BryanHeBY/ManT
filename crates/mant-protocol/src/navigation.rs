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
    /// An unresolved manual query, selected with manual-only policy by the host.
    /// A single-digit main section prefers its exact page, then that section's
    /// alphabetic extensions. A suffixed or nonnumeric section remains exact.
    /// The host returns the actual selected address for tabs and history.
    Manual {
        /// Original manual name, not its displayed link label.
        name: String,
        /// Requested section constraint, or the host's section order when absent.
        /// This does not assert that a page with this exact section exists.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        manual_section: Option<String>,
    },
}

impl From<DocumentAddress> for DocumentOpenTarget {
    fn from(address: DocumentAddress) -> Self {
        Self::Address { address }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_queries_and_exact_manual_addresses_remain_distinct_on_the_wire() {
        for section in [None, Some("1"), Some("1p"), Some("l")] {
            let query = DocumentOpenTarget::Manual {
                name: "ed".into(),
                manual_section: section.map(str::to_owned),
            };
            let wire = serde_json::to_string(&query).unwrap();
            assert_eq!(
                serde_json::from_str::<DocumentOpenTarget>(&wire).unwrap(),
                query
            );
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&wire).unwrap()["kind"],
                "manual"
            );
            if let Some(section) = section {
                let exact: DocumentOpenTarget = DocumentAddress::Manual {
                    name: "ed".into(),
                    manual_section: section.into(),
                }
                .into();
                assert_ne!(query, exact);
                let wire = serde_json::to_string(&exact).unwrap();
                assert_eq!(
                    serde_json::from_str::<DocumentOpenTarget>(&wire).unwrap(),
                    exact
                );
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&wire).unwrap()["kind"],
                    "address"
                );
            }
        }
    }
}
