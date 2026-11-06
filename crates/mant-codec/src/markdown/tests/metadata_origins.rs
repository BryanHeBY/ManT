//! Metadata diagnostics follow effective definitions, not every use site.
use super::parse_markdown;
use mant_ir::{
    Document, Inline,
    visit::{Visit, walk_inline},
};

#[derive(Default)]
struct Titles(Vec<String>);
impl<'a> Visit<'a> for Titles {
    fn visit_inline(&mut self, inline: &'a Inline) {
        if let Inline::Link {
            title: Some(title), ..
        } = inline
        {
            self.0.push(title.clone());
        }
        walk_inline(self, inline);
    }
}
fn titles(document: &Document) -> Vec<String> {
    let mut visitor = Titles::default();
    visitor.visit_document(document);
    visitor.0
}

#[test]
fn effective_reference_title_has_one_definition_diagnostic_across_source_forms() {
    let mut cases = 0;
    // RefDefs::get supplies parser whitespace/case folding and first-definition
    // wins; LinkDef.span covers the accepted definition, including its title.
    for ending in ["\n", "\r\n", "\r"] {
        for link in ["[one][ref]", "[ref][]", "[ref]", "[one][ReF]"] {
            for prefix in ["", "<br />\n", "- <br />\n  "] {
                for before in [false, true] {
                    let definition = "[ref]: https://example.test \"before&#12;after\"";
                    let uses = format!("{prefix}{link} {link}\n");
                    let source = if before {
                        format!("{definition}\n\n{uses}")
                    } else {
                        format!("{uses}\n{definition}\n")
                    }
                    .replace('\n', ending);
                    let document = parse_markdown(&source, None).unwrap().document;
                    assert_eq!(
                        titles(&document),
                        ["before after", "before after"],
                        "{source:?}"
                    );
                    let findings = document
                        .diagnostics
                        .iter()
                        .filter(|d| d.code.as_deref() == Some("markdown.control-characters"))
                        .collect::<Vec<_>>();
                    assert_eq!(findings.len(), 1, "{source:?}");
                    let range = findings[0].source.unwrap().byte_range.unwrap();
                    assert_eq!(
                        &source[range.start.get() as usize..range.end.get() as usize],
                        definition,
                        "{source:?}"
                    );
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 72);
}

#[test]
fn title_dedup_does_not_merge_labels_inline_titles_or_distinct_definitions() {
    let source = "[a&#12;][r] [b&#12;][r] [c][s] [d](https://d.test \"t&#12;\") [e](https://e.test \"t&#12;\")\n\n[r]: https://r.test \"t&#12;\"\n[s]: https://s.test \"t&#12;\"\n[unused]: https://u.test \"t&#12;\"\n[r]: https://shadow.test \"bad&#12;\"\n";
    let document = parse_markdown(source, None).unwrap().document;
    assert_eq!(titles(&document), ["t ", "t ", "t ", "t ", "t "]);
    let findings = document
        .diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("markdown.control-characters"))
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 6);
    assert!(findings.iter().all(|d| {
        let r = d.source.unwrap().byte_range.unwrap();
        let raw = &source[r.start.get() as usize..r.end.get() as usize];
        !raw.contains("unused") && !raw.contains("shadow")
    }));
}

#[test]
fn repeated_reference_expansion_does_not_repeat_metadata_diagnostics() {
    for count in [1, 100, 1000] {
        let source = format!(
            "{}\n\n[r]: https://r.test \"a&#12;b\"",
            "[x][r] ".repeat(count)
        );
        let document = parse_markdown(&source, None).unwrap().document;
        assert_eq!(titles(&document).len(), count);
        assert_eq!(document.diagnostics.len(), 1);
        let json = serde_json::to_string(&document).unwrap();
        assert_eq!(serde_json::from_str::<Document>(&json).unwrap(), document);
    }
}

#[test]
fn multiline_effective_definition_and_unknown_origin_do_not_borrow_use_locations() {
    for ending in ["\n", "\r\n", "\r"] {
        let definition =
            "[Réf]: https://example.test\n  \"before&#12;after\"".replace('\n', ending);
        let source = format!(
            "中{ending}{ending}<br />{ending}[label][RÉF]{ending}{ending}{definition}{ending}"
        );
        let document = parse_markdown(&source, None).unwrap().document;
        assert_eq!(titles(&document), ["before after"]);
        let span = document.diagnostics[0].source.unwrap();
        let range = span.byte_range.unwrap();
        assert_eq!(
            &source[range.start.get() as usize..range.end.get() as usize],
            definition
        );
    }
    let source = super::super::source::MarkdownSource::new("[x][missing]");
    let mut cursor = super::super::EventCursor::new(Vec::new());
    let mut diagnostics = Vec::new();
    assert_eq!(
        super::super::decoded::title(
            "a\x0cb".into(),
            &(0..12),
            pulldown_cmark::LinkType::ReferenceUnknown,
            &mut cursor,
            &source,
            &mut diagnostics
        ),
        "a b"
    );
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].source, None);
}
