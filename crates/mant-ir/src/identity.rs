//! Strong document-local identities used by navigation and semantic lookup.

use std::{borrow::Borrow, fmt, ops::Deref};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Validated document-local identity shared by sections, anchors, and entries.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// Construct an ID after the lowering layer has normalized and validated it.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for NodeId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for NodeId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl AsRef<str> for NodeId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for NodeId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Deref for NodeId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq<str> for NodeId {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for NodeId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}
