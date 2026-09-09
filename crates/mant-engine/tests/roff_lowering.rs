//! End-to-end native lowering contracts across codec, query and rendering.
use std::{collections::HashSet, fmt::Write as _, fs, process};

use mant_ir::{
    Block, DiagnosticLevel, Inline, ListKind, ResolvedContent, SemanticIndex, SourceFormat,
    ValueDomain,
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
