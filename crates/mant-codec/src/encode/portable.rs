//! Portable glyph replacements preserve the executed structural boundaries.

use mant_ir::Inline;

/// Replace accepted native glyphs once, at the first glyph's position. Hard
/// rows and anchors retain their original order. An interval without glyphs
/// cannot create portable prose that the formatter did not accept.
pub(super) fn project(display: &str, children: &[Inline]) -> Vec<Inline> {
    let mut projection = Projection {
        display,
        inserted: false,
        row_prefix: true,
        output: Vec::new(),
    };
    projection.append(children);
    projection.output
}

struct Projection<'a> {
    display: &'a str,
    inserted: bool,
    row_prefix: bool,
    output: Vec<Inline>,
}

impl Projection<'_> {
    fn append(&mut self, children: &[Inline]) {
        for child in children {
            match child {
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    self.text(value);
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. }
                | Inline::PortableDisplay { children, .. } => self.append(children),
                Inline::LineBreak { .. } => {
                    self.output.push(child.clone());
                    self.row_prefix = true;
                }
                Inline::Anchor { .. } => self.output.push(child.clone()),
            }
        }
    }

    fn text(&mut self, value: &str) {
        for (index, row) in value.split('\n').enumerate() {
            if index > 0 {
                self.output.push(Inline::LineBreak { indent_columns: 0 });
                self.row_prefix = true;
            }
            let prefix_end = if self.row_prefix {
                row.char_indices()
                    .find(|(_, character)| !character.is_whitespace())
                    .map_or(row.len(), |(offset, _)| offset)
            } else {
                0
            };
            if prefix_end > 0 {
                self.output.push(Inline::Text {
                    value: row[..prefix_end].to_owned(),
                });
            }
            if row[prefix_end..]
                .chars()
                .any(|character| !character.is_whitespace())
            {
                self.row_prefix = false;
                if !self.inserted {
                    self.inserted = true;
                    if !self.display.is_empty() {
                        self.output.push(Inline::Strong {
                            children: vec![Inline::Text {
                                value: self.display.to_owned(),
                            }],
                        });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_glyphs_keep_distinct_hard_rows_and_resolved_origins() {
        // The real Lk `label\\p` suffix was checked with pristine CVS:
        // termp_lk_pre writes ':' before the address word consumes \\p.
        // These IR rows are already executed facts, never repeated requests.
        let source = vec![
            Inline::Text { value: ":".into() },
            Inline::LineBreak { indent_columns: 6 },
            Inline::LineBreak { indent_columns: 2 },
            Inline::Text {
                value: "https://example.com/x".into(),
            },
        ];
        assert_eq!(project("", &source), source[1..3]);
    }

    #[test]
    fn replacement_keeps_prefix_spacing_and_anchor_order() {
        let anchor = Inline::anchor("source-position");
        let source = vec![
            Inline::Text { value: " ".into() },
            anchor.clone(),
            Inline::Strong {
                children: vec![Inline::Text {
                    value: "-alphaBSD".into(),
                }],
            },
            Inline::LineBreak { indent_columns: 0 },
        ];
        assert_eq!(
            project("BSD (currently in alpha test)", &source),
            vec![
                Inline::Text { value: " ".into() },
                anchor,
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "BSD (currently in alpha test)".into()
                    }]
                },
                Inline::LineBreak { indent_columns: 0 },
            ]
        );
    }

    #[test]
    fn replacement_does_not_restore_a_rejected_native_interval() {
        assert_eq!(
            project(
                "BSD (currently in alpha test)",
                &[Inline::LineBreak { indent_columns: 0 }]
            ),
            [Inline::LineBreak { indent_columns: 0 }]
        );
    }
}
