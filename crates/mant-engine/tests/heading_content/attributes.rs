use mant_codec::encode::render_markdown;
use mant_ir::Inline;
use mant_loader::{load_markdown_text, load_roff_bytes};

#[test]
fn only_source_separated_literals_become_heading_attributes() {
    for (title, expected, authored) in [
        ("**Caption**{#id}", "Caption{#id}", false),
        ("\x60Caption\x60{#id}", "Caption{#id}", false),
        (
            "[Caption](https://example.test){#id}",
            "Caption{#id}",
            false,
        ),
        ("**Caption {#id}**", "Caption {#id}", false),
        (r"Caption \{#id}", "Caption {#id}", false),
        (r"Caption {\#id}", "Caption {#id}", false),
        (r"Caption {#id\}", "Caption {#id}", false),
        ("Caption &#123;#id&#125;", "Caption {#id}", false),
        ("Caption&nbsp;{#id}", "Caption\u{a0}{#id}", false),
        ("Caption C#", "Caption C#", false),
        (r"Caption \###", "Caption ###", false),
        ("**Caption** {#id}", "Caption", true),
        ("\x60Caption\x60 {#id} ###", "Caption", true),
        ("[Caption](https://example.test) {#id}", "Caption", true),
    ] {
        for level in 1..=3 {
            let head = "#".repeat(level);
            let prefix = match level {
                1 => "",
                2 => "# Probe\n\n",
                _ => "# Probe\n\n## Parent\n\n",
            };
            let mut content =
                load_markdown_text(&format!("{prefix}{head} {title}\n\nBODY.\n"), None).unwrap();
            let get = |document: &mant_ir::Document| {
                let (heading, aliases) = match level {
                    1 => (
                        document.heading.as_ref().unwrap(),
                        document.fragment_aliases.as_slice(),
                    ),
                    2 => {
                        let section = &document.sections[0];
                        (&section.heading, section.fragment_aliases.as_slice())
                    }
                    _ => {
                        let section = &document.sections[0].children[0];
                        (&section.heading, section.fragment_aliases.as_slice())
                    }
                };
                (
                    heading.content.clone(),
                    aliases.iter().any(|alias| alias.as_str() == "id"),
                )
            };
            let (original, alias) = get(content.document.as_ref().unwrap());
            assert_eq!(mant_ir::inline_plain_text(&original), expected, "{title}");
            assert_eq!(alias, authored, "{title}");
            if title.starts_with("**") {
                assert!(matches!(original.first(), Some(Inline::Strong { .. })));
            } else if title.starts_with('[') {
                assert!(matches!(original.first(), Some(Inline::Link { .. })));
            } else if title.starts_with('\x60') {
                assert!(matches!(original.first(), Some(Inline::Code { .. })));
            }
            for _ in 0..2 {
                let exported = render_markdown(&content);
                content = load_markdown_text(&exported, None).unwrap();
                let (heading, alias) = get(content.document.as_ref().unwrap());
                assert_eq!(heading, original, "{title}: {exported}");
                assert!(
                    !alias,
                    "portable output did not author an attribute: {exported}"
                );
            }
        }
    }
}

#[test]
fn source_attributes_can_span_parsed_style_tokens_and_keep_setext_hard_rows() {
    for source in [
        "Caption\\\n{#_foo_}\n=======\n",
        "Caption  \n{#_foo_}\n=======\n",
        "Caption\n{#_foo_}\n=======\n",
    ] {
        let content = load_markdown_text(source, None).unwrap();
        let document = content.document.as_ref().unwrap();
        assert_eq!(
            document.heading.as_ref().unwrap().plain_text(),
            if source.contains("\\\n") || source.contains("  \n") {
                "Caption\n"
            } else {
                "Caption"
            },
        );
        assert!(
            document
                .fragment_aliases
                .iter()
                .any(|alias| alias.as_str() == "_foo_")
        );
    }
}

#[test]
fn roff_heading_literals_survive_two_markdown_roundtrips() {
    // CVS man_term.c::pre_SH/pre_SS preserve their heading operands, and
    // man_validate.c::post_SH tags the complete authored words.
    let source = ".TH PROBE 1\n.SH \"Caption {#id}\"\nfirst body\n.SH \"Caption ###\"\nsecond body\n.SH \"Caption C#\"\nthird body\n.SH \"Caption\"\n.SS \"Child {#id}\"\nchild body\n.SS \"Child ###\"\nlast body\n";
    let mut content = load_roff_bytes(source.as_bytes()).unwrap();
    for round in 0..=2 {
        let document = content.document.as_ref().unwrap();
        let titles = document
            .sections
            .iter()
            .map(|section| section.heading.plain_text())
            .collect::<Vec<_>>();
        assert_eq!(
            titles,
            ["Caption {#id}", "Caption ###", "Caption C#", "Caption"],
            "round {round}"
        );
        let children = &document.sections[3].children;
        assert_eq!(
            children
                .iter()
                .map(|section| section.heading.plain_text())
                .collect::<Vec<_>>(),
            ["Child {#id}", "Child ###"],
        );
        for section in document.sections.iter().chain(children) {
            assert!(
                !section
                    .fragment_aliases
                    .iter()
                    .any(|alias| alias.as_str() == "id")
            );
        }
        content = load_markdown_text(&render_markdown(&content), None).unwrap();
    }
}
