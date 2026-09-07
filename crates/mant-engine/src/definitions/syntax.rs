//! Source-neutral grammars for semantic definition names.
//!
//! These recognizers deliberately consume complete authored forms. Section
//! context decides which grammar to try; this module only decides whether a
//! spelling is trustworthy enough to expose as an addressable entry.

use mant_ir::{DefinitionCase, DefinitionItem, DefinitionRole};

use crate::inline::plain_text;

use super::context::DefinitionContext;

mod commands;
mod forms;
mod named;
mod options;
use commands::command_names;
pub(super) use named::environment_names_from_terms;
pub(super) use named::is_value_name;
use named::{environment_names, is_configuration_key, is_variable_term, named_term};
pub(crate) use named::{environment_variable_alias, environment_variable_body};
#[cfg(test)]
pub(super) use options::option_names;
use options::parameter_identity;
pub(crate) use options::{option_names_from_terms, option_prefix, slash_option_forms};

pub(super) fn infer_identity(
    item: &DefinitionItem,
    context: DefinitionContext,
) -> (DefinitionRole, DefinitionCase, Vec<String>) {
    let first = item
        .terms
        .first()
        .map_or_else(String::new, |term| plain_text(term));
    let trimmed = first.trim();
    match context {
        DefinitionContext::Commands
            if trimmed.starts_with(['-', '+']) || trimmed.starts_with("[-+]") =>
        {
            parameter_identity(item, trimmed)
        }
        DefinitionContext::Commands => {
            let names = command_names(item);
            if names.is_empty() {
                (DefinitionRole::Term, DefinitionCase::Sensitive, Vec::new())
            } else {
                (DefinitionRole::Command, DefinitionCase::Sensitive, names)
            }
        }
        DefinitionContext::EnvironmentVariables => named_identity(
            DefinitionRole::EnvironmentVariable,
            DefinitionCase::Sensitive,
            environment_names(item),
        ),
        DefinitionContext::Variables => named_identity(
            DefinitionRole::Variable,
            DefinitionCase::Sensitive,
            named_term(item, is_variable_term),
        ),
        DefinitionContext::ConfigurationKeys => named_identity(
            DefinitionRole::ConfigurationKey,
            DefinitionCase::Insensitive,
            named_term(item, is_configuration_key),
        ),
        DefinitionContext::Values => named_identity(
            DefinitionRole::Value,
            DefinitionCase::Sensitive,
            named_term(item, is_value_name),
        ),
        DefinitionContext::Parameters => parameter_identity(item, trimmed),
        DefinitionContext::Generic if trimmed.starts_with('-') => parameter_identity(item, trimmed),
        DefinitionContext::Generic => (DefinitionRole::Term, DefinitionCase::Sensitive, Vec::new()),
    }
}

fn named_identity(
    role: DefinitionRole,
    case: DefinitionCase,
    names: Vec<String>,
) -> (DefinitionRole, DefinitionCase, Vec<String>) {
    if names.is_empty() {
        (DefinitionRole::Term, DefinitionCase::Sensitive, names)
    } else {
        (role, case, names)
    }
}
