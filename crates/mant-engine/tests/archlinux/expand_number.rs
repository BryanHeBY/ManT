//! Regression coverage for libbsd's multi-operand mdoc `Fa` declaration.

use mant_ir::Block;

use crate::{
    common::{self, inline_text},
    fixtures::archlinux_manual,
};

#[test]
fn separates_multiple_operands_from_one_fa_invocation() {
    let document = archlinux_manual("expand_number");
    let synopsis = common::section(document, "SYNOPSIS");
    let declarations = synopsis
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(
        declarations
            .iter()
            .any(|declaration| declaration == "int expand_number(const char *buf, uint64_t *num);")
    );
}

#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup(
        "archlinux/expand_number",
        archlinux_manual("expand_number"),
    );
}
