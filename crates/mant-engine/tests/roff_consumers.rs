//! Complete corpus regressions stay outside the published crate source set.
#[allow(dead_code)]
mod common;
use mant_ir::{
    DefinitionItem,
    visit::{self, Visit},
};

#[test]
fn real_groff_font_escape_definition_preserves_all_four_literal_terms() {
    struct Terms(bool);
    impl<'ir> Visit<'ir> for Terms {
        fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
            self.0 |= item
                .terms
                .iter()
                .any(|term| common::inline_text(term) == r"\fB, \fI, \fR, \fP");
            visit::walk_definition_item(self, item);
        }
    }
    let document = mant_engine::parse_manual_source(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/roff/real/debian/groff_man_style.7.gz"
    )))
    .unwrap();
    let mut terms = Terms(false);
    terms.visit_document(&document);
    assert!(
        terms.0,
        "real groff font-escape definition lost literal content"
    );
}
