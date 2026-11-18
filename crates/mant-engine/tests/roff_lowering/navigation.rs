//! Existing regressions grouped by navigation behavior; expected values remain independent.
use super::*;

#[path = "navigation/content.rs"]
mod content;

#[path = "navigation/contracts.rs"]
mod contracts;

#[path = "navigation/references.rs"]
mod references;

#[path = "navigation/section_qualifiers.rs"]
mod section_qualifiers;

#[path = "navigation/target_identity.rs"]
mod target_identity;

#[path = "navigation/target_ownership.rs"]
mod target_ownership;

#[path = "navigation/target_successors.rs"]
mod target_successors;

#[path = "navigation/target_structural.rs"]
mod target_structural;

#[path = "navigation/target_structural_owners.rs"]
mod target_structural_owners;
