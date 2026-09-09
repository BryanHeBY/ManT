//! Shared semantic roles, without terminal colors or widget dependencies.
use mant_ir::EntryKind;

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

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::ParameterKind;

    #[test]
    fn all_nine_kinds_select_their_palette_family() {
        let kinds = [
            (
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                EntryTone::Parameter,
            ),
            (
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Marker,
                },
                EntryTone::Parameter,
            ),
            (
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Operand,
                },
                EntryTone::Parameter,
            ),
            (EntryKind::Command, EntryTone::Command),
            (EntryKind::EnvironmentVariable, EntryTone::Environment),
            (EntryKind::ConfigurationKey, EntryTone::Configuration),
            (EntryKind::Variable, EntryTone::Variable),
            (EntryKind::Value, EntryTone::Value),
            (EntryKind::Term, EntryTone::Primary),
        ];
        for (kind, tone) in kinds {
            assert_eq!(entry_tone(kind), tone);
        }
    }

    #[test]
    fn conservative_terms_and_values_keep_distinct_tones() {
        assert_eq!(entry_tone(EntryKind::Term), EntryTone::Primary);
        assert_eq!(entry_tone(EntryKind::Value), EntryTone::Value);
    }
}
