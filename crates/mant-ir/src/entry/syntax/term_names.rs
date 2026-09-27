//! Whole-label names for an independently established definition owner.
//!
//! These spellings do not create an owner. PP/RS candidates need stronger
//! structure and declaration evidence; TP/It definitions can bind a complete
//! surviving technical label without guessing aliases from its words.

use std::ops::Range;

/// Exact callable name interval in a complete `name(section)` manual label.
/// This is a ManT declaration rule, not an inferred native hyperlink.
#[doc(hidden)]
#[must_use]
pub fn manual_call_name_range(text: &str) -> Option<Range<usize>> {
    let start = text.len() - text.trim_start().len();
    let label = text.trim();
    let (name, section) = label.strip_suffix(')')?.rsplit_once('(')?;
    if name.contains(['(', ')'])
        || !crate::is_inferred_manual_topic(name)
        || section == "0"
        || !crate::is_manual_section(section)
    {
        return None;
    }
    Some(start..start.checked_add(name.len())?)
}

/// Narrow callable syntax usable by a generic definition owner. It does not
/// treat a command-line option or bracketed command usage as a generic Term
/// name; those forms require command context or native role evidence.
#[doc(hidden)]
#[must_use]
pub fn generic_callable_name_range(text: &str) -> Option<Range<usize>> {
    if let Some(range) = manual_call_name_range(text) {
        return Some(range);
    }
    let start = text.len() - text.trim_start().len();
    let label = text.trim();
    let name = placeholder_invocation_name(label).or_else(|| sigilled_invocation_name(label))?;
    Some(start..start.checked_add(name.len())?)
}

/// The callable name in one complete command declaration. A PP/RS layout
/// candidate must pass this grammar independently of its bold appearance;
/// an explicit TP head may use the same range without inventing a name from
/// its argument. This is ManT policy, not a semantic role assigned by mandoc.
#[doc(hidden)]
#[must_use]
pub fn command_declaration_name_range(text: &str) -> Option<Range<usize>> {
    if let Some(range) = generic_callable_name_range(text) {
        return Some(range);
    }
    let start = text.len() - text.trim_start().len();
    let label = text.trim();
    let name = bound_command_name(label).or_else(|| structured_invocation_name(label))?;
    Some(start..start.checked_add(name.len())?)
}

/// A syntax-bearing invocation has a bounded, balanced argument suffix. A
/// heading and a bold word do not establish this fact; in particular ordinary
/// prose following a word cannot become a command name. CVS `man_term.c`
/// `pre_TP/pre_B` only establish the displayed head and font, not this role.
fn structured_invocation_name(label: &str) -> Option<&str> {
    let (name, suffix) = label.split_once(char::is_whitespace)?;
    let suffix = suffix.trim_start();
    if name == "{" {
        return complete_invocation_suffix(suffix, Some('}')).then_some(name);
    }
    let mut characters = name.chars();
    if !characters
        .next()
        .is_some_and(|character| character.is_ascii_lowercase())
        || !characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        || !suffix.starts_with(['-', '+', '/', '<', '[', '{'])
    {
        return None;
    }
    complete_invocation_suffix(suffix, None).then_some(name)
}

