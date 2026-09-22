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
    struct TextCollector<'store> {
        content: mant_ir::ContentContext<'store>,
        output: String,
    }

    impl<'ir> Visit<'ir> for TextCollector<'ir> {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            match self.content.inline(inline) {
                Ok(mant_ir::InlineView::Text(value) | mant_ir::InlineView::Code(value)) => {
                    self.output.push_str(value);
                    self.output.push(' ');
                }
                Ok(mant_ir::InlineView::LineBreak) => self.output.push('\n'),
                _ => {}
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = TextCollector {
        content: document.content(),
        output: String::new(),
    };
    collector.visit_document(document);
    collector.output
}

fn projected_document_text(document: &mant_ir::Document) -> String {
    struct TextCollector<'store> {
        content: mant_ir::ContentContext<'store>,
        output: String,
    }

    impl<'ir> Visit<'ir> for TextCollector<'ir> {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            match self.content.inline(inline) {
                Ok(mant_ir::InlineView::Text(value) | mant_ir::InlineView::Code(value)) => {
                    self.output.push_str(value);
                }
                Ok(mant_ir::InlineView::LineBreak) => self.output.push('\n'),
                _ => {}
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = TextCollector {
        content: document.content(),
        output: String::new(),
    };
    collector.visit_document(document);
    collector.output
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

fn inline_text(content: mant_ir::ContentContext<'_>, children: &[Inline]) -> String {
    mant_ir::inline_plain_text(content, children)
}

fn link_target<'a>(
    document: &'a mant_ir::Document,
    inline: &'a Inline,
) -> Option<&'a mant_ir::LinkTarget> {
    document
        .content()
        .link(inline)
        .ok()
        .flatten()
        .map(mant_ir::LinkView::target)
}

fn leaf_text<'a>(document: &'a mant_ir::Document, inline: &'a Inline) -> Option<&'a str> {
    match document.content().inline(inline).ok()? {
        mant_ir::InlineView::Text(value) | mant_ir::InlineView::Code(value) => Some(value),
        _ => None,
    }
}

mod entries;

mod navigation;
mod tables;

mod basic_inline;
mod control_boundaries;
mod declaration_groups;
mod inline_execution;
mod semantic_links;
