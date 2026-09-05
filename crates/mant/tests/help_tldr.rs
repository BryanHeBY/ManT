//! Guard the shipped static help against drift from the authoritative manual.

#[path = "support/help_tldr_generation.rs"]
mod help_tldr_generation;

#[test]
fn embedded_help_matches_the_self_manual() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manual = std::fs::read_to_string(root.join("docs/manuals/mant.md")).unwrap();
    let generated =
        std::fs::read_to_string(root.join("crates/mant/src/arguments/help_tldr_generated.rs"))
            .unwrap();
    assert_eq!(
        generated,
        help_tldr_generation::generate(&manual),
        "run cargo run --locked -p mant --example generate_help_tldr",
    );
}