fn complete_invocation_suffix(suffix: &str, initial_closer: Option<char>) -> bool {
    let mut closers = ['\0'; 64];
    let mut depth = 0;
    let mut at_token_start = initial_closer.is_none();
    if let Some(closer) = initial_closer {
        closers[0] = closer;
        depth = 1;
    }
    for character in suffix.chars() {
        if character.is_control() {
            return false;
        }
        if character.is_whitespace() {
            if depth == 0 {
                at_token_start = true;
            }
            continue;
        }
        if at_token_start && depth == 0 {
            if !matches!(character, '-' | '+' | '/' | '<' | '[' | '{') {
                return false;
            }
            at_token_start = false;
        }
        match character {
            '[' | '{' | '<' | '(' => {
                if depth == closers.len() {
                    return false;
                }
                closers[depth] = match character {
                    '[' => ']',
                    '{' => '}',
                    '<' => '>',
                    _ => ')',
                };
                depth += 1;
            }
            ']' | '}' | '>' | ')' => {
                if depth == 0 || closers[depth - 1] != character {
                    return false;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    !at_token_start && depth == 0
}

fn placeholder_invocation_name(label: &str) -> Option<&str> {
    let (name, arguments) = label.split_once(char::is_whitespace)?;
    (super::environment_names::is_variable_term(name)
        && name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase())
        && !arguments.trim().is_empty()
        && arguments.split_whitespace().all(|token| {
            !token.is_empty()
                && token.split(',').all(|part| {
                    !part.is_empty()
                        && !part.starts_with('-')
                        && part.chars().all(|character| {
                            character.is_ascii_uppercase()
                                || character.is_ascii_digit()
                                || matches!(character, '_' | '-')
                        })
                })
        }))
    .then_some(name)
}

fn sigilled_invocation_name(label: &str) -> Option<&str> {
    let (name, arguments) = label.split_once('(')?;
    let arguments = arguments.strip_suffix(')')?;
    if name.is_empty()
        || name.contains(char::is_whitespace)
        || !super::environment_names::is_variable_term(name)
        || !name.chars().any(char::is_lowercase)
    {
        return None;
    }
    let arguments = arguments.trim();
    if arguments.is_empty() {
        return Some(name);
    }
    let mut count = 0usize;
    for argument in arguments.split(',') {
        count += 1;
        if count > 64 {
            return None;
        }
        let variable = argument.trim().strip_prefix(['$', '@', '%'])?;
        if variable.is_empty()
            || !variable
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
            || !variable
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return None;
        }
    }
    Some(name)
}

fn bound_command_name(label: &str) -> Option<&str> {
    let split = label
        .char_indices()
        .find(|(_, character)| character.is_whitespace());
    let (name, suffix) = split.map_or((label, ""), |(index, _)| {
        (&label[..index], label[index..].trim())
    });
    let mut characters = name.chars();
    if !characters
        .next()
        .is_some_and(|character| character.is_ascii_lowercase())
        || !characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return None;
    }
    let binding = suffix.strip_prefix('(')?.strip_suffix(')')?.trim();
    complete_key_binding(binding).then_some(name)
}

fn complete_key_binding(binding: &str) -> bool {
    let mut groups = 0;
    let mut parts = binding.split(',').peekable();
    while let Some(group) = parts.next() {
        groups += 1;
        if groups > 64 {
            return false;
        }
        if group.trim() == "..." {
            return groups > 1 && parts.peek().is_none();
        }
        let mut tokens = group.split_whitespace().peekable();
        if groups == 1 && tokens.peek() == Some(&"usually") {
            tokens.next();
        }
        let mut keys = 0;
        for token in tokens {
            keys += 1;
            if keys > 4 || !key_binding_atom(token, keys > 1) {
                return false;
            }
        }
        if keys == 0 {
            return false;
        }
    }
    groups > 0
}

fn key_binding_atom(token: &str, continuation: bool) -> bool {
    if matches!(
        token,
        "TAB" | "Return" | "Newline" | "Rubout" | "ESC" | "<space>"
    ) {
        return true;
    }
    let mut key = token;
    let mut modifiers = 0;
    while let Some(rest) = key.strip_prefix("C-").or_else(|| key.strip_prefix("M-")) {
        modifiers += 1;
        if modifiers > 2 {
            return false;
        }
        key = rest;
    }
    (modifiers > 0 || continuation)
        && (key == "<space>"
            || key.chars().count() == 1
                && key
                    .chars()
                    .all(|character| !character.is_whitespace() && !character.is_control()))
}

/// A pure list marker, not a semantic subject even in a definition-shaped
/// physical owner. The ordinal grammar is shared with Flow's existing rule.
#[doc(hidden)]
#[must_use]
pub fn is_presentation_term(text: &str) -> bool {
    let label = text.trim();
    is_ordinal_marker(label) || matches!(label, "•" | "∙" | "*" | "-" | "–" | "—")
}

/// Original Flow ordinal marker grammar, shared with Fixed read-time proof.
#[doc(hidden)]
#[must_use]
pub fn is_ordinal_marker(value: &str) -> bool {
    let value = value.trim();
    let digits = if let Some(digits) = value.strip_suffix('.') {
        Some(digits)
    } else if let Some(digits) = value.strip_suffix(')') {
        Some(digits.strip_prefix('(').unwrap_or(digits))
    } else {
        value
            .strip_prefix('[')
            .and_then(|digits| digits.strip_suffix(']'))
    };
    digits.is_some_and(|digits| {
        !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit())
    })
}

/// Bind the complete visible spelling of a real definition as one Term name.
/// Punctuation and spaces inside a diagnostic or glossary subject remain part
/// of that name; neither supplies extra aliases or partial word selectors.
#[doc(hidden)]
#[must_use]
pub fn complete_term_label_range(text: &str) -> Option<Range<usize>> {
    let start = text.len() - text.trim_start().len();
    let label = text.trim();
    // A standalone command-line marker may be a declared option/operand;
    // once followed by a placeholder, however, the whole visible invocation
    // is not a single Term name. Keep the physical head readable without
    // laundering a failed marker declaration into a semantic selector.
    let marker_with_operand = label
        .split_once(char::is_whitespace)
        .is_some_and(|(prefix, _)| matches!(prefix, "-" | "--" | "--%"));
    // An anchored regular-expression label denotes a family rather than an
    // exact subject. Keep its physical definition/form searchable, but do
    // not make the pattern itself a Term name. This is intentionally narrower
    // than rejecting every asterisk or caret in diagnostic/technical terms.
    let anchored_pattern = label.starts_with('^') && (label.ends_with('$') || label.contains(".*"));
    if label.is_empty()
        || label.contains(['\r', '\n'])
        || is_presentation_term(label)
        || marker_with_operand
        || anchored_pattern
    {
        return None;
    }
    Some(start..start.checked_add(label.len())?)
}

