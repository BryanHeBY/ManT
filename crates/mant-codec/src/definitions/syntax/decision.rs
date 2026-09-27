//! Deterministic declaration-role precedence, independent of name availability.
use super::option_prefix;
use crate::definitions::{NativeHeadRole, context::DefinitionContext};
use mant_ir::{EntryKind, NameCase, ParameterKind};

pub(super) fn select_kind(
    head: &str,
    context: DefinitionContext,
    hint: Option<NativeHeadRole>,
    native_numeric_option: bool,
    checked_configuration_name: bool,
) -> (EntryKind, NameCase) {
    match hint {
        Some(
            NativeHeadRole::LiteralTerm
            | NativeHeadRole::Presentation
            | NativeHeadRole::DefinedVariable,
        ) => {
            return (EntryKind::Term, NameCase::Sensitive);
        }
        Some(NativeHeadRole::Option) => {
            return (
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                NameCase::Sensitive,
            );
        }
        Some(NativeHeadRole::Environment) => {
            return (EntryKind::EnvironmentVariable, NameCase::Sensitive);
        }
        Some(NativeHeadRole::Variable) => {
            return (EntryKind::Variable, NameCase::Sensitive);
        }
        Some(NativeHeadRole::Literal) if matches!(head, "-" | "--") => {
            return (EntryKind::Term, NameCase::Sensitive);
        }
        Some(NativeHeadRole::Literal) | None => {}
    }
    if hint == Some(NativeHeadRole::Literal)
        && matches!(
            context,
            DefinitionContext::ConfigurationKeys | DefinitionContext::RootConfigurationKeys
        )
        && checked_configuration_name
    {
        return (EntryKind::ConfigurationKey, NameCase::Sensitive);
    }
    if matches!(
        context,
        DefinitionContext::ConfigurationKeys | DefinitionContext::RootConfigurationKeys
    ) && checked_configuration_name
    {
        return (EntryKind::ConfigurationKey, NameCase::Sensitive);
    }
    if hint == Some(NativeHeadRole::Literal)
        && context == DefinitionContext::Generic
        && head.contains(" [-")
        && head
            .split_whitespace()
            .next()
            .is_some_and(super::commands::is_command_name)
    {
        return (EntryKind::Command, NameCase::Sensitive);
    }
    // A short digit is ordinarily a signed argument, but an independently
    // executed, finally bold native head may prove it as an option. The
    // caller supplies this only after the shared name scan has bound `-4` or
    // `-6` to the leading visible glyphs; the textual spelling alone cannot.
    if native_numeric_option || local_option_spelling(head) {
        return (
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            },
            NameCase::Sensitive,
        );
    }
    // A documented all-caps variable-like symbol nested below an option is
    // not thereby an accepted option value. Without Ev/environment context,
    // preserve it as a named Term rather than assert exported-variable status.
    if context == DefinitionContext::Values
        && head.contains('_')
        && head
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        return (EntryKind::Term, NameCase::Sensitive);
    }
    inherited_kind(head, context)
}

/// A section/parent default is only a hint; native evidence can override it.
fn inherited_kind(trimmed: &str, context: DefinitionContext) -> (EntryKind, NameCase) {
    let parameter = || EntryKind::Parameter {
        parameter_kind: match trimmed {
            "--" | "--%" => ParameterKind::Marker,
            "-" => ParameterKind::Operand,
            _ => ParameterKind::Option,
        },
    };
    match context {
        DefinitionContext::Commands
            if matches!(trimmed, "-" | "--" | "--%")
                || trimmed.starts_with(['+'])
                || trimmed.starts_with("[-+]") =>
        {
            (parameter(), NameCase::Sensitive)
        }
        DefinitionContext::Commands => (EntryKind::Command, NameCase::Sensitive),
        DefinitionContext::EnvironmentVariables => {
            (EntryKind::EnvironmentVariable, NameCase::Sensitive)
        }
        DefinitionContext::Variables => (EntryKind::Variable, NameCase::Sensitive),
        DefinitionContext::Parameters => (parameter(), NameCase::Sensitive),
        DefinitionContext::Generic if matches!(trimmed, "-" | "--" | "--%") => {
            (parameter(), NameCase::Sensitive)
        }
        // A parent option or key only locates a possible value scope. Native
        // inferred values and weak configuration hints need local declaration
        // proof; explicit Value facts remain untouched by this inference.
        DefinitionContext::ConfigurationKeys
        | DefinitionContext::Values
        | DefinitionContext::RootConfigurationKeys
        | DefinitionContext::Generic => (EntryKind::Term, NameCase::Sensitive),
    }
}

/// Local complete dash spelling overrides a weak inherited category. A
/// negative number is not a flag, and an arbitrary dash in prose is not a head.
fn local_option_spelling(text: &str) -> bool {
    let Some(token) = text.split_whitespace().next() else {
        return false;
    };
    if token.starts_with('-')
        && !token.starts_with("--")
        && token.chars().nth(1).is_some_and(|c| c.is_ascii_digit())
    {
        return false;
    }
    option_prefix(token).is_some()
}
