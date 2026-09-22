//! End-to-end native lowering contracts across codec, query and rendering.
use std::{collections::HashSet, fmt::Write as _, fs, process};

use mant_ir::{
    Block, ContentContext, Inline, InlineView, ListKind, ResolvedContent, SemanticIndex,
    SourceFormat, ValueDomain,
    visit::{self, Visit},
};

use mant_loader::parse_manual_bytes;

#[path = "../src/semantic_test_read.rs"]
mod semantic_test_read;

// Integration fixtures are read by the harness; product parsing uses public APIs.
fn parse_manual_source(
    path: &std::path::Path,
) -> Result<mant_ir::Document, Box<dyn std::error::Error>> {
    Ok(parse_manual_bytes(path, &fs::read(path)?)?)
}

#[path = "roff_lowering/consumer_boundaries.rs"]
mod consumer_boundaries;
#[path = "roff_lowering/cvs_renderer_contracts.rs"]
mod cvs_renderer_contracts;
#[path = "roff_lowering/driver.rs"]
mod driver;
#[path = "roff_lowering/entry_forms.rs"]
mod entry_forms;
#[path = "roff_lowering/flow_controls.rs"]
mod flow_controls;
#[path = "roff_lowering/font_boundaries.rs"]
mod font_boundaries;
#[path = "roff_lowering/glyphs.rs"]
mod glyphs;
#[path = "roff_lowering/inline_boundaries.rs"]
mod inline_boundaries;

#[path = "roff_lowering/upstream_inline.rs"]
mod upstream_inline;
#[path = "roff_lowering/upstream_tables.rs"]
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
    struct AnchorCollector<'a>(Vec<(String, u32)>, ContentContext<'a>);

    impl<'ir> Visit<'ir> for AnchorCollector<'ir> {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Ok(InlineView::Anchor(anchor)) = self.1.inline(inline)
                && let Some(source) = anchor.owner_source()
            {
                self.0.push((anchor.id().to_string(), source.line));
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = AnchorCollector(Vec::new(), document.content());
    collector.visit_document(document);
    collector.0
}

fn visible_document_text(document: &mant_ir::Document) -> String {
    struct TextCollector<'a>(String, ContentContext<'a>);

    impl<'ir> Visit<'ir> for TextCollector<'ir> {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            match self.1.inline(inline).unwrap() {
                InlineView::Text(value) | InlineView::Code(value) => {
                    self.0.push_str(value);
                    self.0.push(' ');
                }
                InlineView::LineBreak { .. } => self.0.push('\n'),
                _ => {}
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = TextCollector(String::new(), document.content());
    collector.visit_document(document);
    collector.0
}

fn inline_text(content: ContentContext<'_>, children: &[Inline]) -> String {
    content.plain_text(children).unwrap()
}

#[path = "roff_lowering/entries.rs"]
mod entries;
#[path = "roff_lowering/layout.rs"]
mod layout;
#[path = "roff_lowering/layout_geometry.rs"]
mod layout_geometry;
#[path = "roff_lowering/navigation.rs"]
mod navigation;
#[path = "roff_lowering/tables.rs"]
mod tables;
