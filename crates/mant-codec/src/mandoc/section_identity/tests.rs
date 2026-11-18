use super::{RoffSectionHeading, RoffSectionTargets, deroff_text_fragment};

#[test]
fn deroff_matches_cvs_per_fragment_identity_rules() {
    for escape in [
        r"\ NEXT", r"\%NEXT", r"\&NEXT", r"\0NEXT", r"\^NEXT", r"\|NEXT", r"\~NEXT",
    ] {
        assert_eq!(
            deroff_text_fragment(escape),
            Some(("NEXT", "NEXT")),
            "{escape}"
        );
    }
    assert_eq!(deroff_text_fragment(r"\&"), None);
    assert_eq!(
        deroff_text_fragment("  NEXT  \\"),
        Some(("NEXT", "NEXT  \\"))
    );
    assert_eq!(deroff_text_fragment(r"A\&B"), Some((r"A\&B", r"A\&B")));
    assert_eq!(deroff_text_fragment(r"A\zBC"), Some((r"A\zBC", r"A\zBC")));
}

#[test]
fn resolves_only_a_uniquely_qualified_section_title() {
    let mut targets = RoffSectionTargets::default();
    for (title, id) in [
        (
            "White Space Splitting (Field Splitting)",
            "white-space-splitting-field-splitting-36",
        ),
        ("Examples (basic)", "examples-basic-2"),
        ("Examples (advanced)", "examples-advanced-3"),
    ] {
        targets.register(
            RoffSectionHeading {
                title: title.to_owned(),
                native_fragment: None,
            },
            id.to_owned(),
        );
    }
    assert_eq!(
        targets.resolve("White Space Splitting"),
        Some("white-space-splitting-field-splitting-36")
    );
    assert_eq!(targets.resolve("Examples"), None);
}
