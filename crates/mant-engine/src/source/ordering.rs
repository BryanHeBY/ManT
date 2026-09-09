//! Pure manual-name comparison and exact section precedence policy.

const DEFAULT_MANUAL_SECTIONS: [&str; 16] = [
    "1", "1p", "n", "l", "8", "3", "3p", "0", "0p", "2", "3type", "5", "4", "9", "6", "7",
];

pub(super) fn manual_names_equal(left: &str, right: &str) -> bool {
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

pub(super) fn manual_name_key(name: &str) -> String {
    #[cfg(windows)]
    {
        name.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        name.to_owned()
    }
}

pub(super) fn default_manual_section_order() -> Vec<String> {
    DEFAULT_MANUAL_SECTIONS.map(str::to_owned).to_vec()
}

pub(super) fn parse_manual_section_order(value: &str) -> Option<Vec<String>> {
    let mut sections = Vec::new();
    for section in value
        .split(':')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !sections.iter().any(|existing| existing == section) {
            sections.push(section.to_owned());
        }
    }
    (!sections.is_empty()).then_some(sections)
}

pub(super) fn compare_manual_sections(
    left: &str,
    right: &str,
    section_order: &[String],
) -> std::cmp::Ordering {
    let rank = |section: &str| {
        section_order
            .iter()
            .position(|candidate| candidate == section)
    };
    match (rank(left), rank(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.cmp(right),
    }
}

#[cfg(test)]
mod tests {
    use super::{compare_manual_sections, manual_name_key, manual_names_equal};

    #[test]
    fn explicit_sections_rank_exactly_and_unknown_sections_keep_lexical_order() {
        let order = vec!["3p".to_owned(), "3".to_owned()];
        let mut sections = ["5", "3type", "3", "1", "3p", "3P"];
        sections.sort_by(|left, right| compare_manual_sections(left, right, &order));
        assert_eq!(sections, ["3p", "3", "1", "3P", "3type", "5"]);
    }

    #[test]
    fn name_key_and_lookup_share_only_the_platform_ascii_case_policy() {
        for (left, right, equal) in [
            ("cargo.EXE", "cargo.exe", cfg!(windows)),
            ("Écho", "écho", false),
            ("tool", "tool-extra", false),
            ("日本", "日本", true),
        ] {
            assert_eq!(manual_names_equal(left, right), equal);
            assert_eq!(manual_name_key(left) == manual_name_key(right), equal);
        }
    }
}
