//! Grammars and owner inference for semantic definitions.
mod commands;
mod decision;
mod declaration;
mod eligibility;
mod forms;
mod head;
mod inference;
mod named;
#[cfg(feature = "roff")]
mod native_subject;
mod occurrences;
mod options;
mod scan;
pub(super) use head::is_inferred_head;
pub(super) use head::is_inferred_head_with_operands;
pub(super) use head::recognize_inferred_head_with_operands;
pub(super) use inference::{InferredIdentity, infer_identity};
pub(super) use named::is_ordinal_marker;
#[cfg(test)]
pub(super) use named::is_value_name;
pub(crate) use named::{environment_variable_alias, environment_variable_body};
pub(super) use occurrences::name_occurrences;
#[cfg(test)]
pub(super) use options::option_names;
pub(crate) use options::{
    option_names_from_terms, option_occurrences_from_terms, option_prefix, slash_option_forms,
};
pub(super) use scan::DeclarationLimit;
