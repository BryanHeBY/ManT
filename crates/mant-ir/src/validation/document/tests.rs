//! Document invariant fixtures and grouped regression contracts.
mod content;
mod identities;
mod links;

use super::{diagnostics::is_semantic_completeness_diagnostic, validate_document};
use crate::validation::{
    email_address_from_mailto_uri, is_valid_email_address, is_valid_external_uri,
    mailto_uri_for_email_address,
};
use crate::{
    Block, DefinitionItem, Diagnostic, DiagnosticLevel, Document, DocumentMeta, DocumentSource,
    EntryFacts, EntryKind, HeadBodyRelation, Inline, LayoutHint, LinkTarget, NameCase, NodeId,
    Section, SourceFormat, SourceSpan, TableCell, TableRow, TextRange, TextSize,
};

fn document(sections: Vec<Section>, blocks: Vec<Block>) -> Document {
    Document {
        heading: None,
        parser: None,
        source: DocumentSource {
            format: SourceFormat::Markdown,
            path: None,
        },
        meta: DocumentMeta::default(),
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
        blocks,
        sections,
    }
}

fn section(id: &str) -> Section {
    Section {
        id: id.into(),
        fragment_aliases: Vec::new(),
        heading: id.into(),
        spacing_before_lines: 0,
        blocks: Vec::new(),
        children: Vec::new(),
        source: None,
    }
}
