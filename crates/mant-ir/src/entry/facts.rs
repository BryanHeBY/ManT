//! Source-neutral semantic facts attached to authoritative content owners.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{EntryForm, EntryKind, EntryNameBinding, NodeId, ValueDomain};

/// Facts attached to one ordinary or term-and-description item.
///
/// Facts never replace content. Indexes validate their references against that
/// owner and can omit rejected fields without deleting or reparenting it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryFacts {
    /// Exact name occurrences within explicit displayed forms.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub name_bindings: Vec<EntryNameBinding>,
    /// Explicit disjoint equivalence groups. Empty means unknown equivalence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alias_groups: Vec<Vec<String>>,
    /// A declared same-document relationship, never inherited content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_of: Option<NodeId>,
    /// Ordered owner-relative references; empty means unrecorded, not fallback.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forms: Vec<EntryForm>,
    /// Unique document-local owner identity. No extra inline anchor is required.
    pub id: NodeId,
    /// Source-neutral category, including parameter syntax families.
    pub kind: EntryKind,
    /// Lookup policy; binding validation still requires exact visible spelling.
    pub case: NameCase,
    /// Documented names. Derived selectors use only the validated complete set.
    pub names: Vec<String>,
    /// Explicit local or cross-document value space; never copied into content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_domain: Option<ValueDomain>,
}

/// Matching policy for semantic names, independent of document provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum NameCase {
    /// Exact Unicode scalar spelling.
    Sensitive,
    /// Ignore ASCII case distinctions only.
    Insensitive,
}
