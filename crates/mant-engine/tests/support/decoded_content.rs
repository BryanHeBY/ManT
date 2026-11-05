//! Independent admitted scalars and terminal projections for decoded entities.
use mant_ir::{Inline, ResolvedContent, visit::Visit};

pub struct Case {
    pub id: &'static str,
    pub source: &'static str,
    pub label: &'static str,
    pub painted_rows: &'static [&'static str],
    pub title: Option<&'static str>,
    pub warning: bool,
}

pub const CASES: &[Case] = &[
    Case {
        id: "cr-label",
        source: "[A&#13;B](https://a.test)",
        label: "A\rB",
        painted_rows: &["A�B"],
        title: None,
        warning: false,
    },
    Case {
        id: "crlf-label",
        source: "[A&#x0d;&#x0a;B](https://a.test)",
        label: "A\r\nB",
        painted_rows: &["A�", "B"],
        title: None,
        warning: false,
    },
    Case {
        id: "ff-label",
        source: "[A&#12;B](https://a.test)",
        label: "A B",
        painted_rows: &["A B"],
        title: None,
        warning: true,
    },
    Case {
        id: "esc-label",
        source: "[A&#x1b;B](https://a.test)",
        label: "A B",
        painted_rows: &["A B"],
        title: None,
        warning: true,
    },
    Case {
        id: "del-label",
        source: "[A&#127;B](https://a.test)",
        label: "A B",
        painted_rows: &["A B"],
        title: None,
        warning: true,
    },
    Case {
        id: "ff-title",
        source: "[ABC](https://a.test \"before&#12;after\")",
        label: "ABC",
        painted_rows: &["ABC"],
        title: Some("before after"),
        warning: true,
    },
    Case {
        id: "esc-title",
        source: "[ABC](https://a.test \"before&#27;after\")",
        label: "ABC",
        painted_rows: &["ABC"],
        title: Some("before after"),
        warning: true,
    },
    Case {
        id: "cr-title",
        source: "[ABC](https://a.test \"before&#13;after\")",
        label: "ABC",
        painted_rows: &["ABC"],
        title: Some("before\rafter"),
        warning: false,
    },
    Case {
        id: "double-entity",
        source: "[A&amp;#13;B](https://a.test)",
        label: "A&#13;B",
        painted_rows: &["A&#13;B"],
        title: None,
        warning: false,
    },
    Case {
        id: "numeric-c1-control",
        source: "[A&#133;B](https://a.test)",
        // The locked parser decodes U+0085 directly (no Windows-1252 remap).
        // Shared admission masks its two UTF-8 bytes on the first read.
        label: "A  B",
        painted_rows: &["A  B"],
        title: None,
        warning: true,
    },
    Case {
        id: "printable-ellipsis",
        source: "[A&#8230;B](https://a.test)",
        label: "A…B",
        painted_rows: &["A…B"],
        title: None,
        warning: false,
    },
];

impl Case {
    pub fn assert_content(&self, content: &ResolvedContent) {
        struct Links<'a>(Vec<(&'a mant_ir::LinkTarget, Option<&'a str>, String)>);
        impl<'a> Visit<'a> for Links<'a> {
            fn visit_inline(&mut self, inline: &'a Inline) {
                match inline {
                    Inline::Link {
                        target,
                        title,
                        children,
                    } => {
                        self.0.push((
                            target,
                            title.as_deref(),
                            mant_ir::inline_plain_text(children),
                        ));
                    }
                    _ => mant_ir::visit::walk_inline(self, inline),
                }
            }
        }
        let document = content.document.as_ref().unwrap();
        assert_eq!(
            mant_ir::validate_document(document),
            [],
            "{}: valid admitted IR",
            self.id
        );
        let mut links = Links(Vec::new());
        links.visit_document(document);
        assert_eq!(links.0.len(), 1, "{}: original occurrence", self.id);
        let (target, title, label) = &links.0[0];
        assert_eq!(
            target.to_uri().as_deref(),
            Some("https://a.test"),
            "{}",
            self.id
        );
        assert_eq!(*title, self.title, "{}: independent metadata", self.id);
        assert_eq!(
            label, self.label,
            "{}: independent decoded scalars",
            self.id
        );
        let visual: Vec<_> = label
            .split('\n')
            .map(|row| mant_render::sanitize_terminal_text(row).into_owned())
            .collect();
        assert_eq!(
            visual, self.painted_rows,
            "{}: terminal-safe projection",
            self.id
        );
    }

    pub fn assert_first_diagnostic(&self, content: &ResolvedContent) {
        let controls: Vec<_> = content
            .document
            .as_ref()
            .unwrap()
            .diagnostics
            .iter()
            .filter(|finding| finding.code.as_deref() == Some("markdown.control-characters"))
            .collect();
        assert_eq!(
            !controls.is_empty(),
            self.warning,
            "{}: first admission diagnostic",
            self.id
        );
        for diagnostic in controls {
            assert!(
                diagnostic.source.is_some(),
                "{}: original source event range",
                self.id
            );
            assert_eq!(diagnostic.impact, mant_ir::DiagnosticImpact::None);
        }
    }
}
