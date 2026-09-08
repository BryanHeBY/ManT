//! Guard the shipped static help against drift from the authoritative manual.

#[path = "support/help_tldr_generation.rs"]
mod help_tldr_generation;

#[test]
fn generation_has_canonical_newlines_for_both_checkout_styles() {
    let manual = "<!-- mant:tldr:start -->\n# mant\n\n- Read a manual:\n\n`mant {{command}}`\n<!-- mant:tldr:end -->\n";
    let generated = help_tldr_generation::generate(manual);
    assert!(!generated.contains('\r'));
    assert_eq!(
        generated,
        help_tldr_generation::generate(&manual.replace('\n', "\r\n")),
    );
}

#[test]
fn embedded_help_matches_the_self_manual() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manual = std::fs::read_to_string(root.join("docs/manuals/mant.md")).unwrap();
    let generated =
        std::fs::read_to_string(root.join("crates/mant/src/arguments/help_tldr_generated.rs"))
            .unwrap();
    assert_eq!(
        normalized_checkout(&generated),
        help_tldr_generation::generate(&manual),
        "run cargo run --locked -p mant --example generate_help_tldr",
    );
}

// Attributes govern future checkouts; an existing clean Windows checkout can
// still contain CRLF after the eol=lf rule is added. Ignore that representation
// difference only, without hiding content drift or changing generator policy.
fn normalized_checkout(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[test]
fn checkout_normalization_accepts_crlf_but_preserves_content_differences() {
    let canonical = include_str!("../src/arguments/help_tldr_generated.rs").replace("\r\n", "\n");
    for checkout in [canonical.clone(), canonical.replace('\n', "\r\n")] {
        assert_eq!(normalized_checkout(&checkout), canonical);
        assert_ne!(normalized_checkout(&format!("{checkout} ")), canonical);
        assert_ne!(normalized_checkout(&format!("{checkout}\r")), canonical);
        assert_ne!(
            normalized_checkout(&checkout.replacen("mant", "other", 1)),
            canonical
        );
    }
}
