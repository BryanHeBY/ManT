//! Regression coverage for libzip's out-of-synopsis `Fo` declaration.

use mant_ir::Block;

use crate::{
    common::{self, inline_text},
    fixtures::archlinux_manual,
};

#[test]
fn separates_multiple_operands_in_an_out_of_synopsis_function_pointer() {
    let document = archlinux_manual("zip_source_function");
    let description = common::section(document, "DESCRIPTION");
    let declarations = description
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(declarations.iter().any(|declaration| {
        declaration == "(*zip_source_callback)(void *userdata, void *data, zip_uint64_t len, zip_source_cmd_t cmd)"
    }));
}

#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup(
        "archlinux/zip_source_function",
        archlinux_manual("zip_source_function"),
    );
}
