//! Stable business labels, independent of styling or terminal presentation.

use mant_ir::{EntryKind, ParameterKind};

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
    }
}
