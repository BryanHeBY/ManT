//! Complete visible configuration-key spelling, independent of roff macros.
//!
//! This recognizes a name range inside one already-proved declaration. It
//! does not make an arbitrary paragraph or source token into a key.

use std::ops::Range;

/// The key bytes of a complete visible key or assignment declaration.
///
/// A key may carry one adjacent assignment or optional-assignment suffix.
/// Values are retained in the displayed form but never become key names.
/// A native macro component may end at the key and leave its argument in a
/// separate component; callers bind this range to its final visible glyphs.
#[must_use]
pub fn configuration_key_declaration_range(form: &str) -> Option<Range<usize>> {
    let start = form.len() - form.trim_start().len();
    let visible = form.get(start..)?.trim_end();
    let bytes = visible.as_bytes();
    let mut end = 0;
    while let Some(&byte) = bytes.get(end) {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.') {
            end += 1;
        } else {
            break;
        }
    }
    let key = visible.get(..end)?;
    if !configuration_key_token(key) {
        return None;
    }
    let remainder = visible.get(end..)?.trim_start();
    if remainder.is_empty()
        || remainder
            .strip_prefix("[=")
            .and_then(|value| value.strip_suffix(']'))
            .is_some_and(complete_value_suffix)
        || remainder
            .strip_prefix('=')
            .is_some_and(complete_value_suffix)
    {
        return Some(start..start + end);
    }
    None
}

/// A weak PP/RS VARIABLES candidate needs more than an identifier and a
/// following indented paragraph. One complete assignment or one separate
/// angle-bracket operand supplies local declaration syntax; only the leading
/// variable becomes a selectable name. The native PP/RS structure is checked
/// independently by each producer.
#[must_use]
pub fn variable_assignment_declaration_range(form: &str) -> Option<Range<usize>> {
    let start = form.len() - form.trim_start().len();
    let visible = form.get(start..)?.trim_end();
    let end = visible
        .find(|character: char| character.is_whitespace() || character == '=')
        .unwrap_or(visible.len());
    let name = visible.get(..end)?;
    if !super::environment_names::is_variable_term(name) {
        return None;
    }
    let rest = visible.get(end..)?;
    let complete = if let Some(value) = rest.strip_prefix('=') {
        complete_value_suffix(value)
    } else {
        let parameter = rest.trim_start();
        parameter.len() < rest.len()
            && parameter
                .strip_prefix('<')
                .and_then(|value| value.strip_suffix('>'))
                .is_some_and(|value| {
                    !value.is_empty()
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                })
    };
    complete.then_some(start..start + end)
}

/// Name range of a complete variable in an explicit definition HEAD.
/// Unlike a weak PP/RS paragraph, a real TP/IP label supplies its own
/// declaration boundary, so a bare variable token is sufficient. The
/// assignment/placeholder spelling still binds only its leading name.
#[must_use]
pub fn variable_declaration_name_range(form: &str) -> Option<Range<usize>> {
    let start = form.len() - form.trim_start().len();
    let visible = form.get(start..)?.trim_end();
    if super::environment_names::is_variable_term(visible) {
        return Some(start..start.checked_add(visible.len())?);
    }
    if let Some((name, suffix)) = visible.split_once(char::is_whitespace)
        && super::environment_names::is_variable_term(name)
        && super::environment_names::annotations(suffix)
    {
        return Some(start..start.checked_add(name.len())?);
    }
    variable_assignment_declaration_range(form)
}

/// A variable-shaped but incomplete explicit HEAD must not be renamed by a
/// generic full-label Term fallback. Callers supply the VARIABLES context
/// and native definition boundary; this only checks source-neutral spelling.
#[must_use]
pub fn rejected_variable_declaration_head(form: &str) -> bool {
    let label = form.trim();
    let start = label.strip_prefix('$').unwrap_or(label);
    start
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && (label.contains(['[', ']', '=', '<', '>', '*']) || label.contains("::"))
        && variable_declaration_name_range(form).is_none()
}

/// A document-level configuration hint is weaker than a CONFIGURATION
/// section or an authored Cm component: a key needs an actual, complete value
/// assignment before an otherwise ordinary PP/RS paragraph can declare it.
#[must_use]
pub fn root_configuration_assignment_range(form: &str) -> Option<Range<usize>> {
    let range = configuration_key_declaration_range(form)?;
    form.get(range.end..)?
        .trim_start()
        .starts_with('=')
        .then_some(range)
}

/// Finite spelling of one configuration-key token, without an assignment.
#[must_use]
pub fn configuration_key_token(name: &str) -> bool {
    name.split('.').all(|component| {
        !component.is_empty()
            && component
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    }) && name
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
}

/// One visible native Ar argument component after an independently proved
/// declaration key. This validates the parameter boundary, not a selectable
/// Value name; bracketed templates such as `algorithm[,algorithm...]` are
/// legitimate Ar operands and must not be promoted as names.
#[must_use]
pub fn native_argument_component_token(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && !text
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

/// Complete visible separator between two independently executed literal
/// declaration components. An empty gap means native instances were glued
/// into one displayed word; punctuation inside a parameter is not examined
/// here because the caller must first exclude its native argument ranges.
#[must_use]
pub fn complete_literal_component_gap(gap: &str) -> bool {
    !gap.is_empty() && matches!(gap.trim(), "" | "," | "|")
}

fn complete_value_suffix(value: &str) -> bool {
    if value.is_empty()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte == 0)
    {
        return false;
    }
    let mut closers = Vec::new();
    for character in value.chars() {
        match character {
            '[' | '(' | '{' | '<' => {
                if closers.len() == 64 {
                    return false;
                }
                closers.push(match character {
                    '[' => ']',
                    '(' => ')',
                    '{' => '}',
                    _ => '>',
                });
            }
            ']' | ')' | '}' | '>' if closers.pop() != Some(character) => return false,
            _ if character.is_control() => return false,
            _ => {}
        }
    }
    closers.is_empty()
}

