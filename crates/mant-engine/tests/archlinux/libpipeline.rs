//! Regression coverage for libpipeline's paragraph-owned function targets.

use mant_ir::{
    Inline,
    visit::{self, Visit},
};

use crate::fixtures::archlinux_manual;

#[test]
fn preserves_function_targets_moved_to_paragraphs() {
    struct Anchors(Vec<(String, Vec<String>)>);

    impl<'ir> Visit<'ir> for Anchors {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } = inline
            {
                self.0.push((
                    id.to_string(),
                    fragment_aliases.iter().map(ToString::to_string).collect(),
                ));
            }
            visit::walk_inline(self, inline);
        }
    }

    let document = archlinux_manual("libpipeline");
    let mut anchors = Anchors(Vec::new());
    anchors.visit_document(document);

    // Fixed CVS HTML was checked before changing this assertion: the
    // authored fragments are `pipecmd_new_sequence` and `pipeline_want_out`.
    // Native projection keeps those exact external spellings as aliases and
    // isolates ManT's canonical identity in the target namespace.
    for (target, authored) in [
        ("target-pipecmd-new-sequence", "pipecmd_new_sequence"),
        ("target-pipeline-want-out", "pipeline_want_out"),
    ] {
        assert!(
            anchors.0.iter().any(|(anchor, aliases)| {
                anchor == target && aliases.iter().any(|alias| alias == authored)
            }),
            "missing paragraph-owned function target {target}"
        );
    }
}
