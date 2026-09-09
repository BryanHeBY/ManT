//! Contract-focused tests for Markdown lowering and source preservation.

use mant_ir::{Block, EntryKind, Inline, ListKind, SourceFormat, TableAlignment, TldrOrigin};

use super::{parse_document, parse_markdown};

mod entry_forms;
mod source_contracts;

#[test]
fn classifies_mailto_schemes_without_ascii_case_distinctions() {
    let document = parse_document(
        "[mail](MAILTO:user@example.test \"mail title\") [subject](mailto:user@example.test?subject=hello)\n",
        Some("/docs/tool.md".to_owned()),
    );
    let Block::Paragraph { children, .. } = &document.blocks[0] else {
        panic!("link paragraph");
    };
    assert!(matches!(
        &children[0],
        Inline::Link {
            target: mant_ir::LinkTarget::Email { address },
            title: Some(title),
            ..
        } if address == "user@example.test" && title == "mail title"
    ));
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link {
            target: mant_ir::LinkTarget::External { uri },
            ..
        } if uri == "mailto:user@example.test?subject=hello"
    )));
}

#[test]
fn classifies_mailto_only_after_decoding_and_validating_the_mailbox() {
    let document = parse_document(
        "[percent](mailto:user%25tag@example.test) \
         [slash](mailto:a%2Fb@example.test) \
         [leading-dot](mailto:%2Euser@example.test) \
         [double-dot](mailto:user%2E%2Ename@example.test) \
         [extra-at](mailto:user%40evil@example.test) \
         [query-leading-dot](mailto:%2Euser@example.test?subject=x) \
         [query-double-dot](mailto:user%2E%2Ename@example.test?subject=x) \
         [query-extra-at](mailto:user%40evil@example.test?subject=x) \
         [fragment-leading-dot](mailto:%2Euser@example.test#fragment) \
         [recipients](mailto:user@example.test,second@example.test)\n",
        Some("/docs/tool.md".to_owned()),
    );
    let Block::Paragraph { children, .. } = &document.blocks[0] else {
        panic!("link paragraph");
    };
    let targets = children
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link { target, .. } => Some(target),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        targets[0],
        mant_ir::LinkTarget::Email { address } if address == "user%tag@example.test"
    ));
    assert!(matches!(
        targets[1],
        mant_ir::LinkTarget::Email { address } if address == "a/b@example.test"
    ));
    for (target, uri) in targets[2..9].iter().zip([
        "mailto:%2Euser@example.test",
        "mailto:user%2E%2Ename@example.test",
        "mailto:user%40evil@example.test",
        "mailto:%2Euser@example.test?subject=x",
        "mailto:user%2E%2Ename@example.test?subject=x",
        "mailto:user%40evil@example.test?subject=x",
        "mailto:%2Euser@example.test#fragment",
    ]) {
        assert!(matches!(
            target,
            mant_ir::LinkTarget::External { uri: actual } if actual == uri
        ));
    }
    assert!(matches!(
        targets[9],
        mant_ir::LinkTarget::External { uri }
            if uri == "mailto:user@example.test,second@example.test"
    ));
    assert_eq!(
        document
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code.as_deref() == Some("ir.invalid-external-uri"))
            .count(),
        7
    );
}

