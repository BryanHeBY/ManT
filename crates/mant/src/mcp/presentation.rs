//! Sanitization applied before protocol projections cross the agent boundary.

use mant_protocol::{QueryExcerpt, QueryOutline, QuerySearch};

pub(super) fn prepare_excerpt(excerpt: &mut QueryExcerpt) {
    excerpt.diagnostics.clear();
    discard_document_source_path(&mut excerpt.source);
    for selection in &mut excerpt.selections {
        if let mant_protocol::ExcerptSelection::Tldr { document, .. } = selection {
            document.source_path.clear();
        }
    }
}

pub(super) fn prepare_outline(outline: &mut QueryOutline) {
    outline.diagnostics.clear();
    discard_document_source_path(&mut outline.source);
}

pub(super) fn prepare_search(search: &mut QuerySearch) {
    discard_document_source_path(&mut search.source);
}

fn discard_document_source_path(source: &mut Option<mant_ir::DocumentSource>) {
    if let Some(source) = source {
        source.path = None;
    }
}
