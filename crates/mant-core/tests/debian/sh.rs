//! Regression tests for Debian's `sh(1)` alias of the mdoc-formatted dash page.

use mant_ast::{Block, SourceFormat};

use crate::common::{self, block_slice_text};
use crate::fixtures::debian_manual;

/// Literal mdoc displays retain parser-classified word and delimiter spacing.
#[test]
fn preserves_literal_display_spacing_and_closing_delimiters() {
    let document = debian_manual("sh");
    assert_eq!(document.source.format, SourceFormat::Mdoc);
    assert!(
        document
            .source
            .path
            .as_deref()
            .is_some_and(|path| path.ends_with("debian/sh.1.gz")),
    );

    let functions = block_slice_text(&common::section(document, "Functions").blocks);
    assert!(functions.contains("name () command"));
    assert!(functions.contains("local [variable | -] ..."));
    assert!(functions.contains("return [exitstatus]"));
    assert!(!functions.contains("name()command"));
    assert!(!functions.contains("return [exitstatus\n"));

    let redirections = common::section(document, "Redirections");
    assert!(redirections.blocks.iter().any(|block| {
        matches!(block, Block::Preformatted { children, .. }
            if common::inline_text(children) == "[n] redir-op file")
    }));
}

#[test]
fn keeps_the_real_dash_page_spacing_and_anchors_normalized() {
    let document = debian_manual("sh");
    common::assert_anchor_ids_are_clean("debian/sh", document);
    common::assert_no_duplicate_vertical_spacing(&document.sections, "debian/sh");
}
