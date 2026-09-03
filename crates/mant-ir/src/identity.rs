//! Strong document-local identities used by navigation and semantic lookup.

use std::{borrow::Borrow, fmt, ops::Deref};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Virtual identity used for content before the first section heading.
pub const DOCUMENT_ROOT_ID: &str = "document-overview";

/// Exact source-authored fragment spelling for one local navigation target.
///
/// A fragment alias is not a [`NodeId`]: formats such as mdoc deliberately
/// permit stable external destinations containing uppercase letters or
/// punctuation. Producers retain that spelling here while assigning the
/// target a separate normalized internal identity. Shared validation rejects
/// empty, whitespace-containing, and control-containing aliases.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct FragmentAlias(String);

impl FragmentAlias {
    /// Borrow the exact source-authored fragment spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the alias and return its exact source spelling.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for FragmentAlias {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for FragmentAlias {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl AsRef<str> for FragmentAlias {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for FragmentAlias {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Deref for FragmentAlias {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for FragmentAlias {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Document-local identity shared by sections, anchors, and entries.
///
/// Lowering layers normalize these values; [`crate::validate_document`] reports
/// any invalid value introduced through deserialization or custom producers.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// Construct an ID after the lowering layer has normalized it.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the normalized identity string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the identity and return its owned string.
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
