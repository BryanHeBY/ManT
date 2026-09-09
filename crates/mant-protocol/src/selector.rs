//! Strong wire identities for selecting nodes in projected document views.

use std::{borrow::Borrow, fmt, ops::Deref};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Canonical structural address emitted for a node in a projected outline.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct NodePath(String);

impl NodePath {
    /// Wrap a canonical path produced by a trusted projection.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the canonical path string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the path and return its owned string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for NodePath {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for NodePath {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl AsRef<str> for NodePath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for NodePath {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Deref for NodePath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for NodePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq<str> for NodePath {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for NodePath {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}
