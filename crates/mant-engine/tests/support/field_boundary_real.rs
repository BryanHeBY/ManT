//! Borrow complete source owners without copying or flattening their topology.

use mant_ir::{Block, ResolvedContent, visit::Visit};
use std::path::PathBuf;

pub fn load(file: &str) -> ResolvedContent {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real")
        .join(file);
    ResolvedContent {
        address: None,
        label: file.into(),
        document: Some(mant_loader::parse_manual_source(&path).unwrap()),
        tldr: None,
    }
}

#[derive(Default)]
struct Blocks<'a>(Vec<&'a Block>);

impl<'a> Visit<'a> for Blocks<'a> {
    fn visit_block(&mut self, block: &'a Block) {
        self.0.push(block);
        mant_ir::visit::walk_block(self, block);
    }
}

pub fn blocks(content: &ResolvedContent) -> Vec<&Block> {
    let mut result = Blocks::default();
    result.visit_document(content.document.as_ref().unwrap());
    result.0
}