#[cfg(test)]
mod tests {
    use super::{
        command_declaration_name_range, complete_term_label_range, is_presentation_term,
        manual_call_name_range,
    };

    #[test]
    fn manual_call_and_full_term_are_distinct_name_proofs() {
        // Exact E03/E03b/E04 inputs first ran pinned CVS -Tutf8; in E03b
        // man_html.c::man_PP_pre/man_RS_pre emitted only paragraph/div markup.
        assert_eq!(manual_call_name_range(" git-add(1) "), Some(1..8));
        assert_eq!(manual_call_name_range("Note"), None);
        assert_eq!(manual_call_name_range("git-add(foo)"), None);
        // Exact PP/RS inputs ran pinned CVS -Tutf8 first. man_html.c emits
        // only paragraph/div for these labels; a path, URL, mail-like label,
        // or section 0 is insufficient inferred catalog evidence.
        for value in [
            "https://example.test(1)",
            "/tmp/tool(1)",
            "user@tool(1)",
            "tool(0)",
        ] {
            assert_eq!(manual_call_name_range(value), None, "{value}");
        }
        assert_eq!(complete_term_label_range("working tree"), Some(0..12));
        // Exact .TP/.B marker-plus-FILE inputs ran pinned CVS -Tutf8
        // -Owidth=78 first; man_macro.c::blk_imp keeps their physical HEAD,
        // while man_term.c::pre_TP/pre_B render both words without licensing
        // the pair as one semantic Term name.
        for label in ["- FILE", "-- FILE", "--% FILE"] {
            assert_eq!(complete_term_label_range(label), None, "{label}");
        }
        // These exact .TP/.B labels also ran pinned CVS first. Formatter
        // output preserves their spelling; exact-name eligibility is a
        // separate ManT decision, and ^C remains an ordinary technical term.
        for pattern in ["^find-new.*", "^exact$"] {
            assert_eq!(complete_term_label_range(pattern), None, "{pattern}");
        }
        assert_eq!(complete_term_label_range("^C"), Some(0..2));
        assert_eq!(
            complete_term_label_range("file: not in gzip format"),
            Some(0..24)
        );
        assert!(is_presentation_term("1."));
        assert!(is_presentation_term("•"));
    }

    #[test]
    fn complete_command_syntax_exposes_only_its_callable_name() {
        // Exact COMMANDS PP/RS inputs ran pinned CVS -Tutf8 -Owidth=78
        // before these assertions. man_term.c::pre_PP/pre_RS provide only
        // layout; the complete invocation/binding is ManT's local proof.
        assert_eq!(command_declaration_name_range("run FILE"), Some(0..3));
        assert_eq!(
            command_declaration_name_range(" backward-char (C-b) "),
            Some(1..14)
        );
        // The real pinned sh(1) fixture ran pinned CVS -Ttree first;
        // man_term.c::pre_B prints these full Readline binding labels.
        assert_eq!(
            command_declaration_name_range("do-lowercase-version (M-A, M-B, M-x, ...)"),
            Some(0..20)
        );
        assert_eq!(
            command_declaration_name_range("end-of-file (usually C-d)"),
            Some(0..11)
        );
        assert_eq!(
            command_declaration_name_range("set-mark (C-@, M-<space>)"),
            Some(0..8)
        );
        assert_eq!(command_declaration_name_range("git-add(1)"), Some(0..7));
        // Exact TP/B and mdoc It/Sy forms ran pinned CVS -Ttree first.
        // man_term.c::pre_TP/pre_B and mdoc_term.c::termp_it_pre preserve
        // these balanced invocation suffixes without assigning their role.
        for (label, end) in [
            ("find-new <subvolume> <last_gen>", 8),
            ("launch -p [-x]", 6),
            ("set [ {+|-}options | {+|-}o [ option_name ] ]", 3),
            ("snapshot <source> <dest>|[<dest>/]<name>", 8),
            ("resize [<devid>:]max|<size>", 6),
            ("show <path>|<uuid>", 4),
            ("{ list ;}", 1),
        ] {
            assert_eq!(
                command_declaration_name_range(label),
                Some(0..end),
                "{label}"
            );
        }
        for label in [
            "Note",
            "run",
            "run ",
            "run file",
            "run FILE prose",
            "backward-char (word)",
            "name (C-b prose)",
            "name (usually C-b nonsense)",
            "Note PLEASE",
            "Router examples",
            "router_begin note",
            "start option|other",
            "launch -p [-x",
        ] {
            assert_eq!(command_declaration_name_range(label), None, "{label}");
        }
    }
}
