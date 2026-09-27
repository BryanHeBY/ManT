//! Distinct man definition/alias and ordered-item continuation lifetimes.
mod definitions;
pub(in crate::mandoc::blocks) mod ordered;
pub(in crate::mandoc::blocks) use definitions::{
    IpRun, ManDefinitionState, adjacent_ip_run, lower_man_definition,
};
