use std::{fmt::Write as _, fs, process};

use mant_ir::{
    Block, DiagnosticLevel, Inline,
    visit::{self, Visit},
};

use super::parse_plain_manual as parse_manual_bytes;
use super::{LoweringContext, MAX_INLINE_EQUATION_NORMALIZATIONS, Parser, lower_mandoc_document};

// Lowering tests acquire their own plain-text fixtures, then exercise only the
// byte codec. Product IO, compression and redirect policy tests live under
// manual_input; codec tests must not import that higher-level loader.
fn parse_manual_source(
    path: &std::path::Path,
) -> Result<mant_ir::Document, Box<dyn std::error::Error>> {
    Ok(parse_manual_bytes(path, &fs::read(path)?)?)
}

mod parser_contracts;

fn temporary_source(label: &str, source: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("mant-lower-{label}-{}.1", process::id()));
    fs::write(&path, source).expect("write temporary roff fixture");
    path
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

mod navigation;
mod tables;
