//! Definition context policy; coordinated by the parent discovery passes.
use crate::inline::plain_text;
use mant_ir::{DefinitionItem, EntryKind};

/// Only semantic topology changes inherited context; visual indentation does not.
pub(super) fn definition_group_context(
    items: &[DefinitionItem],
    context: DefinitionContext,
) -> DefinitionContext {
    if context == DefinitionContext::Generic && is_key_binding_command_group(items) {
        DefinitionContext::Commands
    } else {
        context
    }
}

pub(super) fn child_definition_context(
    role: EntryKind,
    item_context: DefinitionContext,
) -> DefinitionContext {
    match role {
        EntryKind::Command => DefinitionContext::Parameters,
        EntryKind::Parameter {
            parameter_kind:
                mant_ir::ParameterKind::Option
                | mant_ir::ParameterKind::Marker
                | mant_ir::ParameterKind::Operand,
        }
        | EntryKind::ConfigurationKey => DefinitionContext::Values,
        EntryKind::EnvironmentVariable
        | EntryKind::Variable
        | EntryKind::Value
        | EntryKind::Term => item_context,
    }
}

/// Recognize a definition group whose authored forms are editor commands and
/// optional key bindings.
///
/// Manuals such as Bash group Readline commands under topical headings like
/// "Killing and Yanking" or "Miscellaneous", so a heading-only classifier
/// cannot recover their executable names. Requiring a whole multi-item group
/// of command-name tokens plus at least one recognizable binding keeps this
/// inference narrower than treating arbitrary hyphenated glossary terms as
/// commands.
fn is_key_binding_command_group(items: &[DefinitionItem]) -> bool {
    items.len() > 1
        && items.iter().all(|item| {
            item.terms
                .first()
                .is_some_and(|term| key_binding_command_form(&plain_text(term)).is_some())
        })
        && items.iter().any(|item| {
            item.terms.first().is_some_and(|term| {
                key_binding_command_form(&plain_text(term))
                    .is_some_and(|(_, binding)| binding.is_some())
            })
        })
}

pub(super) fn key_binding_command_form(value: &str) -> Option<(&str, Option<&str>)> {
    let value = value.trim();
    let split = value
        .char_indices()
        .find(|(_, character)| character.is_whitespace());
    let (name, suffix) = split.map_or((value, ""), |(index, _)| {
        (&value[..index], value[index..].trim())
    });
    let mut characters = name.chars();
    if !characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
        || !characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return None;
    }
    if suffix.is_empty() {
        return Some((name, None));
    }
    let binding = suffix.strip_prefix('(')?.strip_suffix(')')?.trim();
    (!binding.is_empty() && looks_like_key_binding(binding)).then_some((name, Some(binding)))
}

fn looks_like_key_binding(value: &str) -> bool {
    value
        .split([',', ' '])
        .filter(|part| !part.is_empty() && *part != "usually" && *part != "...")
        .any(|part| {
            part.starts_with("C-")
                || part.starts_with("M-")
                || matches!(
                    part,
                    "TAB" | "Return" | "Newline" | "Rubout" | "ESC" | "<space>"
                )
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DefinitionContext {
    Generic,
    Parameters,
    Commands,
    EnvironmentVariables,
    Variables,
    ConfigurationKeys,
    Values,
}

impl DefinitionContext {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Parameters => "parameter",
            Self::Commands => "command",
            Self::EnvironmentVariables => "environment-variable",
            Self::Variables => "variable",
            Self::ConfigurationKeys => "configuration-key",
            Self::Values => "value",
        }
    }

    pub(super) fn for_section(title: &str, inherited: Self) -> Self {
        let normalized = title
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_uppercase()
                } else {
                    ' '
                }
            })
            .collect::<String>();
        let words = normalized.split_whitespace().collect::<Vec<_>>();
        // Composite headings describe the more specific syntax family.  In
        // particular, "ENVIRONMENT OPTIONS" documents command-line options
        // whose defaults happen to come from the environment; it is not a
        // declaration list of environment-variable names.
        if words.contains(&"OPTIONS")
            || words.contains(&"OPTION")
            || words.contains(&"SWITCHES")
            || words.contains(&"FLAGS")
        {
            return Self::Parameters;
        }
        // The final family word disambiguates "Environment Commands" from
        // "Command Environment" without matching every mention of command.
        if matches!(words.last(), Some(&"COMMANDS"))
            || matches!(words.as_slice(), ["COMMAND", "DESCRIPTIONS"])
        {
            return Self::Commands;
        }
        if words.contains(&"ENVIRONMENT") || words.contains(&"ENVIRONMENTS") {
            return Self::EnvironmentVariables;
        }
        if words.contains(&"VARIABLES") || words.contains(&"VARIABLE") {
            return Self::Variables;
        }
        if matches!(words.as_slice(), ["COMMAND" | "COMMANDS" | "BUILTINS"])
            || words
                .windows(2)
                .any(|pair| matches!(pair, ["BUILTIN", "COMMAND" | "COMMANDS"]))
            || words
                .iter()
                .any(|word| matches!(*word, "SUBCOMMAND" | "SUBCOMMANDS"))
        {
            return Self::Commands;
        }
        if normalized.contains("CONFIGURATION") || normalized.trim() == "KEYWORDS" {
            return Self::ConfigurationKeys;
        }
        inherited
    }
}

#[cfg(test)]
mod tests {
    use super::DefinitionContext as Context;

    #[test]
    fn command_context_uses_complete_words_and_preserves_specific_priority() {
        for heading in [
            "COMMAND",
            "COMMANDS",
            "SUBCOMMAND",
            "SUBCOMMANDS",
            "SHELL BUILTIN COMMAND",
            "SHELL BUILTIN COMMANDS",
            "Available Subcommands",
        ] {
            assert_eq!(
                Context::for_section(heading, Context::Generic),
                Context::Commands,
                "{heading}"
            );
        }
        for heading in [
            "SUBCOMMANDER",
            "SUBCOMMANDSET",
            "BUILTIN COMMANDSET",
            "COMMAND LINE",
            "TERMS",
        ] {
            assert_eq!(
                Context::for_section(heading, Context::Generic),
                Context::Generic,
                "{heading}"
            );
        }
        for heading in [
            "SUBCOMMAND OPTIONS",
            "COMMAND OPTION",
            "ENVIRONMENT OPTIONS",
        ] {
            assert_eq!(
                Context::for_section(heading, Context::Generic),
                Context::Parameters,
                "{heading}"
            );
        }
        assert_eq!(
            Context::for_section("COMMAND ENVIRONMENT", Context::Generic),
            Context::EnvironmentVariables
        );
        assert_eq!(
            Context::for_section("SUBCOMMAND VARIABLES", Context::Generic),
            Context::Variables
        );
    }
}
