//! Complete retained real sources; selected facts remain source-bound.

use mant_ir::{Block, DefinitionItem, ResolvedContent, visit::Visit};
use std::path::PathBuf;

pub const BPF_LITERAL: &str =
    "struct bpf_dltlist {\n        u_int bfl_len;\n        u_int *bfl_list;\n};";
pub const TMUX_LAST: &str = "the name for a new group containing only the new session.";
pub const TMUX_AFTER: &str = "-n and shell-command are invalid if -t is used.";

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
struct Items<'a>(Vec<&'a DefinitionItem>);

impl<'a> Visit<'a> for Items<'a> {
    fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
        self.0.push(item);
        mant_ir::visit::walk_definition_item(self, item);
    }
}

pub fn definition_at(content: &ResolvedContent, line: u32) -> &DefinitionItem {
    let mut items = Items::default();
    items.visit_document(content.document.as_ref().unwrap());
    let found = items
        .0
        .into_iter()
        .filter(|item| item.source.is_some_and(|source| source.line == line))
        .collect::<Vec<_>>();
    assert_eq!(found.len(), 1, "original definition source {line}");
    found[0]
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
