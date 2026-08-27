#![doc = include_str!("README.md")]
#![warn(missing_docs)]

//! Private upstream-regression support for `mantdoc`.
//!
//! The public surface is intentionally a small facade: corpus access lives in
//! [`corpus`], canonical projection in [`canonical`], and the archive-backed
//! parser gates in [`gates`]. Keeping those concerns separate lets the example
//! tools depend only on the capability they exercise.

pub mod canonical;
pub mod corpus;
mod coverage;
mod gates;

#[allow(unused_imports)]
pub use canonical::{
    CANONICAL_AST_SCHEMA, CANONICAL_DIAGNOSTIC_SCHEMA, CANONICAL_MDOC_OPERATING_SYSTEM,
    CanonicalDiagnostic, CanonicalDifference, CanonicalDocument, CanonicalEnclosure,
    CanonicalFlags, CanonicalLocation, CanonicalMetadata, CanonicalNode, CanonicalParse,
    CanonicalTableCell, canonicalize_mantdoc, first_difference,
};
#[allow(unused_imports)]
pub use corpus::{
    CorpusArchiveError, CorpusArchiveErrorKind, CorpusCase, CorpusCasePayload, CorpusInventory,
    ReferenceOutput, ReferenceOutputPayload, RendererCasePayload, stable_1_14_6_case,
    stable_1_14_6_inventory, stable_1_14_6_reference_output, stable_1_14_6_renderer_case,
};
#[allow(unused_imports)]
pub use gates::*;
