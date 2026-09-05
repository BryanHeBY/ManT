//! Regenerate the embedded CLI quick reference with the production TLDR parser.

#[path = "../tests/support/help_tldr_generation.rs"]
mod help_tldr_generation;

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manual = std::fs::read_to_string(root.join("docs/manuals/mant.md"))
        .expect("read repository self manual");
    std::fs::write(
        root.join("crates/mant/src/arguments/help_tldr_generated.rs"),
        help_tldr_generation::generate(&manual),
    )
    .expect("write generated CLI quick reference");
}
