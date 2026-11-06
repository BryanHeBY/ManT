//! Entity CR stays ordinary table Text; a portable fence uses safe read cells.

use mant_ir::{Block, ResolvedContent};

pub const TARGET: &str = "https://example.test/table";

pub struct Case {
    pub id: &'static str,
    pub source: &'static str,
    pub original: &'static str,
    pub reading: &'static str,
    pub rows: &'static [&'static str],
}

pub const CASES: &[Case] = &[
    Case {
        id: "text-cr",
        source: "ALPHA&#13;OMEGA",
        original: "ALPHA\rOMEGA",
        reading: "ALPHA�OMEGA",
        rows: &["ALPHA�OMEGA"],
    },
    Case {
        id: "text-lf",
        source: "ALPHA&#10;OMEGA",
        original: "ALPHA\nOMEGA",
        reading: "ALPHA\nOMEGA",
        rows: &["ALPHA", "OMEGA"],
    },
    Case {
        id: "text-crlf",
        source: "ALPHA&#13;&#10;OMEGA",
        original: "ALPHA\r\nOMEGA",
        reading: "ALPHA�\nOMEGA",
        rows: &["ALPHA�", "OMEGA"],
    },
    Case {
        id: "text-cr-unicode",
        source: "界😀e\u{301}&#13;END",
        original: "界😀e\u{301}\rEND",
        reading: "界😀e\u{301}�END",
        rows: &["界😀e\u{301}�END"],
    },
    Case {
        id: "literal-entity-spelling",
        source: "ALPHA&amp;#13;OMEGA",
        original: "ALPHA&#13;OMEGA",
        reading: "ALPHA&#13;OMEGA",
        rows: &["ALPHA&#13;OMEGA"],
    },
];

impl Case {
    pub fn load(&self, linked: bool) -> ResolvedContent {
        let label = if linked {
            format!("[{}]({TARGET})", self.source)
        } else {
            self.source.into()
        };
        let content =
            mant_loader::load_markdown_text(&format!("# Table\n\n| {label} |\n| --- |\n"), None)
                .unwrap();
        let document = content.document.as_ref().unwrap();
        assert!(document.diagnostics.is_empty(), "{}", self.id);
        let [Block::Table { rows, .. }] = document.blocks.as_slice() else {
            panic!("{}: one source table", self.id);
        };
        let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
            panic!("{}: ordinary source Text owner", self.id);
        };
        assert_eq!(mant_ir::inline_plain_text(children), self.original);
        content
    }

    pub fn assert_portable(&self, content: &ResolvedContent) {
        let [Block::Preformatted { children, .. }] =
            content.document.as_ref().unwrap().blocks.as_slice()
        else {
            panic!("{}: selected portable table fence", self.id);
        };
        assert_eq!(mant_ir::inline_plain_text(children), self.reading);
    }
}
