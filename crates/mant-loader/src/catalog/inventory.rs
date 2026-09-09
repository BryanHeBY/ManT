//! Builds the shared discovery inventory and its logical address projections.

use super::{AvailableDocument, AvailableDocumentKind, AvailableDocumentOrigin};
use mant_protocol::{DocumentAddress, DocumentSummary, MarkdownOrigin};
use mant_sources::{BUILTIN_CONTENT_PRIORITY, RegisteredDocument, RegisteredDocumentOrigin};

pub(super) fn document_summary(document: &AvailableDocument) -> DocumentSummary {
    let address = match &document.origin {
        AvailableDocumentOrigin::Documents => DocumentAddress::Markdown {
            path: document.logical_path.clone(),
            origin: MarkdownOrigin::Documents,
        },
        AvailableDocumentOrigin::Source(source) => DocumentAddress::Markdown {
            path: document.logical_path.clone(),
            origin: MarkdownOrigin::Source {
                name: source.clone(),
            },
        },
        AvailableDocumentOrigin::ManualPath => DocumentAddress::Manual {
            name: document.name.clone(),
            manual_section: document.manual_section.clone().unwrap_or_default(),
        },
    };
    DocumentSummary { address }
}

pub(super) fn available_catalog_path(document: &AvailableDocument) -> String {
    match &document.origin {
        AvailableDocumentOrigin::Documents => format!("documents/{}", document.logical_path),
        AvailableDocumentOrigin::Source(source) => {
            format!("sources/{source}/{}", document.logical_path)
        }
        AvailableDocumentOrigin::ManualPath => format!(
            "manual/{}/{}",
            document.manual_section.as_deref().unwrap_or_default(),
            document.name
        ),
    }
}

pub(super) fn compare_precedence(
    left: &AvailableDocument,
    right: &AvailableDocument,
) -> std::cmp::Ordering {
    fn class(document: &AvailableDocument) -> u8 {
        match (&document.origin, document.source_priority) {
            (AvailableDocumentOrigin::Documents, _) => 0,
            (AvailableDocumentOrigin::Source(_), Some(priority))
                if priority > BUILTIN_CONTENT_PRIORITY =>
            {
                1
            }
            (AvailableDocumentOrigin::ManualPath, _) => 2,
            (AvailableDocumentOrigin::Source(_), _) => 3,
        }
    }

    class(left)
        .cmp(&class(right))
        .then_with(|| match (&left.origin, &right.origin) {
            (AvailableDocumentOrigin::Source(_), AvailableDocumentOrigin::Source(_)) => right
                .source_priority
                .unwrap_or_default()
                .cmp(&left.source_priority.unwrap_or_default()),
            _ => std::cmp::Ordering::Equal,
        })
}

pub(crate) fn list_available_documents_from(
    registered: Vec<RegisteredDocument>,
    manuals: &[crate::ManualPage],
) -> Vec<AvailableDocument> {
    let mut documents = registered
        .into_iter()
        .map(|document| AvailableDocument {
            name: document
                .logical_path
                .rsplit('/')
                .next()
                .unwrap_or(&document.logical_path)
                .to_owned(),
            logical_path: document.logical_path,
            kind: AvailableDocumentKind::Markdown,
            manual_section: None,
            path: document.path,
            source_priority: document.source_priority,
            origin: match document.origin {
                RegisteredDocumentOrigin::Documents => AvailableDocumentOrigin::Documents,
                RegisteredDocumentOrigin::Source(source) => AvailableDocumentOrigin::Source(source),
            },
        })
        .chain(manuals.iter().map(|page| AvailableDocument {
            name: page.name.clone(),
            logical_path: page.name.clone(),
            kind: AvailableDocumentKind::Manual,
            manual_section: Some(page.section.clone()),
            path: page.path.clone(),
            origin: AvailableDocumentOrigin::ManualPath,
            source_priority: None,
        }))
        .collect::<Vec<_>>();
    documents.sort_by(|left, right| {
        left.logical_path
            .cmp(&right.logical_path)
            .then_with(|| compare_precedence(left, right))
            .then_with(|| left.manual_section.cmp(&right.manual_section))
            .then_with(|| left.origin.cmp(&right.origin))
    });
    documents
}
