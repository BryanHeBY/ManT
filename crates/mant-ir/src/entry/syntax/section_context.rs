//! Finite section-title syntax-family hints shared by Flow and Fixed.
//!
//! A title is only a local classifier hint. A nearer recognized heading
//! overrides an inherited family; an unrecognized heading inherits it.

/// A bounded declaration family inferred from a complete section title.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionDeclarationFamily {
    Parameters,
    Commands,
    EnvironmentVariables,
    Variables,
    ConfigurationKeys,
}

/// Select one local family without reading a page or guessing from body prose.
/// The ordering preserves Flow's existing `DefinitionContext::for_section`
/// policy, including specific OPTIONS/FLAGS precedence over ENVIRONMENT.
#[doc(hidden)]
#[must_use]
pub fn section_declaration_family(title: &str) -> Option<SectionDeclarationFamily> {
    use SectionDeclarationFamily as Family;
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
    if words.contains(&"OPTIONS")
        || words.contains(&"OPTION")
        || words.contains(&"SWITCHES")
        || words.contains(&"FLAGS")
    {
        return Some(Family::Parameters);
    }
    if matches!(words.last(), Some(&"COMMANDS"))
        || matches!(words.as_slice(), ["COMMAND", "DESCRIPTIONS"])
    {
        return Some(Family::Commands);
    }
    if words.contains(&"ENVIRONMENT") || words.contains(&"ENVIRONMENTS") {
        return Some(Family::EnvironmentVariables);
    }
    if words.contains(&"VARIABLES") || words.contains(&"VARIABLE") {
        return Some(Family::Variables);
    }
    if matches!(words.as_slice(), ["COMMAND" | "COMMANDS" | "BUILTINS"])
        || words
            .windows(2)
            .any(|pair| matches!(pair, ["BUILTIN", "COMMAND" | "COMMANDS"]))
        || words
            .iter()
            .any(|word| matches!(*word, "SUBCOMMAND" | "SUBCOMMANDS"))
    {
        return Some(Family::Commands);
    }
    if normalized.contains("CONFIGURATION") || normalized.trim() == "KEYWORDS" {
        return Some(Family::ConfigurationKeys);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{SectionDeclarationFamily as Family, section_declaration_family};

    #[test]
    fn environment_hint_is_casefolded_and_specific_options_override_it() {
        // The exact .SH Environment/.SS OPTIONS input ran through pinned CVS
        // before this assertion; man_term.c::pre_SH/pre_SS preserve distinct
        // native heading scopes. Category selection remains ManT policy.
        assert_eq!(
            section_declaration_family("Environment"),
            Some(Family::EnvironmentVariables)
        );
        assert_eq!(
            section_declaration_family("ENVIRONMENT OPTIONS"),
            Some(Family::Parameters)
        );
        assert_eq!(section_declaration_family("NOTES"), None);
    }
}
