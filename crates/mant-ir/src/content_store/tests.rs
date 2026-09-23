use super::*;

fn owner_and_root(builder: &mut ContentStoreBuilder) -> (ContentOwnerKey, ContentRootKey) {
    let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
    let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
    (owner, root)
}

fn text(builder: &mut ContentStoreBuilder, root: ContentRootKey, value: &str) -> ContentRef {
    builder.push_text(
        root,
        value.into(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    )
}

#[test]
fn attaching_settled_atoms_is_atomic_and_accepts_only_zero_width_gaps() {
    let mut builder = ContentStoreBuilder::new();
    let (_, root) = owner_and_root(&mut builder);
    let first = text(&mut builder, root, "first").atom;
    let _visible_gap = text(&mut builder, root, "gap");
    let last = text(&mut builder, root, "last").atom;
    assert!(
        builder
            .push_link_for_atoms(
                &[first, last],
                LinkTarget::Document {
                    name: "invalid".into(),
                    fragment: None,
                },
                None,
                Provenance::Unknown,
            )
            .is_none()
    );
    assert!(builder.content_store().links.is_empty());
    assert!(
        builder
            .content_store()
            .atoms
            .iter()
            .all(|atom| atom.link.is_none())
    );

    let mut builder = ContentStoreBuilder::new();
    let (_, root) = owner_and_root(&mut builder);
    let first = text(&mut builder, root, "first").atom;
    let _gap = builder.push_break_opportunity(root, Provenance::Unknown);
    let last = text(&mut builder, root, "last").atom;
    let occurrence = builder
        .push_link_for_atoms(
            &[first, last],
            LinkTarget::Document {
                name: "valid".into(),
                fragment: None,
            },
            None,
            Provenance::Unknown,
        )
        .unwrap();
    assert_eq!(
        builder.content_store().atom(first).unwrap().link,
        Some(occurrence)
    );
    validate_content_store(builder.content_store()).unwrap();
}

#[test]
fn nested_link_parts_may_cross_owner() {
    let mut builder = ContentStoreBuilder::new();
    let (first_owner, first_root) = owner_and_root(&mut builder);
    let outer = builder.push_link(
        first_owner,
        LinkTarget::External {
            uri: "https://outer.test".into(),
        },
        None,
        Provenance::Unknown,
    );
    let _before = builder.push_text(
        first_root,
        "before".into(),
        None,
        ContentStyle::default(),
        None,
        Some(outer),
        Provenance::Unknown,
    );
    let inner = builder.push_link(
        first_owner,
        LinkTarget::Document {
            name: "inner".into(),
            fragment: None,
        },
        None,
        Provenance::Unknown,
    );
    let _middle = builder.push_text(
        first_root,
        "middle".into(),
        None,
        ContentStyle::default(),
        None,
        Some(inner),
        Provenance::Unknown,
    );
    let _after = builder.push_text(
        first_root,
        "after".into(),
        None,
        ContentStyle::default(),
        None,
        Some(outer),
        Provenance::Unknown,
    );
    let second_owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
    let second_root = builder.push_root(second_owner, ContentRootKind::Body, Provenance::Unknown);
    let _continued = builder.push_text(
        second_root,
        "continued".into(),
        None,
        ContentStyle::default(),
        None,
        Some(outer),
        Provenance::Unknown,
    );
    validate_content_store(builder.content_store()).unwrap();
}

#[test]
fn unrelated_visible_gaps_still_fail() {
    let mut invalid = ContentStoreBuilder::new();
    let (owner, root) = owner_and_root(&mut invalid);
    let link = invalid.push_link(
        owner,
        LinkTarget::Document {
            name: "gap".into(),
            fragment: None,
        },
        None,
        Provenance::Unknown,
    );
    for (text, occurrence) in [
        ("first", Some(link)),
        ("unrelated", None),
        ("last", Some(link)),
    ] {
        let _ = invalid.push_text(
            root,
            text.into(),
            None,
            ContentStyle::default(),
            None,
            occurrence,
            Provenance::Unknown,
        );
    }
    assert!(validate_content_store(invalid.content_store()).is_err());

    let mut interleaved = ContentStoreBuilder::new();
    let (owner, first_root) = owner_and_root(&mut interleaved);
    let second_root = interleaved.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
    let link = interleaved.push_link(
        owner,
        LinkTarget::Document {
            name: "interleaved".into(),
            fragment: None,
        },
        None,
        Provenance::Unknown,
    );
    for (root, value, occurrence) in [
        (first_root, "first", Some(link)),
        (first_root, "unrelated", None),
        (second_root, "middle", Some(link)),
        (first_root, "last", Some(link)),
    ] {
        let _ = interleaved.push_text(
            root,
            value.into(),
            None,
            ContentStyle::default(),
            None,
            occurrence,
            Provenance::Unknown,
        );
    }
    assert!(validate_content_store(interleaved.content_store()).is_err());
}

#[test]
fn detaching_a_link_prevalidates_every_atom_and_keeps_dense_identity() {
    let mut builder = ContentStoreBuilder::new();
    let (owner, root) = owner_and_root(&mut builder);
    let occurrence = builder.push_link(
        owner,
        LinkTarget::Document {
            name: "target".into(),
            fragment: None,
        },
        None,
        Provenance::Unknown,
    );
    let first = builder.push_text(
        root,
        "one".into(),
        None,
        ContentStyle::default(),
        None,
        Some(occurrence),
        Provenance::Unknown,
    );
    let second = builder.push_text(
        root,
        "two".into(),
        None,
        ContentStyle::default(),
        None,
        Some(occurrence),
        Provenance::Unknown,
    );
    let mut store = builder.finish();
    store.atoms[second.atom.index().unwrap()].link = None;
    let corrupt = store.clone();
    assert!(!store.detach_link(occurrence));
    assert_eq!(store, corrupt);

    store.atoms[second.atom.index().unwrap()].link = Some(occurrence);
    assert!(store.detach_link(occurrence));
    assert!(store.link(occurrence).unwrap().label.is_empty());
    assert!(store.atom(first.atom).unwrap().link.is_none());
    assert!(store.atom(second.atom).unwrap().link.is_none());
    validate_content_store(&store).unwrap();
}

#[test]
fn replacing_text_shifts_later_points_and_rejects_in_atom_points_atomically() {
    let mut builder = ContentStoreBuilder::new();
    let (_, root) = owner_and_root(&mut builder);
    let mut content = text(&mut builder, root, "a");
    let later = builder.push_point(
        root,
        PointBoundary::BetweenAtoms { atom_boundary: 1 },
        1,
        Provenance::Unknown,
    );
    assert!(builder.replace_text(&mut content, "éé".into()));
    assert_eq!(
        builder
            .content_store()
            .point(later)
            .unwrap()
            .scalar_boundary,
        2
    );
    validate_content_store(builder.content_store()).unwrap();

    let inside = builder.push_point(
        root,
        PointBoundary::InAtom {
            atom: content.atom,
            byte_offset: 2,
        },
        1,
        Provenance::Unknown,
    );
    let before = builder.content_store().clone();
    assert!(!builder.replace_text(&mut content, "x".into()));
    assert_eq!(builder.content_store(), &before);
    assert_eq!(
        builder
            .content_store()
            .point(inside)
            .unwrap()
            .scalar_boundary,
        1
    );
}

#[test]
fn repeated_in_atom_points_share_incremental_prefix_work() {
    let mut builder = ContentStoreBuilder::new();
    let (_, root) = owner_and_root(&mut builder);
    let long = text(&mut builder, root, &"x".repeat(64 * 1024));
    let next = text(&mut builder, root, "y");
    for _ in 0..4096 {
        let _ = builder.push_point(
            root,
            PointBoundary::InAtom {
                atom: long.atom,
                byte_offset: 64 * 1024,
            },
            64 * 1024,
            Provenance::Unknown,
        );
        let _ = builder.push_point(
            root,
            PointBoundary::InAtom {
                atom: next.atom,
                byte_offset: 0,
            },
            64 * 1024,
            Provenance::Unknown,
        );
    }
    let mut store = builder.finish();
    validate_content_store(&store).unwrap();
    store.points.last_mut().unwrap().scalar_boundary += 1;
    assert!(validate_content_store(&store).is_err());
}

#[test]
fn point_wire_round_trips_flattened_boundaries_and_rejects_unknown_fields() {
    let point = ContentPoint {
        key: ContentPointKey::FIRST,
        root: ContentRootKey::FIRST,
        owner: ContentOwnerKey::FIRST,
        boundary: PointBoundary::BetweenAtoms { atom_boundary: 0 },
        scalar_boundary: 0,
        provenance: Provenance::Unknown,
    };
    let mut value = serde_json::to_value(&point).unwrap();
    assert_eq!(
        serde_json::from_value::<ContentPoint>(value.clone()).unwrap(),
        point
    );
    value["unknown"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<ContentPoint>(value).is_err());
}

#[test]
fn display_projection_is_single_scalar_inline_and_terminal_safe() {
    let mut builder = ContentStoreBuilder::new();
    let (_, root) = owner_and_root(&mut builder);
    let dash = builder.push_text(
        root,
        "—".into(),
        Some("--".into()),
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let _space = builder.push_whitespace(
        root,
        "\u{a0}".into(),
        Some(" ".into()),
        false,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let valid = builder.finish();
    validate_content_store(&valid).expect("ASCII glyph strings and spaces are safe projections");

    for unsafe_glyph in ["", "\u{1b}[31m", "\t", "\n", "\r", "\u{2028}", "\u{2029}"] {
        let mut store = valid.clone();
        let ContentAtomKind::Text {
            display_override, ..
        } = &mut store.atoms[dash.atom.index().unwrap()].kind
        else {
            unreachable!();
        };
        *display_override = Some(unsafe_glyph.into());
        assert!(
            validate_content_store(&store).is_err(),
            "unsafe projected glyph {unsafe_glyph:?} was accepted"
        );
    }

    let mut store = valid;
    let ContentAtomKind::Text { text, .. } = &mut store.atoms[dash.atom.index().unwrap()].kind
    else {
        unreachable!();
    };
    *text = "ab".into();
    assert!(validate_content_store(&store).is_err());
}
