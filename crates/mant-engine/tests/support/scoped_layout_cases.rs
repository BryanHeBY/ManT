//! Source-bound, pristine-CVS rows shared by public consumer tests.

use mant_ir::{Block, ResolvedContent, Section, visit::Visit};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    case_count: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
pub struct Case {
    pub id: String,
    pub source: String,
    pub utf8_rows: Vec<String>,
}

pub fn cases() -> Vec<Case> {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../mant-codec/src/mandoc/tests/control_boundaries/scoped_layout/cases.json"
    ))
    .unwrap();
    assert_eq!(fixture.case_count, fixture.cases.len());
    fixture.cases
}

pub fn description(content: &ResolvedContent) -> &Section {
    content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap()
}

pub fn cli_body(content: &ResolvedContent) -> String {
    let rendered = mant_render::render_query_man(content);
    rendered.split_once("DESCRIPTION\n").unwrap().1.to_owned()
}

#[derive(Default)]
struct Owners(Vec<(bool, String)>);

impl Visit<'_> for Owners {
    fn visit_block(&mut self, block: &Block) {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                let text = mant_ir::inline_plain_text(children);
                if !text.is_empty() {
                    self.0
                        .push((matches!(block, Block::Preformatted { .. }), text));
                }
            }
            _ => mant_ir::visit::walk_block(self, block),
        }
    }
}

impl Case {
    pub fn load(&self) -> ResolvedContent {
        mant_loader::load_roff_bytes(self.source.as_bytes()).unwrap()
    }

    pub fn labels(&self) -> &[&str] {
        match self.id.as_str() {
            "tag-two-items" | "nested-tag-two-items" | "nested-tag-two-paragraphs" => {
                &["First body.", "Second body."]
            }
            "nested-list-offset-origins" => &["First body.", "Second body.", "Later body."],
            "tag-display-sibling" | "plain-display" | "negative-display-reading-origin" => {
                &["CODE"]
            }
            "column-header" | "nested-column-header" | "nested-column-plain-header" => {
                &["Backend", "Path", "Description"]
            }
            "nested-enum-paragraph" | "nested-item-paragraph" | "enum-paragraph" => {
                &["LAST", "AFTER"]
            }
            "ragged-display-paragraph" => &[".Ft", ".Fc", "AFTER"],
            "column-return-literal-origin" => &["Key", "Function", "CODE"],
            "temporary-origin-with-author-blank" => &["ipv4", "AFTER"],
            _ => panic!("unclassified source-bound consumer case: {}", self.id),
        }
    }

    pub fn assert_portable(&self, content: &ResolvedContent) {
        if self.id == "temporary-origin-with-author-blank" {
            return Self::assert_author_blank(content);
        }
        let paragraph = |value: &str| (false, value.to_owned());
        let literal = |value: &str| (true, value.to_owned());
        let mut expected = match self.id.as_str() {
            "tag-two-items" | "nested-tag-two-items" => {
                vec![paragraph("# First body."), paragraph("- Second body.")]
            }
            "nested-tag-two-paragraphs" => vec![
                paragraph("# First body."),
                paragraph("Continued body."),
                paragraph("- Second body."),
            ],
            "nested-list-offset-origins" => vec![
                paragraph("FIRST\nFirst body."),
                paragraph("SECOND\nSecond body."),
                paragraph("LATER\nLater body."),
            ],
            "tag-display-sibling" | "negative-display-reading-origin" => vec![
                paragraph("FIRST"),
                literal("CODE"),
                paragraph("SECOND\nSecond body."),
            ],
            "plain-display" => vec![literal("CODE")],
            "column-header" | "nested-column-header" | "nested-column-plain-header" => {
                vec![literal(
                    "Backend | Path | Description\nuscsi | /dev/node | Body",
                )]
            }
            "nested-enum-paragraph" | "nested-item-paragraph" | "enum-paragraph" => {
                vec![paragraph("LAST"), paragraph("AFTER")]
            }
            "ragged-display-paragraph" => {
                vec![paragraph("HEAD"), paragraph(".Ft\n.Fc"), paragraph("AFTER")]
            }
            "column-return-literal-origin" => vec![
                paragraph("ENTRY"),
                literal("Key | Function\nq | Exit"),
                paragraph("-h and -k are intended to ease use of the mode; for example:"),
                literal("CODE"),
            ],
            _ => panic!("unclassified portable policy: {}", self.id),
        };
        if self.id.starts_with("nested-") {
            expected.insert(0, paragraph("OUTER"));
        }
        let mut actual = Owners::default();
        for block in &description(content).blocks {
            actual.visit_block(block);
        }
        assert_eq!(
            actual.0, expected,
            "{}: exact portable owner/hard rows",
            self.id
        );
        if self.id.ends_with("-paragraph") {
            let rendered = cli_body(content);
            let rows = rendered.lines().collect::<Vec<_>>();
            let marker = if self.id == "ragged-display-paragraph" {
                ".Fc"
            } else {
                "LAST"
            };
            let before = rows.iter().position(|row| row.ends_with(marker)).unwrap();
            let after = rows.iter().position(|row| row.ends_with("AFTER")).unwrap();
            assert_eq!(after, before + 2, "{}: exactly one Pp empty row", self.id);
            assert_eq!(rows[before + 1], "", "{}: actual empty row", self.id);
        }
    }

    fn assert_author_blank(content: &ResolvedContent) {
        let mut actual = Owners::default();
        for block in &description(content).blocks {
            actual.visit_block(block);
        }
        assert!(actual.0.iter().all(|(literal, _)| !literal));
        let lines = actual
            .0
            .iter()
            .flat_map(|(_, text)| text.split('\n'))
            .collect::<Vec<_>>();
        // Native BODY8 + temporary4 is layout, while the last one of its
        // thirteen columns is authored Text. Portable reading keeps that SP
        // and the actual hard boundaries without inventing a code block.
        assert_eq!(
            lines,
            [
                "-r keyword",
                "The default keyword for this option is:",
                " ipv4",
                "AFTER"
            ]
        );
    }
}
