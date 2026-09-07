//! Bounded semantic evidence, deliberately separate from strict navigation.
use crate::{OutlineTrail, Producer};
use mant_ir::{
    DefinitionCase, DefinitionRole, Diagnostic, DocumentAddress, Inline, NodeId, SourceSpan,
    ValueDomain,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Maximum evidence owners materialized in one page.
pub const MAX_EXPLANATION_RESULTS: u32 = 256;
/// Maximum matching owners indexed per document before reporting incomplete recall.
pub const MAX_EXPLANATION_CANDIDATES: usize = 10_000;
/// Maximum serialized content bytes copied into one response page.
pub const MAX_EXPLANATION_CONTENT_BYTES: u32 = 4 * 1024 * 1024;
/// Maximum explicit relationship edges followed per document.
pub const MAX_EXPLANATION_RELATIONS: usize = 4096;
/// Maximum edges in one returned relationship chain.
pub const MAX_EXPLANATION_RELATION_DEPTH: usize = 32;

/// Request-local semantic pagination and materialization controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationOptions {
    /// Maximum evidence records on this page (default 50, maximum 256).
    #[serde(default = "default_explanation_limit")]
    #[schemars(range(min = 1, max = 256))]
    pub limit: u32,
    /// Number of matching owners skipped in deterministic source order.
    #[serde(default)]
    pub offset: u32,
    /// Aggregate UTF-8 JSON byte budget for original forms/facts and body copies.
    /// Oversized bodies are omitted atomically, with location retained for reads.
    #[serde(default = "default_explanation_content_bytes")]
    #[schemars(range(min = 1, max = 4_194_304))]
    pub content_bytes: u32,
}
impl Default for ExplanationOptions {
    fn default() -> Self {
        Self {
            limit: default_explanation_limit(),
            offset: 0,
            content_bytes: default_explanation_content_bytes(),
        }
    }
}
/// Default number of explanation records (50).
#[must_use]
pub const fn default_explanation_limit() -> u32 {
    50
}
/// Default shared forms/body budget (1 MiB).
#[must_use]
pub const fn default_explanation_content_bytes() -> u32 {
    1024 * 1024
}

/// A literal evidence request, not executable syntax or a unique selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationQuery {
    /// Documented name, complete form, exact owner coordinate, or bounded literal.
    #[schemars(length(min = 1, max = 512))]
    pub entry: String,
    /// Semantic result and content pagination, independent of MCP character paging.
    #[serde(default)]
    pub options: ExplanationOptions,
}

/// Exact marker for the independent explanation result contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ExplanationSchema {
    /// Unreleased v0.11 explanation family.
    #[serde(rename = "mant.explanation/v0.11")]
    V0Dot11,
}
impl ExplanationSchema {
    /// Serialized discriminator.
    pub const ID: &'static str = "mant.explanation/v0.11";
}

/// A normal result outcome, independent of pagination and source coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ExplanationOutcome {
    /// At least one supporting owner was found before pagination.
    Evidence,
    /// No owner was found by the bounded rules; not proof of absence of behavior.
    NoEvidence,
}

/// Why this owner is included. Multiple bases do not duplicate its content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EvidenceBasis {
    /// Exact documented name under the owner's declared case policy.
    Name,
    /// Complete authored form under the owner's declared case policy.
    Form,
    /// Literal visible content with finite token boundaries, always case-sensitive.
    Literal,
    /// Exact entry ID or structural coordinate (no alias/shorthand resolver).
    Identity,
    /// A directly matched name participates in a validated explicit alias group.
    AliasGroup {
        /// Exact member spellings; there is no canonical first member.
        members: Vec<String>,
    },
    /// Evidence connected by explicit same-document aliasOf edges.
    Related {
        /// Starting directly matched owner.
        from: NodeId,
        /// Declaration owners in traversal order, each supplying one aliasOf edge.
        declarations: Vec<NodeId>,
    },
}

// Serde's internally tagged unit variants ignore extra fields, even with
// deny_unknown_fields. Empty struct variants close the deserialization boundary
// while retaining the convenient public unit-variant API and serialized shape.
#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum ClosedEvidenceBasis {
    Name {},
    Form {},
    Literal {},
    Identity {},
    AliasGroup {
        members: Vec<String>,
    },
    Related {
        from: NodeId,
        declarations: Vec<NodeId>,
    },
}

