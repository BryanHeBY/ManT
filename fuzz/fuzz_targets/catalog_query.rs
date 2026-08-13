#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use mant_engine::{
    AvailableDocument, AvailableDocumentKind, AvailableDocumentOrigin, query_available_documents,
};
use mant_protocol::{CatalogDocumentKind, CatalogQuery, SearchCase, SearchSyntax};

const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_DOCUMENTS: usize = 64;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let control = |index: usize| data.get(index).copied().unwrap_or_default();
    let text = String::from_utf8_lossy(data.get(8..).unwrap_or_default());
    let mut fields = text
        .split(['\0', '\n'])
        .map(|field| field.chars().take(256).collect::<String>());
    let pattern = fields.next().unwrap_or_default();
    let mut documents = fields
        .filter(|field| !field.is_empty())
        .take(MAX_DOCUMENTS)
        .enumerate()
        .map(|(index, logical_path)| document(index, logical_path))
        .collect::<Vec<_>>();
    if documents.is_empty() {
        documents.push(document(0, "man".to_owned()));
    }

    let query = CatalogQuery {
        pattern: (control(0) & 1 != 0).then_some(pattern),
        syntax: if control(1) & 1 == 0 {
            SearchSyntax::Literal
        } else {
            SearchSyntax::Regex
        },
        case: match control(2) % 3 {
            0 => SearchCase::Sensitive,
            1 => SearchCase::Insensitive,
            _ => SearchCase::Smart,
        },
        kind: match control(3) % 3 {
            0 => None,
            1 => Some(CatalogDocumentKind::Markdown),
            _ => Some(CatalogDocumentKind::Manual),
        },
        source: (control(4) & 1 != 0).then(|| "alpha".to_owned()),
        manual_section: (control(5) & 1 != 0)
            .then(|| ["1", "3", "8"][usize::from(control(5)) % 3].to_owned()),
        limit: [0, 1, 2, 100, 10_000, 10_001][usize::from(control(6)) % 6],
        offset: [0, 1, 2, 100, u32::MAX][usize::from(control(7)) % 5],
    };

    let Ok(result) = query_available_documents(&documents, &query) else {
        return;
    };
    let repeated = query_available_documents(&documents, &query).expect("same query stays valid");
    assert_eq!(result, repeated, "catalog queries must be deterministic");
    assert_eq!(
        result.returned as usize,
        result.documents.len(),
        "returned count must describe the page"
    );
    assert!(result.returned <= result.total);
    assert_eq!(result.truncated, result.next_offset.is_some());
    for summary in &result.documents {
        assert_eq!(summary.catalog_path, summary.address.catalog_path());
    }
});

fn document(index: usize, logical_path: String) -> AvailableDocument {
    let name = logical_path
        .rsplit('/')
        .next()
        .unwrap_or(&logical_path)
        .to_owned();
    let kind = if index % 3 == 2 {
        AvailableDocumentKind::Manual
    } else {
        AvailableDocumentKind::Markdown
    };
    let origin = match kind {
        AvailableDocumentKind::Manual => AvailableDocumentOrigin::ManualPath,
        AvailableDocumentKind::Markdown if index & 1 == 0 => AvailableDocumentOrigin::Documents,
        AvailableDocumentKind::Markdown => AvailableDocumentOrigin::Source("alpha".to_owned()),
    };
    let source_priority = matches!(origin, AvailableDocumentOrigin::Source(_))
        .then(|| [-1, 0, 1][index % 3]);
    AvailableDocument {
        name,
        logical_path,
        kind,
        manual_section: (kind == AvailableDocumentKind::Manual)
            .then(|| ["1", "3", "8"][index % 3].to_owned()),
        path: PathBuf::from(format!("/fuzz/document-{index}")),
        origin,
        source_priority,
    }
}
