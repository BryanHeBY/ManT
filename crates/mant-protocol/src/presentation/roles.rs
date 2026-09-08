//! Shared semantic roles, without terminal colors or widget dependencies.
use mant_ir::{EntryKind, ParameterKind};

/// Palette family for a semantic entry, never importance or confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryTone {
    /// Ordinary primary foreground, including conservative generic terms.
    Primary,
    /// An option, marker or operand; the original kind remains authoritative.
    Parameter,
    /// A callable command.
    Command,
    /// An environment variable.
    Environment,
    /// A configuration key.
    Configuration,
    /// A non-environment variable.
    Variable,
    /// A documented value.
    Value,
}

/// Select a palette family without erasing the caller's full entry kind.
#[must_use]
pub const fn entry_tone(kind: EntryKind) -> EntryTone {
    match kind {
        EntryKind::Parameter { .. } => EntryTone::Parameter,
        EntryKind::Command => EntryTone::Command,
        EntryKind::EnvironmentVariable => EntryTone::Environment,
        EntryKind::ConfigurationKey => EntryTone::Configuration,
        EntryKind::Variable => EntryTone::Variable,
        EntryKind::Value => EntryTone::Value,
        EntryKind::Term => EntryTone::Primary,
    }
}

/// Stable human label, distinct from Rust Debug and from wire discriminators.
#[must_use]
pub const fn entry_kind_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option,
        } => "option",
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Marker,
        } => "marker",
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Operand,
        } => "operand",
        EntryKind::Command => "command",
        EntryKind::EnvironmentVariable => "environment variable",
        EntryKind::ConfigurationKey => "configuration key",
        EntryKind::Variable => "variable",
        EntryKind::Value => "value",
        EntryKind::Term => "term",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_nine_kinds_have_labels_without_conflating_parameters() {
        let kinds = [
            (
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                "option",
            ),
            (
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Marker,
                },
                "marker",
            ),
            (
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Operand,
                },
                "operand",
            ),
            (EntryKind::Command, "command"),
            (EntryKind::EnvironmentVariable, "environment variable"),
            (EntryKind::ConfigurationKey, "configuration key"),
            (EntryKind::Variable, "variable"),
            (EntryKind::Value, "value"),
            (EntryKind::Term, "term"),
        ];
        for (kind, label) in kinds {
            assert_eq!(entry_kind_label(kind), label);
        }
        assert_eq!(entry_tone(EntryKind::Term), EntryTone::Primary);
        assert_eq!(entry_tone(EntryKind::Value), EntryTone::Value);
    }
}
