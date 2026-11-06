//! Raw-source display recovery must not establish a different target identity.
use super::parse_markdown;
use mant_ir::{
    Document, Inline, LinkTarget,
    visit::{Visit, walk_inline},
};

#[derive(Default)]
struct Links(Vec<(LinkTarget, Option<String>, String)>);
impl<'a> Visit<'a> for Links {
    fn visit_inline(&mut self, node: &'a Inline) {
        if let Inline::Link {
            target,
            title,
            children,
        } = node
        {
            self.0.push((
                target.clone(),
                title.clone(),
                mant_ir::inline_plain_text(children),
            ));
        }
        walk_inline(self, node);
    }
}
fn links(document: &Document) -> Vec<(LinkTarget, Option<String>, String)> {
    let mut links = Links::default();
    links.visit_document(document);
    links.0
}
fn check(source: &str, expected_links: usize, refused: usize) -> Document {
    let document = parse_markdown(source, None).unwrap().document;
    assert_eq!(links(&document).len(), expected_links, "{source:?}");
    let warnings = document
        .diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("markdown.recovered-link-target"))
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), refused, "{source:?}");
    for warning in warnings {
        let r = warning.source.unwrap().byte_range.unwrap();
        assert!(
            source
                .get(r.start.get() as usize..r.end.get() as usize)
                .is_some()
        );
        assert_eq!(warning.impact, mant_ir::DiagnosticImpact::None);
    }
    assert_eq!(
        serde_json::from_str::<Document>(&serde_json::to_string(&document).unwrap()).unwrap(),
        document
    );
    document
}

#[test]
fn target_label_and_title_source_recovery_have_independent_admission() {
    let mut cases = 0;
    for control in ['\0', '\x0c', '\x1b', '\u{85}', '\u{9b}'] {
        for prefix in ["", "<br />\n", "- <br />\n  "] {
            // FF is already parser whitespace beside a destination; masking
            // it there preserves identity. Inside angle destinations it is
            // content, so recovery cannot establish the changed address.
            for (link, kept) in [
                (
                    format!("[label](https://a.test/{control})"),
                    usize::from(control == '\x0c'),
                ),
                (format!("[label](<https://a.test/a{control}b>)"), 0),
                (
                    format!("[label]({control}https://a.test)"),
                    usize::from(control == '\x0c'),
                ),
            ] {
                let document = check(&format!("{prefix}{link}"), kept, 1 - kept);
                let markdown = crate::encode::render_blocks_fragment(
                    &document.blocks,
                    crate::encode::MarkdownFragmentOptions::default(),
                )
                .join("\n\n");
                let read = parse_markdown(&markdown, None).unwrap().document;
                assert_eq!(links(&read).len(), kept);
                assert!(markdown.contains("label"));
                cases += 1;
            }
            let document = check(&format!("{prefix}[a{control}b](https://a.test)"), 1, 0);
            assert_eq!(
                links(&document)[0].2,
                format!("a{}b", " ".repeat(control.len_utf8()))
            );
            let document = check(
                &format!("{prefix}[label](https://a.test \"a{control}b\")"),
                1,
                0,
            );
            assert_eq!(
                links(&document)[0].1,
                Some(format!("a{}b", " ".repeat(control.len_utf8())))
            );
            check(
                &format!("{prefix}[label](https://a.test) {control} after"),
                1,
                0,
            );
        }
    }
    assert_eq!(cases, 45);
}

#[test]
fn reference_recovery_uses_original_effective_definition_and_reference_id() {
    for ending in ["\n", "\r\n", "\r"] {
        for use_ in ["[x][r]", "[r][]", "[r]"] {
            for prefix in ["", "<br />\n", "- <br />\n  "] {
                check(
                    &format!("{prefix}{use_} {use_}\n\n[r]: https://a.test/\x1b\n")
                        .replace('\n', ending),
                    0,
                    1,
                );
                let source =
                    format!("{prefix}{use_} {use_}\n\n[r]: https://a.test \"before\x1bafter\"\n")
                        .replace('\n', ending);
                let document = check(&source, 2, 0);
                assert!(
                    links(&document)
                        .iter()
                        .all(|link| link.1.as_deref() == Some("before after"))
                );
            }
        }
        check(
            &"[x][r\x1bref]\n\n[r ref]: https://a.test\n".replace('\n', ending),
            0,
            1,
        );
        check(
            &"[x][r ref]\n\n[r\x1bref]: https://a.test\n".replace('\n', ending),
            0,
            1,
        );
        check(
            &"[a\x1bb][r]\n\n[r]: https://a.test\n".replace('\n', ending),
            1,
            0,
        );
        check(&"[x][r]\n\n[r]: https://a.test\n[r]: https://bad.test/\x1b\n[unused]: https://bad.test/\x1b\n".replace('\n', ending), 1, 0);
    }
}