#[cfg(test)]
mod tests {
    use super::{
        complete_literal_component_gap, configuration_key_declaration_range,
        configuration_key_token, native_argument_component_token,
        rejected_variable_declaration_head, root_configuration_assignment_range,
        variable_assignment_declaration_range, variable_declaration_name_range,
    };

    #[test]
    fn only_complete_key_or_assignment_exposes_a_name() {
        // The exact `.It Cm color=[yes|no]` input was rendered with pinned
        // CVS -Tutf8 first. mdoc_macro.c::blk_full retains its one It HEAD;
        // mdoc_term.c::termp_it_pre/termp_bold_pre print the entire form.
        assert_eq!(
            configuration_key_declaration_range("color=[yes|no]"),
            Some(0..5)
        );
        assert_eq!(
            configuration_key_declaration_range(" BatchMode "),
            Some(1..10)
        );
        assert_eq!(
            configuration_key_declaration_range("Log.Level=warn"),
            Some(0..9)
        );
        for form in ["color=[yes|no", "color=", "foo bar", "--help", "a..b=on"] {
            assert_eq!(configuration_key_declaration_range(form), None, "{form}");
        }
        assert!(configuration_key_token("BatchMode"));
        assert!(!configuration_key_token("A..B"));
        // Exact SSH_CONFIG `.It Cm HostKeyAlgorithms Ar
        // algorithm[,algorithm...]` ran pinned CVS -Tutf8 first. Ar's
        // operand spelling is a parameter, not an inferred Value selector.
        assert!(native_argument_component_token("algorithm[,algorithm...]"));
        assert!(!native_argument_component_token("two words"));
        assert!(complete_literal_component_gap(", "));
        assert!(complete_literal_component_gap(" "));
        assert!(!complete_literal_component_gap(""));
        assert!(!complete_literal_component_gap("first,--fake,last,"));
    }

    #[test]
    fn weak_pp_rs_names_require_complete_local_declaration_syntax() {
        // This exact SSH_CONFIG PP/RS matrix ran pinned CVS -Tutf8
        // -Owidth=78 before assertions. man_term.c::pre_PP/pre_RS establish
        // paragraphs and indentation, not ManT semantic entry roles.
        assert_eq!(variable_assignment_declaration_range("foo=bar"), Some(0..3));
        assert_eq!(
            variable_assignment_declaration_range(" foo <S> "),
            Some(1..4)
        );
        for form in ["foo", "foo=", "<K><S>"] {
            assert_eq!(variable_assignment_declaration_range(form), None, "{form}");
        }
        assert_eq!(
            root_configuration_assignment_range("BatchMode=yes"),
            Some(0..9)
        );
        assert_eq!(
            root_configuration_assignment_range("core.editor=vim"),
            Some(0..11)
        );
        for form in ["core.editor", "BatchMode"] {
            assert_eq!(root_configuration_assignment_range(form), None, "{form}");
        }
        assert_eq!(configuration_key_declaration_range("local.key"), Some(0..9));
    }

    #[test]
    fn explicit_variable_head_accepts_only_complete_token_or_assignment() {
        // Exact TP and IP inputs for all four spellings ran pinned CVS
        // -Tutf8 -Owidth=78 first. man_macro.c::blk_imp gives each an
        // authored HEAD; man_term.c::pre_TP/pre_IP execute the visible tag.
        for token in ["foo", "FOO[bar]", "$FOO[_index]"] {
            assert_eq!(
                variable_declaration_name_range(token),
                Some(0..token.len()),
                "{token}"
            );
            assert_eq!(
                variable_assignment_declaration_range(token),
                None,
                "{token}"
            );
        }
        assert_eq!(variable_declaration_name_range("foo=bar"), Some(0..3));
        // Exact TP/It labels first ran pinned CVS -Ttree; the annotation is
        // displayed but the selector remains the complete leading variable.
        for (form, end) in [
            ("APPEND_HISTORY <K> <S>", 14),
            ("vi-cmd-mode-string ((cmd))", 18),
            ("isearch-terminators (C-[C-j)", 19),
        ] {
            assert_eq!(
                variable_declaration_name_range(form),
                Some(0..end),
                "{form}"
            );
        }
        assert_eq!(variable_declaration_name_range("FOO[bar"), None);
        // Each exact TP/IP FOO:: and $FOO:: input ran pinned CVS -Tutf8
        // -Owidth=78 first. The visible HEAD survives; the incomplete
        // qualifier is only rejected as a semantic selector.
        for label in ["FOO[bar", "FOO::", "$FOO::"] {
            assert!(rejected_variable_declaration_head(label), "{label}");
        }
        for label in ["foo", "FOO[bar]", "$FOO[_index]", "foo=bar"] {
            assert!(!rejected_variable_declaration_head(label), "{label}");
        }
    }
}
