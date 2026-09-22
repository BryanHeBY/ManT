//! Inline content and typed navigation intent, independent of host actions.
use crate::{ContentAtomKey, ContentPointKey, ContentRef, LinkOccurrenceKey, NodeId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Inline content shared by prose, terms, and styled preformatted runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Inline {
    /// Plain visible text.
    Text {
        /// Checked slice of the authoritative logical atom.
        content: ContentRef,
    },
    /// Strongly emphasized content.
    Strong {
        /// Nested inline content.
        children: Vec<Inline>,
    },
    /// Emphasized content.
    Emphasis {
        /// Nested inline content.
        children: Vec<Inline>,
    },
    /// Literal code or symbolic token.
    Code {
        /// Checked slice of the authoritative logical atom.
        content: ContentRef,
    },
    /// A typed link whose navigation semantics are explicit in the IR.
    ///
    /// Only document and manual targets form cross-document graph edges.
    /// Section targets remain local, while external and email targets require a
    /// host action and never expand a documentation scope.
    Link {
        /// Logical occurrence shared by every structural fragment.
        occurrence: LinkOccurrenceKey,
        /// Visible linked content.
        children: Vec<Inline>,
    },
    /// A zero-width, document-local navigation destination such as mdoc `Tg`.
    ///
    /// Anchor IDs and section IDs share one namespace within a document.
    Anchor {
        /// Exact zero-width logical position.
        point: ContentPointKey,
        /// Document-local destination identity.
        id: NodeId,
        /// Exact source fragments resolving to this normalized identity.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fragment_aliases: Vec<crate::FragmentAlias>,
    },
    /// Hard line break that renderers must preserve.
    LineBreak {
        /// Authoritative linked or unlinked hard-break atom.
        atom: ContentAtomKey,
    },
}

impl Inline {
    /// Construct a normalized local anchor without source-authored aliases.
    #[must_use]
    pub fn anchor(point: ContentPointKey, id: impl Into<NodeId>) -> Self {
        Self::Anchor {
            point,
            id: id.into(),
            fragment_aliases: Vec::new(),
        }
    }

    /// Construct a normalized local anchor at its addressable source owner.
    #[must_use]
    pub fn anchor_at(point: ContentPointKey, id: impl Into<NodeId>) -> Self {
        Self::Anchor {
            point,
            id: id.into(),
            fragment_aliases: Vec::new(),
        }
    }

    /// Construct a normalized local anchor with exact source fragments.
    #[must_use]
    pub fn anchor_with_aliases(
        point: ContentPointKey,
        id: impl Into<NodeId>,
        fragment_aliases: Vec<crate::FragmentAlias>,
    ) -> Self {
        Self::Anchor {
            point,
            id: id.into(),
            fragment_aliases,
        }
    }
}

/// Typed destination kind for [`Inline::Link`].
///
/// This enum records navigation intent, not host resolution state. The query
/// engine resolves [`LinkTarget::Document`] and [`LinkTarget::Manual`] against
/// the catalog; consumers must not infer equivalent edges from rendered text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum LinkTarget {
    /// An external URI from mdoc `Lk`, man `UR`, or Markdown links.
    External {
        /// Absolute URI.
        uri: String,
    },
    /// An email address without a `mailto:` prefix.
    Email {
        /// Mailbox address without a URI scheme.
        address: String,
    },
    /// A relative Markdown link to another document in the current source.
    ///
    /// This is a logical cross-document graph edge, not a physical path.
    Document {
        /// Extension-free relative document path.
        name: String,
        /// Optional document-local destination.
        #[serde(skip_serializing_if = "Option::is_none")]
        fragment: Option<String>,
    },
    /// A typed reference to another installed manual page.
    ///
    /// This is a logical cross-document graph edge resolved through the manual
    /// catalog and its normal ambiguity rules.
    Manual {
        /// Manual topic without a section suffix.
        name: String,
        /// Native manual category, when specified by the source.
        #[serde(skip_serializing_if = "Option::is_none")]
        manual_section: Option<String>,
    },
    /// A reference to addressable content in this document, including a
    /// section, entry-backed owner, or inline anchor (such as mdoc `Sx`).
    ///
    /// The target is a canonical document-local identity in [`crate::DocumentIndex`],
    /// not a rendered heading slug or unresolved authored fragment.
    Section {
        /// Canonical target identity in the current document.
        id: NodeId,
    },
}
