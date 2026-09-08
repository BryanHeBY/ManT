use std::{collections::HashSet, fmt::Write as _, fs, process};

use mant_ir::{
    Block, DiagnosticLevel, Inline, ListKind, ResolvedContent, SemanticIndex, SourceFormat,
    ValueDomain,
    visit::{self, Visit},
};

use super::{
    LoweringContext, MAX_INLINE_EQUATION_NORMALIZATIONS, Parser, lower_mandoc_document,
    parse_manual_bytes, parse_manual_source,
};

mod consumer_boundaries;
mod entry_forms;
mod flow_controls;
mod font_boundaries;
mod glyphs;
mod inline_boundaries;
mod parser_contracts;
mod upstream_inline;
mod upstream_tables;

fn temporary_source(label: &str, source: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("mant-lower-{label}-{}.1", process::id()));
    fs::write(&path, source).expect("write temporary roff fixture");
    path
}

fn anchor_ids(document: &mant_ir::Document) -> Vec<String> {
    struct AnchorCollector(Vec<String>);

    impl<'ir> Visit<'ir> for AnchorCollector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor { id, .. } = inline {
                self.0.push(id.to_string());
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = AnchorCollector(Vec::new());
    collector.visit_document(document);
    collector.0
}

fn anchor_owner_lines(document: &mant_ir::Document) -> Vec<(String, u32)> {
    struct AnchorCollector(Vec<(String, u32)>);

    impl<'ir> Visit<'ir> for AnchorCollector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor {
                id,
                owner_source: Some(source),
                ..
            } = inline
            {
                self.0.push((id.to_string(), source.line));
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = AnchorCollector(Vec::new());
    collector.visit_document(document);
    collector.0
}

fn visible_document_text(document: &mant_ir::Document) -> String {
    struct TextCollector(String);

    impl<'ir> Visit<'ir> for TextCollector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            match inline {
                Inline::Text { value } | Inline::Code { value } => {
                    self.0.push_str(value);
                    self.0.push(' ');
                }
                Inline::LineBreak => self.0.push('\n'),
                Inline::Strong { .. }
                | Inline::Emphasis { .. }
                | Inline::Link { .. }
                | Inline::Anchor { .. } => {}
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = TextCollector(String::new());
    collector.visit_document(document);
    collector.0
}

fn find_macro_mut<'a>(
    node: &'a mut libmandoc_rs::Node,
    name: &str,
) -> Option<&'a mut libmandoc_rs::Node> {
    if node.macro_name.as_deref() == Some(name) {
        return Some(node);
    }
    node.children
        .iter_mut()
        .find_map(|child| find_macro_mut(child, name))
}

fn replace_first_text(node: &mut libmandoc_rs::Node, value: &str) -> bool {
    if let Some(text) = node.text.as_mut() {
        *text = value.to_owned();
        return true;
    }
    node.children
        .iter_mut()
        .any(|child| replace_first_text(child, value))
}

fn inline_text(children: &[Inline]) -> String {
    children
        .iter()
        .map(|child| match child {
            Inline::Text { value } | Inline::Code { value } => value.clone(),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => inline_text(children),
            Inline::Anchor { .. } => String::new(),
            Inline::LineBreak => "\n".to_owned(),
        })
        .collect()
}

mod entries;
mod layout;
mod navigation;
mod tables;
