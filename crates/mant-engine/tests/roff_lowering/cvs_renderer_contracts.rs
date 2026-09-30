//! Cross-layer contracts for deliberate CVS and groff behavior choices.
//!
//! These cases are intentionally executed through both the vendored native
//! renderer and `ManT`'s lowering pipeline.  Updating the CVS snapshot must not
//! silently change one layer while leaving the other layer's expectation
//! frozen.  GNU groff is documented here as the secondary comparison, not run
//! by ordinary Cargo tests.

use libmandoc_rs::{RenderFormat, Renderer};
use mant_ir::{
    Block, DefinitionItem, Document, Inline, LinkTarget,
    visit::{self, Visit},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectedContract {
    /// `ManT` deliberately follows the pinned CVS formatter instead of groff.
    Cvs,
    /// `ManT` deliberately retains the portable groff projection instead of a
    /// known CVS presentation artifact.
    Groff,
}

struct TerminalCase {
    label: &'static str,
    source: &'static str,
    native_contains: &'static [&'static str],
    lowered_contains: &'static [&'static str],
    selected: SelectedContract,
}

struct StrongText(bool);

struct ExactTailInline(Option<Inline>);

struct AuthoredSectionLink {
    id: &'static str,
    found: bool,
}

impl<'ir> Visit<'ir> for StrongText {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Strong { children } = inline
            && super::inline_text(children) == "TAIL"
        {
            self.0 = true;
        }
        visit::walk_inline(self, inline);
    }
}

impl<'ir> Visit<'ir> for ExactTailInline {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if self.0.is_none() && mant_ir::inline_plain_text(std::slice::from_ref(inline)) == "TAIL" {
            self.0 = Some(inline.clone());
            return;
        }
        visit::walk_inline(self, inline);
    }
}

impl<'ir> Visit<'ir> for AuthoredSectionLink {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if matches!(
            inline,
            Inline::Link {
                target: LinkTarget::Section { id },
                ..
            } if id.as_str() == self.id
        ) {
            self.found = true;
        }
        visit::walk_inline(self, inline);
    }
}

fn native_terminal(source: &str) -> String {
    apply_terminal_backspaces(&native_terminal_raw(source))
}

fn native_terminal_raw(source: &str) -> String {
    Renderer::new(RenderFormat::Utf8)
        .with_width(80)
        .render_bytes("contract.1", source.as_bytes())
        .expect("render the pinned native CVS contract")
        .output
}

fn apply_terminal_backspaces(output: &str) -> String {
    let mut projected = String::with_capacity(output.len());
    for character in output.chars() {
        if character == '\u{8}' {
            projected.pop();
        } else {
            projected.push(character);
        }
    }
    projected
}

fn lowered_terminal(source: &str) -> String {
    let query = mant_loader::load_roff_bytes(source.as_bytes())
        .expect("lower the same pinned CVS contract through ManT");
    mant_render::render_query_text(&query)
}

fn lowered_document(source: &str) -> Document {
    mant_loader::load_roff_bytes(source.as_bytes())
        .expect("lower the same pinned CVS contract through ManT")
        .document
        .expect("document")
}

fn without_line_indentation(output: &str) -> String {
    output
        .lines()
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_definition_item(document: &Document) -> &DefinitionItem {
    document
        .sections
        .iter()
        .flat_map(|section| section.blocks.iter())
        .find_map(|block| match block {
            Block::DefinitionList { items, .. } => items.first(),
            _ => None,
        })
        .expect("definition item")
}

use super::inline_text;

#[path = "cvs_renderer_contracts/author_execution.rs"]
mod author_execution;
#[path = "cvs_renderer_contracts/definition_fields.rs"]
mod definition_fields;
#[path = "cvs_renderer_contracts/formatter_boundaries.rs"]
mod formatter_boundaries;
#[path = "cvs_renderer_contracts/identity.rs"]
mod identity;
#[path = "cvs_renderer_contracts/macro_gaps.rs"]
mod macro_gaps;
#[path = "cvs_renderer_contracts/matrix.rs"]
mod matrix;
