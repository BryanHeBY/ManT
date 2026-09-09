//! Markdown import combined with public query and presentation contracts.
use super::*;

#[test]
fn explicit_heading_ids_cannot_shadow_paths_and_remain_link_targets() {
    let document = parse_document(
        "\
# Demo

See [entry owner](#1/e1), [path owner](#3.1), and [explicit root](#root).

## Entry owner {#1/e1}

- `--help`: Show help.

## Path owner {#3.1}

Path owner body.

## Parent

### Child

Child body.

## Explicit root {#root}

Root body.
",
        None,
    );

    assert_eq!(document.sections[0].id, "entry-owner");
    assert_eq!(document.sections[1].id, "path-owner");
    assert_eq!(document.sections[2].children[0].id, "child");
    assert_eq!(document.sections[3].id, "explicit-root");
    let Block::Paragraph { children, .. } = &document.blocks[0] else {
        panic!("document preface contains source links");
    };
    let targets = children
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link {
                target: mant_ir::LinkTarget::Section { id: target },
                ..
            } => Some(target.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        targets,
        ["entry-owner", "path-owner", "explicit-root"],
        "renamed explicit IDs remain valid Markdown link aliases"
    );
    assert_eq!(
        document.sections[0]
            .fragment_aliases
            .iter()
            .map(mant_ir::FragmentAlias::as_str)
            .collect::<Vec<_>>(),
        ["1/e1"]
    );
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_deref() != Some("markdown.unresolved-reference"))
    );

    let query = ResolvedContent {
        address: None,
        label: "demo.md".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let entry = select_excerpt(&query, &[mant_protocol::ContentSelector::path("1/e1")])
        .expect("entry path");
    assert!(matches!(
        entry.selections.as_slice(),
        [selection @ ExcerptSelection::DocumentEntry { .. }]
            if selection.outline().title().contains("--help")
    ));
    let child =
        select_excerpt(&query, &[mant_protocol::ContentSelector::path("3.1")]).expect("child path");
    assert!(matches!(
        child.selections.as_slice(),
        [selection @ ExcerptSelection::DocumentSection { .. }]
            if selection.outline().title() == "Child"
    ));
}