#[test]
fn heading_attributes_consume_only_an_explicit_id() {
    let document = parse_document(
        "# API Reference\n\n\
         ## GET /users/{id}\n\
         ## POST /orders/{orderId}/items/{itemId}\n\
         ## Config {#config-anchor}\n\
         ## Template {{placeholder}}\n\
         ## Route /users/{#id}\n\
         ## Shell ${HOME} expansion\n",
        None,
    );
    let titles = document
        .sections
        .iter()
        .map(|section| (section.heading.plain_text(), section.id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        titles
            .iter()
            .map(|(title, id)| (title.as_str(), *id))
            .collect::<Vec<_>>(),
        vec![
            ("GET /users/{id}", "get-users-id"),
            (
                "POST /orders/{orderId}/items/{itemId}",
                "post-orders-orderid-items-itemid"
            ),
            ("Config", "config-anchor"),
            ("Template {{placeholder}}", "template-placeholder"),
            ("Route /users/{#id}", "route-users-id"),
            ("Shell ${HOME} expansion", "shell-home-expansion"),
        ]
    );
    assert_eq!(
        document.sections[2]
            .fragment_aliases
            .iter()
            .map(mant_ir::FragmentAlias::as_str)
            .collect::<Vec<_>>(),
        ["config-anchor"]
    );
}

#[test]
fn unsupported_math_does_not_leak_markdown_bracket_escapes() {
    let document = parse_document(
        "# Variables\n\n\
         ## $a (\\[$b\\])\n\
         ## a (\\[$b\\])\n\
         ## $a (\\[b\\])\n\
         ## \\[$b\\]\n\
         ## $a \\[$b\\]\n",
        None,
    );
    assert_eq!(
        document
            .sections
            .iter()
            .map(|section| section.heading.plain_text())
            .collect::<Vec<_>>(),
        vec!["$a ([$b])", "a ([$b])", "$a ([b])", "[$b]", "$a [$b]"]
    );
    assert!(document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.unsupported")
            && diagnostic.message.contains("math")
    }));
}

#[test]
fn markdown_extension_does_not_turn_uri_schemes_or_authorities_into_documents() {
    for uri in [
        "https://example.md",
        "https://example.test/path.md#fragment",
        "HTTP://example.markdown",
        "ftp://host/manual.md",
        "custom:manual.md",
        "//example.md/path.md",
        "C:/manual.md",
    ] {
        let document = parse_document(&format!("[LINK]({uri})\n"), None);
        let Block::Paragraph { children, .. } = &document.blocks[0] else {
            panic!("link paragraph")
        };
        assert!(
            matches!(&children[0], Inline::Link { target: mant_ir::LinkTarget::External { uri: actual }, .. } if actual == uri),
            "{uri}: {children:?}"
        );
    }
}

