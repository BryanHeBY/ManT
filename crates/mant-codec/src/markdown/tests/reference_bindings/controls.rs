//! Required normal controls and identity/cache/metadata interactions.
use super::*;

#[test]
fn original_first_definition_normalization_and_order_survive_label_recovery() {
    let mut count = 0;
    for ending in ["\n", "\r\n", "\r"] {
        for before in [false, true] {
            for (use_id, definition_id) in [
                ("r", "r"),
                ("ReF", "ref"),
                (" R\t F ", "r f"),
                ("RÉF", "Réf"),
                ("中文", "中文"),
            ] {
                let use_ = format!("[a\x1bb][{use_id}]");
                let definitions = format!(
                    "[{definition_id}]: https://same.test \"before&#12;after\"\n\
                     [{definition_id}]: https://shadow.test \"unused&#12;\"\n"
                );
                let source = if before {
                    format!("{definitions}\n{use_}\n")
                } else {
                    format!("{use_}\n\n{definitions}")
                }
                .replace('\n', ending);
                let document = parse(&source);
                let actual = reading(&document);
                assert_eq!(actual.text, "a b", "{source:?}");
                assert_eq!(actual.links.len(), 1, "{source:?}");
                assert_eq!(actual.links[0].1.as_deref(), Some("before after"));
                assert_eq!(decoded_findings(&document).len(), 1);
                assert_eq!(
                    findings(&document, "markdown.recovered-link-target"),
                    [] as [&mant_ir::Diagnostic; 0]
                );
                let definition =
                    format!("[{definition_id}]: https://same.test \"before&#12;after\"");
                let start = source.find(&definition).unwrap();
                assert_span(
                    &source,
                    decoded_findings(&document)[0].source.unwrap(),
                    start..start + definition.len(),
                );
                assert_two_reads(&document, "a b", 1);
                count += 1;
            }
        }
    }
    assert_eq!(count, 30);
}

#[test]
fn collision_refusal_does_not_reserve_later_effective_title_diagnostics() {
    for ending in ["\n", "\r\n", "\r"] {
        for context in ["paragraph", "canonical", "heading", "list", "table"] {
            let use_ = "[DECLINED][r\x1bf] [ACCEPTED][r f] [LATER][r f] [OTHER][s]";
            let body = match context {
                "paragraph" => format!("{use_}\n"),
                "canonical" => format!("<br />\n{use_}\n"),
                "heading" => format!("# {use_}\n"),
                "list" => format!("- {use_}\n"),
                "table" => format!("| {use_} |\n| --- |\n| END |\n"),
                _ => unreachable!(),
            };
            let first = "[r f]: https://same.test \"same&#12;title\"";
            let second = "[s]: https://same.test \"same&#12;title\"";
            let source = format!(
                "{body}\n{first}\n[r\x1bf]: https://same.test \"same&#12;title\"\n{second}\n"
            )
            .replace('\n', ending);
            let document = parse(&source);
            let actual = reading(&document);
            assert_eq!(actual.links.len(), 3, "{source:?}");
            assert!(
                actual.links.iter().all(|link| {
                    link.1.as_deref() == Some("same title") && link.2 != "DECLINED"
                })
            );
            assert!(actual.text.contains("DECLINED ACCEPTED LATER OTHER"));
            assert_eq!(
                findings(&document, "markdown.recovered-link-target").len(),
                1
            );
            let decoded = decoded_findings(&document);
            assert_eq!(decoded.len(), 2, "{source:?}");
            for (finding, definition) in decoded.into_iter().zip([first, second]) {
                let start = source.find(definition).unwrap();
                assert_span(
                    &source,
                    finding.source.unwrap(),
                    start..start + definition.len(),
                );
            }
        }
    }
}

#[test]
fn distinct_damaged_uses_cannot_reuse_equal_uri_and_title_proofs() {
    for reverse in [false, true] {
        let mut references = ['\x1b', '\x01', '\x7f'];
        if reverse {
            references.reverse();
        }
        let mut uses = String::new();
        let mut definitions = String::new();
        for (index, control) in references.into_iter().enumerate() {
            use std::fmt::Write;
            write!(uses, "[DECLINED{index}][r{control}f] ").unwrap();
            writeln!(definitions, "[r{control}f]: https://same.test \"same\"").unwrap();
        }
        let source =
            format!("{uses}[ACCEPTED][r f]\n\n[r f]: https://same.test \"same\"\n{definitions}");
        let document = parse(&source);
        let actual = reading(&document);
        assert_eq!(actual.links.len(), 1, "{source:?}");
        assert_eq!(actual.links[0].2, "ACCEPTED");
        assert_eq!(
            findings(&document, "markdown.recovered-link-target").len(),
            3
        );
        assert_eq!(decoded_findings(&document), [] as [&mant_ir::Diagnostic; 0]);
    }
}