#[test]
fn recovery_cannot_activate_a_later_originally_shadowed_definition() {
    for control in ['\u{85}', '\u{9b}'] {
        let source = format!("[x][r]\n\n[r]: {control}\n\n[r]: https://other.test\n");
        // Locked firstpass::scan_refdef accepts the non-ASCII control operand;
        // scan_link_dest does not treat its UTF-8 bytes as ASCII whitespace.
        let parser = pulldown_cmark::Parser::new(&source);
        assert_eq!(
            parser
                .reference_definitions()
                .get("r")
                .unwrap()
                .dest
                .as_ref(),
            control.to_string()
        );
        let document = check(&source, 0, 1);
        let warning = document
            .diagnostics
            .iter()
            .find(|d| d.code.as_deref() == Some("markdown.recovered-link-target"))
            .unwrap();
        let r = warning.source.unwrap().byte_range.unwrap();
        assert_eq!(
            &source[r.start.get() as usize..r.end.get() as usize],
            format!("[r]: {control}")
        );
    }
    check("\u{feff}[x][r]\n\n[r]: https://a.test\n", 1, 0);
}

#[test]
fn original_reference_checks_share_the_embedded_preface_boundary() {
    let source = "<!-- mant:tldr:start -->\n# tool\n\n> Quick reference.\n\n- Run:\n\n`tool`\n\n[r]: https://preface.test\n<!-- mant:tldr:end -->\n\n[x][r]\n\n[r]: https://body.test\n\ntail\x1b\n";
    let document = check(source, 1, 0);
    assert_eq!(
        links(&document)[0].0,
        LinkTarget::External {
            uri: "https://body.test".into()
        }
    );
}

fn control_warning_sources(document: &Document, source: &str) -> Vec<String> {
    document
        .diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("markdown.control-characters"))
        .map(|diagnostic| {
            let range = diagnostic.source.unwrap().byte_range.unwrap();
            source[range.start.get() as usize..range.end.get() as usize].to_owned()
        })
        .collect()
}

#[test]
fn declined_targets_do_not_execute_repaired_or_discarded_titles() {
    let mut cases = 0;
    for ending in ["\n", "\r\n", "\r"] {
        for prefix in ["", "<br />\n"] {
            for (link, definitions, original_control) in [
                (
                    "[DECLINED][r]",
                    "[r]: \u{85}\n\n[r]: https://other.test \"before&#12;after\"\n",
                    "\u{85}",
                ),
                (
                    "[DECLINED][r\x1bf]",
                    "[r f]: https://other.test \"before&#12;after\"\n",
                    "\x1b",
                ),
                (
                    "[DECLINED](https://a.test/\x1b \"before&#12;after\")",
                    "",
                    "\x1b",
                ),
            ] {
                let source = format!("{prefix}{link}\n\n{definitions}").replace('\n', ending);
                let document = check(&source, 0, 1);
                // Original source recovery still warns about its raw control.
                // The rejected Link's decoded title is not retained or visited,
                // including a formerly shadowed definition exposed by masking.
                assert_eq!(
                    control_warning_sources(&document, &source),
                    [original_control],
                    "{source:?}"
                );
                assert!(document.blocks.iter().any(|block| matches!(block,
                    mant_ir::Block::Paragraph { children, .. }
                        if mant_ir::inline_plain_text(children).contains("DECLINED"))));
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 18);
}

#[test]
fn a_later_admitted_reference_still_normalizes_its_title_once() {
    for ending in ["\n", "\r\n", "\r"] {
        for prefix in ["", "<br />\n"] {
            let definition = "[r f]: https://other.test \"before&#12;after\"";
            let source = format!(
                "{prefix}[DECLINED][r\x1bf] [ACCEPTED][r f] [LATER][r f]\n\n{definition}\n"
            )
            .replace('\n', ending);
            let document = check(&source, 2, 1);
            assert_eq!(
                links(&document),
                ["ACCEPTED", "LATER"].map(|label| (
                    LinkTarget::External {
                        uri: "https://other.test".into()
                    },
                    Some("before after".into()),
                    label.into()
                ))
            );
            assert_eq!(
                control_warning_sources(&document, &source),
                ["\x1b", definition],
                "a refused use cannot consume the accepted definition's title diagnostic"
            );
        }
    }
}
