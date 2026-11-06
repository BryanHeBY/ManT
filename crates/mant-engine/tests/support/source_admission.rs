//! Independent source, destination and reading facts for the public pipeline.
use mant_ir::{Block, Inline, LinkTarget, ResolvedContent, visit::Visit};

pub struct Case {
    pub id: &'static str,
    pub source: &'static str,
    pub rows: &'static [&'static str],
    pub raw_target: Option<&'static str>,
    pub active_target: Option<&'static str>,
    pub title: Option<&'static str>,
    pub warning: Option<(&'static str, &'static str)>,
    pub code: bool,
}

pub const CASES: &[Case] = &[
    Case {
        id: "literal-angle-text",
        source: "!&lt;https://plain.test&gt;",
        rows: &["!<https://plain.test>"],
        raw_target: None,
        active_target: None,
        title: None,
        warning: None,
        code: false,
    },
    Case {
        id: "valid-authored-percent-uri",
        source: "[VALID](https://a.test/a%20b%1B \"TITLE\")",
        rows: &["VALID"],
        raw_target: Some("https://a.test/a%20b%1B"),
        active_target: Some("https://a.test/a%20b%1B"),
        title: Some("TITLE"),
        warning: None,
        code: false,
    },
    Case {
        id: "entity-invalid-uri",
        source: "[REJECTED](https://a.test/a&#32;b)",
        rows: &["REJECTED"],
        raw_target: Some("https://a.test/a b"),
        active_target: None,
        title: None,
        warning: None,
        code: false,
    },
    Case {
        id: "effective-reference-title-origin",
        source: "[REFERENCE][r]\r\n\r\n[r]: https://ref.test \"before&#12;after\"",
        rows: &["REFERENCE"],
        raw_target: Some("https://ref.test"),
        active_target: Some("https://ref.test"),
        title: Some("before after"),
        warning: Some((
            "markdown.control-characters",
            "[r]: https://ref.test \"before&#12;after\"",
        )),
        code: false,
    },
    Case {
        id: "physical-crlf-code",
        source: "```sh\r\nCODE_A\r\nCODE_B\r\n```",
        rows: &["CODE_A", "CODE_B"],
        raw_target: None,
        active_target: None,
        title: None,
        warning: None,
        code: true,
    },
    Case {
        id: "raw-destination-recovery-refused",
        source: "[BLOCKED](https://a.test/\x1b)",
        rows: &["BLOCKED"],
        raw_target: None,
        active_target: None,
        title: None,
        warning: Some((
            "markdown.recovered-link-target",
            "[BLOCKED](https://a.test/\x1b)",
        )),
        code: false,
    },
    Case {
        id: "raw-label-recovery-keeps-target",
        source: "[A\x1bB](https://a.test)",
        rows: &["A B"],
        raw_target: Some("https://a.test"),
        active_target: Some("https://a.test"),
        title: None,
        warning: Some(("markdown.control-characters", "\x1b")),
        code: false,
    },
    Case {
        id: "recovered-reference-id-cannot-rebind",
        source: "[IDENTIFIER][r\x1bf]\n\n[r f]: https://other.test",
        rows: &["IDENTIFIER"],
        raw_target: None,
        active_target: None,
        title: None,
        warning: Some(("markdown.recovered-link-target", "[IDENTIFIER][r\x1bf]")),
        code: false,
    },
];

#[derive(Default)]
struct Links(Vec<(LinkTarget, Option<String>, String)>);
impl<'a> Visit<'a> for Links {
    fn visit_inline(&mut self, inline: &'a Inline) {
        if let Inline::Link {
            target,
            title,
            children,
        } = inline
        {
            self.0.push((
                target.clone(),
                title.clone(),
                mant_ir::inline_plain_text(children),
            ));
        }
        mant_ir::visit::walk_inline(self, inline);
    }
}

impl Case {
    pub fn load(&self) -> ResolvedContent {
        let source = self.document_source();
        let content = mant_loader::load_markdown_text(&source, None).unwrap();
        self.assert_content(&content, true);
        if let Some((code, original)) = self.warning {
            let findings: Vec<_> = content
                .document
                .as_ref()
                .unwrap()
                .diagnostics
                .iter()
                .filter(|d| d.code.as_deref() == Some(code))
                .collect();
            assert_eq!(findings.len(), 1, "{}: first warning", self.id);
            let span = findings[0].source.unwrap();
            let range = span.byte_range.unwrap();
            let start = range.start.get() as usize;
            assert_eq!(
                &source[start..range.end.get() as usize],
                original,
                "{}: original source location",
                self.id
            );
            assert_eq!(
                span.line as usize,
                source[..start]
                    .bytes()
                    .filter(|&byte| byte == b'\n')
                    .count()
                    + 1
            );
        }
        content
    }

    fn document_source(&self) -> String {
        format!("# Probe\n\n## Payload\n\n{}\n", self.source)
    }

    pub fn assert_content(&self, content: &ResolvedContent, first: bool) {
        let document = content.document.as_ref().unwrap();
        let mut links = Links::default();
        links.visit_document(document);
        let target = if first {
            self.raw_target
        } else {
            self.active_target
        };
        let expected: Vec<_> = target
            .map(|uri| {
                (
                    LinkTarget::External { uri: uri.into() },
                    self.title.map(str::to_owned),
                    self.rows.join("\n"),
                )
            })
            .into_iter()
            .collect();
        assert_eq!(links.0, expected, "{}: target/title/occurrence", self.id);
        for (target, _, _) in &links.0 {
            assert_eq!(
                target.to_uri().as_deref(),
                self.active_target,
                "{}",
                self.id
            );
        }
        let [block] = document.sections[0].blocks.as_slice() else {
            panic!("{}: one source owner", self.id);
        };
        let children = match block {
            Block::Paragraph { children, .. } if !self.code => children,
            Block::Preformatted {
                children, language, ..
            } if self.code => {
                assert_eq!(language.as_deref(), Some("sh"));
                children
            }
            _ => panic!("{}: original content role: {block:?}", self.id),
        };
        assert_eq!(mant_ir::inline_plain_text(children), self.rows.join("\n"));
    }

    pub fn export_witness(&self, original: &ResolvedContent) -> ResolvedContent {
        let mut expected = original.clone();
        if self.raw_target.is_some() && self.active_target.is_none() {
            // This one source has a flat plain label. Invalid URI admission
            // remains in IR0; safe export deliberately projects only that label.
            let Block::Paragraph { children, .. } =
                &mut expected.document.as_mut().unwrap().sections[0].blocks[0]
            else {
                panic!("invalid source label");
            };
            *children = vec![Inline::Text {
                value: self.rows.join("\n"),
            }];
        }
        expected
    }
}