#[test]
fn normal_controls_keep_their_labels_targets_and_unused_metadata_inactive() {
    for (source, text, link_count) in [
        ("[plain][r]\n\n[r]: https://same.test\n", "plain", 1),
        ("[a\x1bb][r]\n\n[r]: https://same.test\n", "a b", 1),
        (
            "[plain][r]\n\n[r]: https://same.test \"a\x1bb\"\n",
            "plain",
            1,
        ),
        ("[a\x1bb](https://same.test \"same\")", "a b", 1),
        (
            "[plain][r]\n\n[r]: https://same.test/%20/%1B\n\nTAIL\x1bEND",
            "plainTAIL END",
            1,
        ),
        (
            "[missing][none]\n\n[unused]: https://unused.test \"unused&#12;\"\n\nEND\x1bTAIL",
            "[missing][none]END TAIL",
            0,
        ),
    ] {
        let document = parse(source);
        let actual = reading(&document);
        assert_eq!(actual.text, text, "{source:?}");
        assert_eq!(actual.links.len(), link_count, "{source:?}");
        assert_eq!(
            findings(&document, "markdown.recovered-link-target"),
            [] as [&mant_ir::Diagnostic; 0]
        );
        assert_eq!(decoded_findings(&document), [] as [&mant_ir::Diagnostic; 0]);
    }
}

#[test]
fn changed_labels_share_one_reference_table_without_per_use_document_parses() {
    for count in [1, 100, 1000] {
        let source = format!(
            "{}\n\n[r]: https://same.test \"same&#12;title\"\n",
            "[a\x1bb][r] ".repeat(count)
        );
        let document = parse(&source);
        let actual = reading(&document);
        assert_eq!(actual.links.len(), count);
        assert!(actual.links.iter().all(|link| link.2 == "a b"));
        assert_eq!(decoded_findings(&document).len(), 1);
        assert_eq!(
            findings(&document, "markdown.recovered-link-target"),
            [] as [&mant_ir::Diagnostic; 0]
        );
    }
}

#[test]
fn original_and_recovered_reference_sources_share_preface_and_bom_boundaries() {
    let prefix = "\u{feff}<!-- mant:tldr:start -->\n# tool\n\n> Quick reference.\n\n- Run:\n\n`tool`\n\n[r f]: https://preface.test \"preface&#12;\"\n<!-- mant:tldr:end -->\n\n";
    for ending in ["\n", "\r\n", "\r"] {
        let source = format!(
            "{prefix}# 中\n\n## Section\n[a\x1bb][r] [DECLINED][r\x1bf]\n\n[r]: https://safe.test\n[r f]: https://same.test \"WRONG&#12;\"\n[r\x1bf]: https://same.test \"RIGHT\"\n"
        )
        .replace('\n', ending);
        let document = parse(&source);
        assert_eq!(reading(&document).links.len(), 1);
        assert_eq!(decoded_findings(&document), [] as [&mant_ir::Diagnostic; 0]);
        let refused = findings(&document, "markdown.recovered-link-target");
        assert_eq!(refused.len(), 1);
        let use_ = "[DECLINED][r\x1bf]";
        let start = source.find(use_).unwrap();
        assert_span(
            &source,
            refused[0].source.unwrap(),
            start..start + use_.len(),
        );
    }
}

#[test]
fn table_specific_reference_spelling_is_not_guessed_during_local_proof() {
    use pulldown_cmark::{Event, LinkType, Parser, Tag};
    let definition = "[r|f]: https://same.test \"same\"";
    for (use_, suffix, expected_label, expected_links, refused) in [
        ("[ab][r\\|f]", "", "ab", 1, 0),
        ("[ab][r\\|f]", "\nTAIL\x1bEND\n", "ab", 1, 0),
        ("[a\x1bb][r\\|f]", "", "a b", 0, 1),
    ] {
        let source = format!("| {use_} |\n| --- |\n| END |\n\n{definition}\n{suffix}");
        let original = Parser::new_ext(&source, super::super::super::markdown_options());
        let definitions = original.reference_definitions();
        assert_eq!(
            definitions.get("r|f").unwrap().dest.as_ref(),
            "https://same.test"
        );
        assert!(definitions.get("r\\|f").is_none());
        let original_links = original
            .filter_map(|event| match event {
                Event::Start(Tag::Link { link_type, id, .. }) => {
                    Some((link_type, id.into_string()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(original_links, [(LinkType::Reference, "r|f".into())]);
        // Locked linklabel.rs only removes the escaped pipe when is_in_table.
        // A local ordinary use parser cannot prove that identity. Conservatively
        // retain the label and diagnose, rather than rewrite the ID or keep a
        // second full-page event history. A clean use is not locally reparsed.
        let document = parse(&source);
        let actual = reading(&document);
        assert!(actual.text.starts_with(expected_label));
        assert_eq!(actual.links.len(), expected_links);
        assert_eq!(
            findings(&document, "markdown.recovered-link-target").len(),
            refused
        );
        if refused > 0 {
            let start = source.find(use_).unwrap();
            assert_span(
                &source,
                findings(&document, "markdown.recovered-link-target")[0]
                    .source
                    .unwrap(),
                start..start + use_.len(),
            );
        }
    }
    let use_ = "[X][r\\|\x1bf]";
    let source = format!(
        "| {use_} |\n| --- |\n| END |\n\n[r| f]: https://same.test \"same\"\n[r|\x1bf]: https://same.test \"same\"\n"
    );
    let parser = Parser::new_ext(&source, super::super::super::markdown_options());
    let original = parser.reference_definitions().get("r|\x1bf").unwrap();
    let other = parser.reference_definitions().get("r| f").unwrap();
    assert_ne!(original.span, other.span);
    assert_eq!(original.dest, other.dest);
    assert_eq!(original.title, other.title);
    let document = parse(&source);
    assert_eq!(reading(&document).links, []);
    assert_eq!(
        findings(&document, "markdown.recovered-link-target").len(),
        1
    );
    assert_eq!(decoded_findings(&document), [] as [&mant_ir::Diagnostic; 0]);
}
