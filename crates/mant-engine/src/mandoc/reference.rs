//! Conservative recognition shared by explicit and generated manual links.

/// One conventional manual reference immediately before a legacy Sphinx
/// empty destination marker.
pub(super) struct TrailingManualReference<'a> {
    pub(super) prefix: &'a str,
    pub(super) display: &'a str,
    pub(super) name: &'a str,
    pub(super) manual_section: &'a str,
}

/// Recognize the visible label immediately before Sphinx's exact ` \%<>`
/// marker. The caller has already retained the formatter-level marker as a
/// typed event, so this function never guesses from a bare `name(section)`.
pub(super) fn trailing_sphinx_manual_reference(value: &str) -> Option<TrailingManualReference<'_>> {
    let candidate = value.strip_suffix(' ')?;
    let closing = candidate.strip_suffix(')')?.len();
    let opening = candidate[..closing].rfind('(')?;
    let manual_section = &candidate[opening + 1..closing];
    if !is_manual_section(manual_section) {
        return None;
    }

    let name_start = candidate[..opening]
        .char_indices()
        .rev()
        .find(|(_, character)| !is_manual_reference_name_character(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    let name = &candidate[name_start..opening];
    let prefix = &candidate[..name_start];
    if !is_manual_reference_name(name)
        || prefix
            .chars()
            .next_back()
            .is_some_and(|character| matches!(character, '/' | '\\' | '@'))
    {
        return None;
    }

    Some(TrailingManualReference {
        prefix,
        display: &candidate[name_start..],
        name,
        manual_section,
    })
}

pub(super) fn is_manual_section(section: &str) -> bool {
    if section.len() > 16 {
        return false;
    }
    let mut characters = section.chars();
    match characters.next() {
        Some('1'..='9') => characters.all(|character| character.is_ascii_alphanumeric()),
        Some('l' | 'n') => characters.next().is_none(),
        _ => false,
    }
}

pub(super) fn is_manual_reference_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 256 && name.chars().all(is_manual_reference_name_character)
}

fn is_manual_reference_name_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '+' | ':' | '-')
}

#[cfg(test)]
mod tests {
    use super::trailing_sphinx_manual_reference;

    #[test]
    fn recognizes_safe_trailing_manual_labels() {
        for (source, name, section) in [
            ("See btrfs-subvolume(8) ", "btrfs-subvolume", "8"),
            ("printf(3p) ", "printf", "3p"),
            ("Tcl(n) ", "Tcl", "n"),
            ("systemd.slice(5) ", "systemd.slice", "5"),
            ("g++(1) ", "g++", "1"),
        ] {
            let reference = trailing_sphinx_manual_reference(source).expect("manual reference");
            assert_eq!(reference.name, name);
            assert_eq!(reference.manual_section, section);
        }
    }

    #[test]
    fn rejects_ambiguous_or_unsafe_trailing_labels() {
        for source in [
            "group(qgroup) ",
            "function(0) ",
            "/tmp/tool(1) ",
            "user@tool(1) ",
            "tool(1)",
        ] {
            assert!(
                trailing_sphinx_manual_reference(source).is_none(),
                "unexpected reference: {source:?}"
            );
        }
    }
}
