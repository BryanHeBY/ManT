//! Original byte-range and navigation mapping contracts.
use super::*;

#[test]
fn prefixes_preserve_nested_unicode_and_crlf_owner_boundaries() {
    // Opaque equal keys are sufficient here: no facts are dereferenced.
    let key = std::ptr::null();
    let rendered = MappedText {
        text: "α\r\n\r\n日本\r\nlast".into(),
        owners: vec![(key, 0..18), (key, 6..12)],
        ..Default::default()
    }
    .prefix("12. ")
    .unwrap();
    assert_eq!(rendered.text, "12. α\n\n    日本\n    last");
    assert_eq!(
        &rendered.text[rendered.owners[0].1.clone()],
        "α\n\n    日本\n    last"
    );
    assert_eq!(&rendered.text[rendered.owners[1].1.clone()], "日本");
}

#[test]
fn joining_and_inserting_do_not_claim_adjacent_unowned_text() {
    let key = std::ptr::null();
    let mut rendered = MappedText::join(
        [
            "before".to_owned().into(),
            MappedText {
                text: "owned".into(),
                owners: vec![(key, 0..5)],
                ..Default::default()
            },
            "after".to_owned().into(),
        ],
        " | ",
    );
    rendered.insert(0, "prefix ");
    rendered.insert(rendered.text.len(), " suffix");
    assert_eq!(&rendered.text[rendered.owners[0].1.clone()], "owned");
}

#[test]
fn navigation_receivers_follow_unicode_prefixes_without_new_paragraphs() {
    let mut rendered = MappedText::from("```txt\nα\n```".to_owned())
        .syntax_site(BlockSyntax::Fence)
        .prefix("12. ")
        .unwrap();
    rendered.attach_navigation("[](first)");
    rendered.append_navigation("[](second)");
    assert_eq!(
        rendered.text,
        "12. [](first)[](second)\n    ```txt\n    α\n    ```"
    );
    assert_eq!(rendered.text.matches("\n\n").count(), 0);
}

#[test]
fn parent_navigation_preserves_a_childs_own_anchor_and_unicode_range() {
    let item: mant_ir::ListItem = serde_json::from_value(serde_json::json!({
        "blocks": [],
        "entry": { "id": "own", "kind": {"kind": "term"}, "case": "sensitive", "names": ["α"] }
    }))
    .unwrap();
    let mut rendered = MappedText::from("α".to_owned()).syntax_site(BlockSyntax::Phrasing);
    rendered.attach_navigation("<a id=\"own\"></a>");
    let mut rendered = rendered
        .with_owner(EntryOwner::List(&item), true)
        .prefix("- ")
        .unwrap();
    rendered.append_navigation("[](parent)");
    assert_eq!(rendered.text, "- [](parent)<a id=\"own\"></a>α");
    assert_eq!(
        &rendered.text[rendered.owners[0].1.clone()],
        "<a id=\"own\"></a>α"
    );
}

#[test]
fn navigation_only_owners_transfer_without_claiming_the_receiver() {
    let key = std::ptr::null();
    let navigation = MappedText {
        text: "[](uri)".into(),
        owners: vec![(key, 0..7)],
        contribution: Contribution::default(),
        ..Default::default()
    };
    let mut rendered = MappedText::from("BODY".to_owned()).syntax_site(BlockSyntax::Phrasing);
    rendered.attach_navigation_text(navigation, false);
    assert_eq!(rendered.text, "[](uri)BODY");
    assert_eq!(&rendered.text[rendered.owners[0].1.clone()], "[](uri)");
}
