//! Distinct man definition/alias and ordered-item continuation lifetimes.
mod definitions;
mod markers;
pub(in crate::mandoc::blocks) mod ordered;
pub(in crate::mandoc::blocks) use definitions::{ManDefinitionState, lower_man_definition};
pub(in crate::mandoc::blocks) use markers::{IpRun, adjacent_ip_run};
