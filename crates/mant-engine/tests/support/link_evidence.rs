//! Independent original-IR scalar, code and link-occurrence witness.
use mant_ir::{Inline, LinkTarget, ResolvedContent, visit::Visit};
use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
struct Cell {
    glyph: char,
    style: u8,
    link: Option<usize>,
}

#[derive(Debug, PartialEq, Eq)]
struct Link {
    target: LinkTarget,
    title: Option<String>,
    scalars: Range<usize>,
}

#[derive(Debug, Default)]
pub struct Meaning {
    cells: Vec<Cell>,
    links: Vec<Link>,
    style: u8,
    active_link: Option<usize>,
}

impl Visit<'_> for Meaning {
    fn visit_inline(&mut self, inline: &Inline) {
        match inline {
            Inline::Strong { children } | Inline::Emphasis { children } => {
                let previous = self.style;
                self.style |= if matches!(inline, Inline::Strong { .. }) {
                    1
                } else {
                    2
                };
                for child in children {
                    self.visit_inline(child);
                }
                self.style = previous;
            }
            Inline::Link {
                target,
                title,
                children,
            } => {
                let previous = self.active_link;
                let index = self.links.len();
                let start = self.cells.len();
                self.links.push(Link {
                    target: target.clone(),
                    title: title.clone(),
                    scalars: start..start,
                });
                self.active_link = Some(index);
                for child in children {
                    self.visit_inline(child);
                }
                self.links[index].scalars.end = self.cells.len();
                self.active_link = previous;
            }
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                let code = u8::from(matches!(
                    inline,
                    Inline::Code { .. } | Inline::Equation { .. }
                )) * 4;
                self.cells.extend(value.chars().map(|glyph| Cell {
                    glyph,
                    style: self.style | code,
                    link: self.active_link,
                }));
            }
            Inline::LineBreak {} => self.cells.push(Cell {
                glyph: '\n',
                style: 0,
                link: self.active_link,
            }),
            Inline::Anchor { .. } => {}
        }
    }
}

impl Meaning {
    pub fn target(&self) -> &LinkTarget {
        assert_eq!(self.links.len(), 1, "exactly one authored link occurrence");
        &self.links[0].target
    }

    pub fn capture(content: &ResolvedContent) -> Self {
        let mut result = Self::default();
        result.visit_document(content.document.as_ref().unwrap());
        result
    }

    pub fn assert_preserved(&self, candidate: &ResolvedContent, context: &str) {
        let actual = Self::capture(candidate);
        assert_eq!(
            actual.links, self.links,
            "{context}: original target/title/occurrence ranges"
        );
        assert_eq!(
            actual.cells.len(),
            self.cells.len(),
            "{context}: scalar count"
        );
        for (index, (expected, actual)) in self.cells.iter().zip(&actual.cells).enumerate() {
            assert_eq!(
                (actual.glyph, actual.link),
                (expected.glyph, expected.link),
                "{context}: scalar {index}"
            );
            assert_eq!(
                actual.style & 4,
                expected.style & 4,
                "{context}: code {index}"
            );
            assert_eq!(
                actual.style & !expected.style,
                0,
                "{context}: added style {index}"
            );
        }
    }
}
