use super::deroff_text_fragment;

#[test]
fn deroff_matches_cvs_per_fragment_identity_rules() {
    for escape in [
        r"\ NEXT", r"\%NEXT", r"\&NEXT", r"\0NEXT", r"\^NEXT", r"\|NEXT", r"\~NEXT",
    ] {
        assert_eq!(deroff_text_fragment(escape), Some("NEXT"), "{escape}");
    }
    assert_eq!(deroff_text_fragment(r"\&"), None);
    assert_eq!(deroff_text_fragment("  NEXT  \\"), Some("NEXT"));
    assert_eq!(deroff_text_fragment(r"A\&B"), Some(r"A\&B"));
    assert_eq!(deroff_text_fragment(r"A\zBC"), Some(r"A\zBC"));
}
