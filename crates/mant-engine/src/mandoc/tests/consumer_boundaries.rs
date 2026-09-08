//! Independent source probes for word events and structural consumers.
use super::inline_boundaries::{assert_flow, variants};
use super::*;

#[test]
fn decoded_literal_font_spellings_are_content_in_every_font() {
    for (macro_name, escape) in [("Sy", "B"), ("Em", "I"), ("Li", "C"), ("No", "R")] {
        for slash in [r"\e", r"\[rs]"] {
            for input in variants(&format!(".{macro_name} before{slash}f{escape}after")) {
                assert_flow(&input, &format!("before\\f{escape}after"));
            }
        }
    }
    let query = crate::query_roff_bytes(b".TH PROBE 1\n.SH DESCRIPTION\n.B \\efB\n").unwrap();
    assert!(crate::render_query_text(&query).contains(r"\fB"));
    let document = parse_manual_source(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/roff/real/debian/groff_man_style.7.gz"
    )))
    .unwrap();
    struct Terms(bool);
    impl<'ir> Visit<'ir> for Terms {
        fn visit_definition_item(&mut self, item: &'ir mant_ir::DefinitionItem) {
            self.0 |= item
                .terms
                .iter()
                .any(|term| inline_text(term) == r"\fB, \fI, \fR, \fP");
            visit::walk_definition_item(self, item);
        }
    }
    let mut terms = Terms(false);
    terms.visit_document(&document);
    assert!(
        terms.0,
        "real groff font-escape definition lost literal content"
    );
}
