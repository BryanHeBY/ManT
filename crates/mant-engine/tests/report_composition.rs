//! Cross-crate query-to-report contracts; renderer unit tests need no query engine.

fn text_inline(document: &mut mant_ir::Document, value: &str, heading: bool) -> mant_ir::Inline {
    use mant_ir::{
        ContentAtom, ContentAtomKey, ContentAtomKind, ContentByteRange, ContentOwner,
        ContentOwnerKey, ContentOwnerKind, ContentRef, ContentRoot, ContentRootKey,
        ContentRootKind, ContentStyle, Provenance,
    };
    let store = &mut document.content_store;
    let owner = ContentOwnerKey::new(u32::try_from(store.owners.len() + 1).unwrap()).unwrap();
    let root = ContentRootKey::new(u32::try_from(store.roots.len() + 1).unwrap()).unwrap();
    let atom = ContentAtomKey::new(u32::try_from(store.atoms.len() + 1).unwrap()).unwrap();
    store.owners.push(ContentOwner {
        key: owner,
        kind: if heading {
            ContentOwnerKind::Section
        } else {
            ContentOwnerKind::Content
        },
        roots: vec![root],
        provenance: Provenance::Unknown,
    });
    store.roots.push(ContentRoot {
        key: root,
        owner,
        kind: if heading {
            ContentRootKind::Heading
        } else {
            ContentRootKind::Body
        },
        atoms: vec![atom],
        points: Vec::new(),
        provenance: Provenance::Unknown,
    });
    store.atoms.push(ContentAtom {
        key: atom,
        root,
        owner,
        kind: ContentAtomKind::Text {
            text: value.to_owned(),
            display_override: None,
        },
        style: ContentStyle::default(),
        role: None,
        link: None,
        provenance: Provenance::Unknown,
    });
    mant_ir::Inline::Text {
        content: ContentRef {
            atom,
            bytes: ContentByteRange {
                start: 0,
                end: u32::try_from(value.len()).unwrap(),
            },
        },
    }
}

fn heading(document: &mut mant_ir::Document, value: &str) -> mant_ir::Heading {
    mant_ir::Heading {
        content: vec![text_inline(document, value, true)],
        source: None,
    }
}

#[path = "report_composition/markdown.rs"]
mod markdown;
#[path = "report_composition/text.rs"]
mod text;
