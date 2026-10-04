//! Contract-oriented tests for `CommonMark` structure and escaping.

use mant_ir::{
    Block, DefinitionItem, Document, DocumentMeta, DocumentSource, EntryFacts, EntryKind, Inline,
    LayoutHint, ListItem, ListKind, NameCase, Section, SourceFormat, TableCell, TableRow,
    visit::{Visit, walk_inline},
};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::{
    ArtifactBuilder, MarkdownFragmentOptions, MarkdownNode, MarkdownOptions, blocks, inline,
    render_addressable_markdown, render_blocks_fragment, render_located_blocks_fragment,
    render_markdown, render_markdown_artifact, render_markdown_with_options,
    render_sections_fragment, semantic,
};
use mant_ir::ResolvedContent;

mod addressable_maps;
mod block_structure;
mod definition_rows;
mod hard_rows;
mod inline_contexts;
mod inline_escaping;
mod inline_styles;
mod links;
mod row_layout;

fn parse_content(
    source: &str,
    source_path: Option<String>,
) -> Result<ResolvedContent, crate::markdown::MarkdownParseError> {
    let label = source_path.clone().unwrap_or_else(|| "stdin".to_owned());
    let parsed = crate::markdown::parse_markdown(source, source_path)?;
    Ok(ResolvedContent {
        address: None,
        label,
        document: Some(parsed.document),
        tldr: parsed.tldr,
    })
}

fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        inline_layout: mant_ir::InlineLayout::default(),
        children,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn manual(sections: Vec<Section>) -> Document {
    Document {
        heading: None,
        parser: None,
        source: DocumentSource {
            format: SourceFormat::Man,
            path: None,
        },
        meta: DocumentMeta::default(),
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
        blocks: Vec::new(),
        sections,
    }
}

fn section(title: &str, blocks: Vec<Block>, children: Vec<Section>) -> Section {
    Section {
        id: title.to_lowercase().into(),
        fragment_aliases: Vec::new(),
        heading: title.into(),
        spacing_before_lines: 0,
        blocks,
        children,
        source: None,
    }
}