#[test]
fn preserves_unsupported_constructs_as_exact_source_with_diagnostics() {
    let markdown = "\
# Unsupported

> quoted **text**

- [x] finished

Text with ~~strike~~, ![alt](image.png), <kbd>raw</kbd>, and $math$.

[^note]: footnote body
";
    let document = parse_document(markdown, None);
    assert!(document.sections.is_empty());
    let blocks = &document.blocks;

    assert!(matches!(
        &blocks[0],
        Block::Unsupported { name: Some(name), text, .. }
            if name == "block quote" && text == "> quoted **text**\n"
    ));
    assert!(matches!(
        &blocks[1],
        Block::Unsupported { name: Some(name), text, .. }
            if name == "task list" && text == "- [x] finished\n\n"
    ));
    let Block::Paragraph { children, .. } = &blocks[2] else {
        panic!("mixed unsupported inline syntax remains in its paragraph");
    };
    let visible = children
        .iter()
        .filter_map(|inline| match inline {
            Inline::Text { value } => Some(value.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert!(visible.contains("~~strike~~"));
    assert!(visible.contains("![alt](image.png)"));
    assert!(visible.contains("<kbd>raw</kbd>"));
    assert!(visible.contains("$math$"));
    assert!(matches!(
        &blocks[3],
        Block::Unsupported { name: Some(name), text, .. }
            if name == "footnote definition" && text.contains("[^note]: footnote body")
    ));
    assert!(document.diagnostics.len() >= 7);
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.source.is_some())
    );
}

#[test]
fn separates_a_leading_tldr_directive_from_the_document_ir() {
    let parsed = parse_markdown(
        "\
<!-- mant:tldr:start -->
# demo

> A demonstration command.

- Show command help:

`demo --help`
<!-- mant:tldr:end -->

# Demo

Document introduction.

## Same

## Same
",
        None,
    )
    .expect("embedded tldr");

    let tldr = parsed.tldr.expect("quick reference");
    assert_eq!(tldr.title, "demo");
    assert_eq!(tldr.description, ["A demonstration command."]);
    assert_eq!(tldr.examples[0].description, "Show command help");
    assert_eq!(tldr.examples[0].command, "demo --help");
    assert_eq!(tldr.origin, TldrOrigin::Embedded);

    assert_eq!(parsed.document.display_title().as_deref(), Some("Demo"));
    assert!(matches!(
        parsed.document.blocks.as_slice(),
        [Block::Paragraph { children, source, .. }]
            if matches!(children.as_slice(), [Inline::Text { value }] if value == "Document introduction.")
                && source.is_some_and(|span| span.line == 13)
    ));
    assert_eq!(parsed.document.sections[0].id, "same");
    assert_eq!(parsed.document.sections[1].id, "same-2");
}

#[test]
fn a_reference_to_a_duplicated_heading_resolves_to_the_first_section() {
    let markdown = "\
# Guide

## Options

See [more options](#options).

## Options

Duplicate heading.
";
    let document = parse_document(markdown, None);

    assert_eq!(document.sections[0].id, "options");
    assert_eq!(document.sections[1].id, "options-2");

    // The bare `#options` anchor renders on the first section, so an ambiguous
    // link must resolve there rather than to the later disambiguated duplicate.
    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("first Options section holds the reference paragraph");
    };
    assert!(
        children.iter().any(|inline| matches!(
            inline,
            Inline::Link { target: mant_ir::LinkTarget::Section { id: target }, .. } if target == "options"
        )),
        "a #options link must resolve to the first section, not options-2"
    );
}

#[test]
fn a_leading_byte_order_mark_hides_neither_the_directive_nor_the_title() {
    let parsed = parse_markdown(
        "\u{feff}<!-- mant:tldr:start -->\n# demo\n\n> Saved by a Windows editor.\n\n- Run:\n\n`demo`\n<!-- mant:tldr:end -->\n\n# Demo\n\nBody.\n",
        None,
    )
    .expect("embedded tldr behind a BOM");
    assert_eq!(parsed.tldr.expect("quick reference").title, "demo");
    assert_eq!(parsed.document.display_title().as_deref(), Some("Demo"));

    let plain = parse_markdown("\u{feff}# Demo\n\nBody.\n", None).expect("plain document");
    assert!(plain.tldr.is_none());
    assert_eq!(plain.document.display_title().as_deref(), Some("Demo"));
}

#[test]
fn terminal_control_characters_are_masked_with_a_diagnostic() {
    let parsed = parse_markdown(
        "# Demo\n\nx\u{1b}]0;EVIL\u{7}y \u{1b}[31mred\u{1b}[0m z\u{8}\u{8}\n",
        None,
    )
    .expect("document with control characters");
    let document = &parsed.document;

    assert!(
        document
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("markdown.control-characters"))
    );
    let Block::Paragraph { children, .. } = &document.blocks[0] else {
        panic!("prose survives sanitizing");
    };
    let Inline::Text { value } = &children[0] else {
        panic!("text inline survives sanitizing");
    };
    assert!(!value.contains('\u{1b}') && !value.contains('\u{8}') && !value.contains('\u{7}'));
    assert!(value.contains("red") && value.contains('z'));
}

#[test]
fn leaves_an_ordinary_tldr_heading_in_the_manual() {
    let document = parse_document(
        "\
# Demo

## Synopsis

Normal manual content.

## TLDR

- This late heading is ordinary content:

`demo --help`
",
        None,
    );

    assert_eq!(document.sections[1].heading.plain_text(), "TLDR");
    assert_eq!(
        document.sections[1].id, "tldr-section",
        "an ordinary TLDR heading must not shadow the reserved tldr selector"
    );
}

#[test]
fn reserved_selectors_never_shadow_section_ids() {
    let document = parse_document(
        "# Demo\n\n## root\n\nA.\n\n## document-overview\n\nB.\n\n## 1\n\nC.\n",
        None,
    );

    let ids: Vec<&str> = document
        .sections
        .iter()
        .map(|section| section.id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["root-section", "document-overview-section", "1-section"]
    );
}

mod entries;
mod navigation;
mod tables;
