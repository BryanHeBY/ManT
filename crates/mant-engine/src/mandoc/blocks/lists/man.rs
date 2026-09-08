//! Distinct man definition/alias and ordered-item continuation lifetimes.
mod definitions;
pub(in crate::mandoc::blocks) mod ordered;
#[cfg(test)]
pub(super) use definitions::is_ip_bullet_item;
pub(in crate::mandoc::blocks) use definitions::{
    ManAliasState, ManDefinitionState, lower_man_definition,
};
