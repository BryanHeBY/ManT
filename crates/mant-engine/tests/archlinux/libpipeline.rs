//! Regression coverage for libpipeline's paragraph-owned function targets.

use mant_ir::{
    Inline,
    visit::{self, Visit},
};

use crate::fixtures::archlinux_manual;

#[test]
fn preserves_function_targets_moved_to_paragraphs() {
    struct Anchors(Vec<String>);

    impl<'ir> Visit<'ir> for Anchors {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor { id } = inline {
                self.0.push(id.to_string());
            }
            visit::walk_inline(self, inline);
        }
    }

    let document = archlinux_manual("libpipeline");
    let mut anchors = Anchors(Vec::new());
    anchors.visit_document(document);

    for target in ["pipecmd-new-sequence", "pipeline-want-out"] {
        assert!(
            anchors.0.iter().any(|anchor| anchor == target),
            "missing paragraph-owned function target {target}"
        );
    }
}
