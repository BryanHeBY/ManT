use super::*;
use mant_ir::HeadBodyRelation;

pub(super) fn review_definition_item(body: &str) -> mant_ir::DefinitionItem {
    let source = format!(
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n{body}"
    );
    let document = parse_manual_bytes(
        std::path::Path::new("definition-physical-row-review.1"),
        source.as_bytes(),
    )
    .expect("lower reviewed definition source");
    let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
        panic!("expected definition list: {document:#?}");
    };
    items[0].clone()
}

pub(super) fn emphasized_document_text(document: &mant_ir::Document) -> String {
    struct Collector(String);
    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Emphasis { children } = inline {
                self.0.push_str(&inline_text(children));
            } else {
                visit::walk_inline(self, inline);
            }
        }
    }
    let mut collector = Collector(String::new());
    collector.visit_document(document);
    collector.0
}

pub(super) fn strong_document_text(document: &mant_ir::Document) -> String {
    struct Collector(String);
    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Strong { children } = inline {
                self.0.push_str(&inline_text(children));
            } else {
                visit::walk_inline(self, inline);
            }
        }
    }
    let mut collector = Collector(String::new());
    collector.visit_document(document);
    collector.0
}

pub(super) fn document_link_targets(document: &mant_ir::Document) -> Vec<mant_ir::LinkTarget> {
    struct Collector(Vec<mant_ir::LinkTarget>);
    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link { target, .. } = inline {
                self.0.push(target.clone());
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut collector = Collector(Vec::new());
    collector.visit_document(document);
    collector.0
}

mod author_modes;
mod field_flush;
mod head_rows;
mod man_flow;
mod native_cells;
