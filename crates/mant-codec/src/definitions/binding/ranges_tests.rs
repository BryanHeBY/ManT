//! Exact authored leaf paths and UTF-8 byte slices after lexical recognition.
use super::*;
use crate::definitions::RecognizedName;
use mant_ir::{DefinitionLayout, EntryOwner, LinkTarget, inline_plain_text};

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn item(terms: Vec<Vec<Inline>>) -> DefinitionItem {
    DefinitionItem {
        terms,
        description: Vec::new(),
        entry: None,
        layout: DefinitionLayout::default(),
        source: None,
    }
}

fn slice(index: usize, path: &[usize], bytes: Range<usize>) -> EntryContentSlice {
    EntryContentSlice {
        root: EntryInlineRoot::Term { index },
        path: path.to_vec(),
        bytes: Some(bytes),
    }
}

fn bound_text(owner: &DefinitionItem, form: &EntryForm) -> String {
    inline_plain_text(
        &EntryOwner::Definition(owner)
            .form(form)
            .expect("all bindings address actual authored leaves"),
    )
}

#[test]
fn nested_styles_links_and_unicode_keep_exact_author_leaf_byte_ranges() {
    // The byte mapper consumes already recognized names. The native fixtures
    // unicode-styled-head/linked-utf8-head were probed before this test;
    // man_term.c::pre_alternate and pre_UR affect display, not URI/title text.
    let owner = item(vec![vec![
        Inline::anchor("not-visible"),
        text("prefix "),
        Inline::Strong {
            children: vec![
                text("--"),
                Inline::Emphasis {
                    children: vec![Inline::Link {
                        target: LinkTarget::External {
                            uri: "https://example.org/hidden".into(),
                        },
                        title: Some("--not-a-visible-name".into()),
                        children: vec![
                            Inline::anchor("inside-label"),
                            Inline::Code {
                                value: "界é".into(),
                            },
                        ],
                    }],
                },
            ],
        },
        text(" suffix"),
    ]]);
    let names = vec!["--界é".into()];
    let recognized = vec![vec![RecognizedName::contiguous("--界é", 7)]];
    let original = owner.clone();
    let bindings = native_name_bindings(&owner, &names, &recognized);
    assert_eq!(owner, original);
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].evidence, EntryNameEvidence::Lexical);
    assert_eq!(
        bindings[0].occurrences,
        [EntryForm {
            parts: vec![slice(0, &[2, 0], 0..2), slice(0, &[2, 1, 0, 1], 0..5)],
        }]
    );
    assert_eq!(bound_text(&owner, &bindings[0].occurrences[0]), names[0]);
    let projected = EntryOwner::Definition(&owner)
        .form(&bindings[0].occurrences[0])
        .unwrap();
    let rendered = format!("{projected:?}");
    assert!(rendered.contains("https://example.org/hidden"));
    assert!(rendered.contains("--not-a-visible-name"));
    assert!(rendered.contains("Strong"));
    assert!(rendered.contains("Emphasis"));
    assert!(rendered.contains("Code"));
}

#[test]
fn hard_rows_and_combining_scalars_advance_bytes_without_changing_leaf_paths() {
    let owner = item(vec![vec![
        text("界"),
        Inline::line_break_indented(6),
        Inline::Strong {
            children: vec![text(""), text("--e\u{301}"), Inline::anchor("tail")],
        },
    ]]);
    let names = vec!["--e\u{301}".into()];
    let bindings = native_name_bindings(
        &owner,
        &names,
        &[vec![RecognizedName::contiguous("--e\u{301}", 4)]],
    );
    assert_eq!(
        bindings[0].occurrences,
        [EntryForm {
            parts: vec![slice(0, &[2, 1], 0..5)],
        }]
    );
    assert_eq!(bound_text(&owner, &bindings[0].occurrences[0]), names[0]);
}

#[test]
fn discontiguous_names_and_repeated_terms_keep_original_occurrence_order() {
    let owner = item(vec![
        vec![text("prefix --"), Inline::line_break(), text("name suffix")],
        vec![Inline::Code {
            value: "--name".into(),
        }],
    ]);
    let recognized = vec![
        vec![RecognizedName {
            name: "--name".into(),
            parts: vec![7..9, 10..14],
        }],
        vec![RecognizedName::contiguous("--name", 0)],
    ];
    let bindings = native_name_bindings(&owner, &["--name".into()], &recognized);
    assert_eq!(
        bindings[0].occurrences,
        [
            EntryForm {
                parts: vec![slice(0, &[0], 7..9), slice(0, &[2], 0..4)],
            },
            EntryForm {
                parts: vec![slice(1, &[0], 0..6)],
            },
        ]
    );
    for occurrence in &bindings[0].occurrences {
        assert_eq!(bound_text(&owner, occurrence), "--name");
    }
}

#[test]
fn names_are_not_discovered_from_links_body_or_unrecognized_terms() {
    let mut owner = item(vec![vec![Inline::Link {
        target: LinkTarget::External {
            uri: "https://example.org/--hidden".into(),
        },
        title: Some("--hidden".into()),
        children: vec![text("--visible")],
    }]]);
    owner.description.push(mant_ir::Block::Paragraph {
        children: vec![text("--hidden --visible")],
        layout: mant_ir::LayoutHint::default(),
        source: None,
    });
    let bindings = native_name_bindings(
        &owner,
        &["--hidden".into(), "--visible".into(), String::new()],
        &[vec![RecognizedName::contiguous("--visible", 0)]],
    );
    assert_eq!(bindings[0].occurrences, []);
    assert_eq!(bindings[1].occurrences.len(), 1);
    assert_eq!(bindings[2].occurrences, []);
    let no_evidence = native_name_bindings(&owner, &["--visible".into()], &[]);
    assert_eq!(no_evidence[0].occurrences, []);
}

#[test]
fn borrowed_head_adapter_produces_the_same_bindings_without_a_temporary_owner() {
    let owner = item(vec![vec![
        text("prefix "),
        Inline::Code {
            value: "--界".into(),
        },
    ]]);
    let names = vec!["--界".into()];
    let recognized = vec![vec![RecognizedName::contiguous("--界", 7)]];
    assert_eq!(
        native_name_bindings_for_head(&owner.terms[0], &names, &recognized),
        native_name_bindings(&owner, &names, &recognized)
    );
}
