//! Public structured-model invariants.

use super::*;

#[test]
fn keys_reject_the_absent_sentinel() {
    assert_eq!(SourceKey::new(0), None);
    assert_eq!(SpanKey::new(0), None);
    assert_eq!(ProvenanceKey::new(0), None);
    assert_eq!(OwnerKey::new(0), None);
    assert_eq!(ContentRootKey::new(0), None);
    assert_eq!(ContentAtomKey::new(0), None);
    assert_eq!(LinkOccurrenceKey::new(0), None);
    assert_eq!(NativeBlockKey::new(0), None);
}

#[test]
fn content_ref_resolves_the_documents_atom_store_without_copying() {
    let text = String::from("alphabet");
    let text_pointer = text.as_ptr();
    let atom_key = ContentAtomKey::new(1).unwrap();
    let reference = ContentRef {
        atom: atom_key,
        bytes: 1..4,
    };
    let document = test_document(vec![ContentAtom {
        key: atom_key,
        root: ContentRootKey::new(1).unwrap(),
        ordinal: 0,
        owner: OwnerKey::new(1).unwrap(),
        kind: ContentAtomKind::Text {
            text,
            display_override: None,
        },
        style: StructuredStyle::default(),
        role: None,
        link: None,
        provenance: ProvenanceKey::new(1).unwrap(),
    }]);

    let resolved = document.resolve_content_ref(&reference).unwrap();
    assert_eq!(resolved, "lph");
    assert_eq!(resolved.as_ptr(), text_pointer.wrapping_add(1));
}

#[test]
fn width_is_deterministic_and_explicitly_overridable() {
    assert_eq!(StructuredRenderer::default().width(), 78);
    assert_eq!(
        StructuredRenderer::new().with_width(100).unwrap().width(),
        100
    );
    assert_eq!(
        StructuredRenderer::new().with_width(19).unwrap_err().kind(),
        StructuredErrorKind::InvalidInput
    );
}

#[test]
fn line_ends_and_hard_breaks_have_no_numeric_sentinels() {
    assert_eq!(LineColumn::new(0, 7), None);
    assert_eq!(LineColumn::new(3, 0), None);
    let point = LineColumns::new(LineColumn::new(3, 7).unwrap(), None);
    assert_eq!(point.end(), None);
    assert_eq!(ContentAtomKind::HardBreak.logical_text(), Some("\n"));
    assert_eq!(ContentAtomKind::BreakOpportunity.logical_text(), None);

    let atom_key = ContentAtomKey::new(1).unwrap();
    let document = test_document(vec![ContentAtom {
        key: atom_key,
        root: ContentRootKey::new(1).unwrap(),
        ordinal: 0,
        owner: OwnerKey::new(1).unwrap(),
        kind: ContentAtomKind::HardBreak,
        style: StructuredStyle::default(),
        role: None,
        link: None,
        provenance: ProvenanceKey::new(1).unwrap(),
    }]);
    assert_eq!(
        document.resolve_content_ref(&ContentRef {
            atom: atom_key,
            bytes: 0..1,
        }),
        None
    );
}

fn test_document(content_atoms: Vec<ContentAtom>) -> StructuredDocument {
    StructuredDocument {
        root_source: SourceKey::new(1).unwrap(),
        profile: StructuredProfile::Utf8,
        width: DEFAULT_STRUCTURED_WIDTH,
        metadata: StructuredMetadata {
            macro_set: SourceFormat::Man,
            title: None,
            section: None,
            volume: None,
            operating_system: None,
            architecture: None,
            name: None,
            date: None,
            alias_target: None,
            has_body: true,
        },
        sources: Vec::new(),
        spans: Vec::new(),
        provenances: vec![Provenance::Unknown],
        owners: Vec::new(),
        content_roots: Vec::new(),
        content_atoms,
        content_refs: Vec::new(),
        links: Vec::new(),
        blocks: Vec::new(),
        diagnostics: Vec::new(),
    }
}
