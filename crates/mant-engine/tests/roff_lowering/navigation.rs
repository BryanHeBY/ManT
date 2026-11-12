//! Existing regressions grouped by navigation behavior; expected values remain independent.
use super::*;

#[path = "navigation/content.rs"]
mod content;

#[path = "navigation/references.rs"]
mod references;

#[path = "navigation/target_identity.rs"]
mod target_identity;

#[path = "navigation/target_ownership.rs"]
mod target_ownership;
