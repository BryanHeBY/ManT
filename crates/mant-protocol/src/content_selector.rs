//! Explicit local-content selectors, never semantic lookup or link activation.

use std::{fmt, str::FromStr};

use mant_ir::{NodeId, OutlinePath, is_normalized_node_id};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::NodePath;

/// One exact local owner in the document actually loaded for this operation.
///
/// Paths are snapshot-relative: after edits a still-valid path can denote a
/// different owner. Neither variant provides cross-call stale detection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ContentSelector {
    /// Exact canonical structural path, including `root` and tldr path `0`.
    Path {
        /// A path returned by the selected document's outline.
        #[schemars(length(min = 1, max = 512))]
        path: NodePath,
    },
    /// Exact canonical content ID, not an authored fragment or semantic name.
    Id {
        /// An ID returned by the selected document's outline.
        #[schemars(length(min = 1, max = 512))]
        id: NodeId,
    },
}

impl<'de> Deserialize<'de> for ContentSelector {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
        enum Wire {
            Path { path: NodePath },
            Id { id: NodeId },
        }
        let selector = match Wire::deserialize(deserializer)? {
            Wire::Path { path } => Self::Path { path },
            Wire::Id { id } => Self::Id { id },
        };
        selector.validate().map_err(serde::de::Error::custom)?;
        Ok(selector)
    }
}

/// A malformed, noncanonical or oversized explicit content selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidContentSelector;

impl fmt::Display for InvalidContentSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "use path:<canonical outline path> or id:<canonical content ID> (maximum 512 bytes)",
        )
    }
}

impl std::error::Error for InvalidContentSelector {}

impl ContentSelector {
    /// Select a trusted projected structural path. Public callers must validate
    /// user input before execution even when constructing the enum directly.
    #[must_use]
    pub fn path(value: impl Into<NodePath>) -> Self {
        Self::Path { path: value.into() }
    }

    /// Select an exact content identity without resolving it.
    #[must_use]
    pub fn id(value: impl Into<NodeId>) -> Self {
        Self::Id { id: value.into() }
    }

    /// The selected value without a CLI prefix; its variant remains authoritative.
    #[must_use]
    pub fn value(&self) -> &str {
        match self {
            Self::Path { path } => path.as_str(),
            Self::Id { id } => id.as_str(),
        }
    }

    /// Validate before resolution; there is no trimming, normalization or fallback.
    ///
    /// # Errors
    /// Returns an error for invalid syntax, control characters or oversized input.
    pub fn validate(&self) -> Result<(), InvalidContentSelector> {
        let value = self.value();
        if value.is_empty() || value.len() > 512 {
            return Err(InvalidContentSelector);
        }
        let valid = match self {
            Self::Path { .. } => value
                .parse::<OutlinePath>()
                .is_ok_and(|path| path.to_string() == value),
            Self::Id { .. } => is_normalized_node_id(value),
        };
        valid.then_some(()).ok_or(InvalidContentSelector)
    }
}

impl FromStr for ContentSelector {
    type Err = InvalidContentSelector;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // Reject before cloning or parsing attacker-controlled path components.
        if value.len() > 517 {
            return Err(InvalidContentSelector);
        }
        let selected = if let Some(id) = value.strip_prefix("id:") {
            Self::id(id)
        } else {
            Self::path(value.strip_prefix("path:").unwrap_or(value))
        };
        selected.validate()?;
        Ok(selected)
    }
}

impl fmt::Display for ContentSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path { path } => write!(f, "path:{path}"),
            Self::Id { id } => write!(f, "id:{id}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_namespaces_and_closed_wire_have_no_name_or_link_fallback() {
        for value in ["root", "0", "1.2/e3", "path:1.2/e3", "id:option-x"] {
            assert!(value.parse::<ContentSelector>().is_ok(), "{value}");
        }
        for value in [
            "option-x",
            "--help",
            "#topic",
            "https://example.com",
            " 1 ",
            "01",
            "path:topic",
            "id:Mixed.Target",
        ] {
            assert!(value.parse::<ContentSelector>().is_err(), "{value}");
        }
        assert_ne!(ContentSelector::path("root"), ContentSelector::id("root"));
        for value in [
            r#""1""#,
            r#"{"kind":"path","path":"1","future":true}"#,
            r#"{"kind":"name","name":"--help"}"#,
        ] {
            assert!(serde_json::from_str::<ContentSelector>(value).is_err());
        }
        for value in [
            serde_json::json!({"kind":"path","path":""}),
            serde_json::json!({"kind":"path","path":"01"}),
            serde_json::json!({"kind":"id","id":"Mixed.Target"}),
            serde_json::json!({"kind":"id","id":"x\n"}),
            serde_json::json!({"kind":"id","id":"x".repeat(513)}),
        ] {
            assert!(serde_json::from_value::<ContentSelector>(value).is_err());
        }
        let selected = ContentSelector::id("option-x");
        assert_eq!(
            serde_json::from_str::<ContentSelector>(&serde_json::to_string(&selected).unwrap())
                .unwrap(),
            selected
        );
        assert!(ContentSelector::path("1.".repeat(1000)).validate().is_err());
    }
}
