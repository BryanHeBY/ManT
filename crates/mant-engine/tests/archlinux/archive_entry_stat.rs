//! Regression coverage for libarchive's mdoc function synopsis.

use mant_ir::Block;

use crate::{
    common::{self, inline_text},
    fixtures::archlinux_manual,
};

#[test]
fn keeps_include_and_function_declarations_independently_addressable() {
    let document = archlinux_manual("archive_entry_stat");
    let synopsis = common::section(document, "SYNOPSIS");
    let paragraphs = synopsis
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            // Fixed CVS places one vertical row between include and function
            // declarations and between successive declarations.
            Block::VerticalSpace { lines: 1, .. } => None,
            block => panic!("expected a declaration boundary, got {block:?}"),
        })
        .collect::<Vec<_>>();

    assert_eq!(paragraphs.len(), 31);
    assert_eq!(paragraphs[0], "#include <archive_entry.h>");
    assert_eq!(
        paragraphs[1],
        "const struct stat *\narchive_entry_stat(struct archive_entry *a);"
    );
    assert_eq!(
        paragraphs[2],
        "void\narchive_entry_copy_stat(struct archive_entry *a, const struct stat *sb);"
    );
    assert_eq!(
        paragraphs.last().map(String::as_str),
        Some("void\narchive_entry_set_rdevminor(struct archive_entry *a, dev_t minor);")
    );
    assert!(
        paragraphs
            .iter()
            .skip(1)
            .all(|declaration| declaration.ends_with(");"))
    );
}

#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup(
        "archlinux/archive_entry_stat",
        archlinux_manual("archive_entry_stat"),
    );
}