impl<'de> Deserialize<'de> for EvidenceBasis {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match ClosedEvidenceBasis::deserialize(deserializer)? {
            ClosedEvidenceBasis::Name {} => Self::Name,
            ClosedEvidenceBasis::Form {} => Self::Form,
            ClosedEvidenceBasis::Literal {} => Self::Literal,
            ClosedEvidenceBasis::Identity {} => Self::Identity,
            ClosedEvidenceBasis::AliasGroup { members } => Self::AliasGroup { members },
            ClosedEvidenceBasis::Related { from, declarations } => {
                Self::Related { from, declarations }
            }
        })
    }
}

/// Original semantic facts and forms; no executable argument grammar is implied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationEntry {
    /// Source-neutral role.
    pub role: DefinitionRole,
    /// Matching policy for documented names/forms.
    pub case: DefinitionCase,
    /// Documented selectable names, not implicit equivalence groups.
    pub names: Vec<String>,
    /// Original visible forms projected through validated content bindings.
    pub forms: Vec<Vec<Inline>>,
    /// Explicit same-owner equivalence groups.
    pub alias_groups: Vec<Vec<String>>,
    /// Explicit independent same-document subject relation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias_of: Option<NodeId>,
    /// Local choices or remote entry set; never implicitly inherited/resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_domain: Option<ValueDomain>,
}

/// One independently addressable evidence owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationEvidence {
    /// Zero-based ordinal before result pagination.
    pub ordinal: u32,
    /// Real owner/containing section; prose is never assigned a synthetic entry.
    pub outline: OutlineTrail,
    /// IR block/item/cell coordinate for ordinary supporting content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_path: Option<String>,
    /// Original source coordinates, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
    /// All retained reasons for this owner's inclusion.
    pub bases: Vec<EvidenceBasis>,
    /// Semantic metadata, absent for prose or when its copy exceeds the budget.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<ExplanationEntry>,
    /// Original owner body, omitted atomically rather than silently clipped.
    /// Prose retains only its matched block, never an invented section/entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ExplanationContent>,
    /// Original forms/facts were too large for the remaining copy budget.
    pub details_omitted: bool,
    /// Original body was too large for the remaining copy budget.
    pub content_omitted: bool,
}

/// Original content copied for one evidence owner, not a navigation selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ExplanationContent {
    /// One original entry in a single-item list retaining its numbering/layout.
    Entry {
        /// Complete original owner, including independently addressable children.
        block: mant_ir::Block,
    },
    /// One literal-support block belonging to the reported root/section.
    Block {
        /// Original ordinary IR content, without synthetic semantic facts.
        block: mant_ir::Block,
    },
}

/// One readable document's independently collected explanation evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("$id" = "urn:mant:explanation:v0.11"))]
pub struct QueryExplanation {
    /// Exact result family.
    pub schema: ExplanationSchema,
    /// Normalized request and applied bounds.
    pub query: ExplanationQuery,
    /// Selected document label.
    pub label: String,
    /// Logical identity, absent for explicit local input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<DocumentAddress>,
    /// Parser/process provenance, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer: Option<Producer>,
    /// Evidence versus no-evidence before page slicing.
    pub outcome: ExplanationOutcome,
    /// Number of collected matching owners before pagination; a lower bound
    /// when candidate or relationship traversal is truncated.
    pub total: u32,
    /// Matching owners returned on this page.
    pub returned: u32,
    /// More already-collected owners remain after this page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    /// Independent collection, relation traversal and body-copy bounds.
    pub truncation: ExplanationTruncation,
    /// Semantic validation coverage, never a recall-completeness claim.
    pub semantics_complete: bool,
    /// Original recoverable validation/parser findings.
    pub diagnostics: Vec<Diagnostic>,
    /// Records in document source order, with independent owners never merged.
    pub evidence: Vec<ExplanationEvidence>,
}

/// Independent reasons that a bounded explanation may omit material.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationTruncation {
    /// Collection hit its matching-owner budget.
    pub candidates: bool,
    /// Relation traversal hit its edge or depth budget.
    pub relations: bool,
    /// Some selected body or details were omitted by the copy budget.
    pub content: bool,
}
