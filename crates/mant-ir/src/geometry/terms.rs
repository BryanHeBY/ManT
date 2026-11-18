//! Measure the open final label row, independently of preceding label rows.
use crate::{DefinitionBodyAlignment, DefinitionItem, HeadBodyRelation, Inline};
use std::borrow::Cow;

/// Term that ends at the alignment field of a separated shared row with a
/// positive minimum gap. A zero gap can rely on its existing separator.
/// The preceding terms and rows remain independent reading content.
#[must_use]
pub fn aligned_definition_term(item: &DefinitionItem) -> Option<usize> {
    if item.head_body_relation != HeadBodyRelation::separated()
        || item.layout.body_alignment != DefinitionBodyAlignment::Indented
        || item.layout.min_term_gap_columns == 0
        || item.shared_description().is_none()
    {
        return None;
    }
    item.terms
        .iter()
        .rposition(|term| crate::last_visible_character(term).is_some())
}

/// Number of final ASCII spaces that an aligned field's gap can replace.
/// Fixed blanks and complete or whitespace-only rows are not padding.
#[must_use]
pub fn definition_term_end_padding(nodes: &[Inline]) -> usize {
    fn end(nodes: &[Inline], spaces: &mut usize) -> Option<char> {
        nodes.iter().rev().find_map(|node| match node {
            Inline::Text { value } | Inline::Code { value } => {
                let prefix = value.trim_end_matches(' ');
                *spaces += value.len() - prefix.len();
                prefix.chars().next_back()
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => end(children, spaces),
            Inline::Equation { value, .. } => value.chars().next_back(),
            Inline::LineBreak { .. } => Some('\n'),
            Inline::Anchor { .. } => None,
        })
    }

    let mut spaces = 0;
    if end(nodes, &mut spaces).is_some_and(|ch| ch != '\n') {
        spaces
    } else {
        0
    }
}

/// Project an aligned term without its replaceable field padding.
/// The original term still owns its spelling and zero-width destinations.
pub fn omit_term_end_padding(nodes: &mut Cow<'_, [Inline]>) {
    fn omit(nodes: &mut [Inline], spaces: &mut usize) {
        for node in nodes.iter_mut().rev() {
            if *spaces == 0 {
                break;
            }
            match node {
                Inline::Text { value } | Inline::Code { value } => {
                    let removed = (value.len() - value.trim_end_matches(' ').len()).min(*spaces);
                    value.truncate(value.len() - removed);
                    *spaces -= removed;
                    if removed > 0 && value.is_empty() {
                        // A stripped code fragment has no Markdown delimiters.
                        *node = Inline::Text {
                            value: String::new(),
                        };
                    }
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => omit(children, spaces),
                _ => {}
            }
        }
    }

    let mut spaces = definition_term_end_padding(nodes);
    if spaces > 0 {
        omit(nodes.to_mut(), &mut spaces);
    }
}

/// Display cells occupied by the final open definition-label row.
///
/// Each nonempty term starts a separate physical label. Earlier rows are
/// already complete and cannot prevent the last row from running into a body.
/// Anchors and empty wrappers do not create rows; a trailing explicit break
/// closes a row and must not be erased by trimming. Width is measured after
/// joining style/link fragments, preserving combining and wide characters.
#[must_use]
pub fn definition_run_in_width(terms: &[impl AsRef<[Inline]>]) -> Option<usize> {
    let mut final_row = String::new();
    let mut present = false;
    for term in terms {
        let mut row = String::new();
        let mut term_present = false;
        append(term.as_ref(), &mut row, &mut term_present);
        if term_present {
            final_row = row;
            present = true;
        }
    }
    (present && !final_row.is_empty()).then(|| super::text_width(&final_row))
}

fn append(nodes: &[Inline], row: &mut String, present: &mut bool) {
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                *present |= !value.is_empty();
                if let Some((_, tail)) = value.rsplit_once('\n') {
                    row.clear();
                    row.push_str(tail);
                } else {
                    row.push_str(value);
                }
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => append(children, row, present),
            Inline::LineBreak { .. } => {
                row.clear();
                *present = true;
            }
            Inline::Anchor { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Inline {
        Inline::Text {
            value: value.into(),
        }
    }

    #[test]
    fn aligned_field_projection_keeps_other_rows_fixed_blanks_and_targets() {
        let anchor = Inline::anchor("end");
        let source = [
            text("  A  B \nC\u{a0}"),
            Inline::Strong {
                children: vec![Inline::Code {
                    value: "   ".into(),
                }],
            },
            anchor.clone(),
        ];
        let mut reading = Cow::Borrowed(source.as_slice());
        omit_term_end_padding(&mut reading);
        assert_eq!(crate::inline_plain_text(&reading), "  A  B \nC\u{a0}");
        assert_eq!(reading.last(), Some(&anchor));
        assert_eq!(crate::inline_plain_text(&source), "  A  B \nC\u{a0}   ");
        for value in ["   ", "A\n   ", "A \u{a0}", "A  \n"] {
            let source = [text(value)];
            let mut reading = Cow::Borrowed(source.as_slice());
            omit_term_end_padding(&mut reading);
            assert!(matches!(reading, Cow::Borrowed(_)), "{value:?}");
        }
    }

    #[test]
    fn only_an_aligned_shared_field_with_its_own_gap_replaces_padding() {
        let mut item = DefinitionItem {
            source: None,
            entry: None,
            terms: vec![vec![text("first ")].into(), vec![text("last ")].into()],
            description: vec![crate::Block::Paragraph {
                children: vec![text("BODY")],
                inline_layout: crate::InlineLayout::default(),
                layout: crate::LayoutHint::default(),
                source: None,
            }],
            head_body_relation: HeadBodyRelation::separated(),
            layout: crate::DefinitionLayout::default(),
        };
        for (relation, alignment, gap, expected) in [
            (
                HeadBodyRelation::separated(),
                DefinitionBodyAlignment::Indented,
                1,
                Some(1),
            ),
            (
                HeadBodyRelation::separated(),
                DefinitionBodyAlignment::Indented,
                0,
                None,
            ),
            (
                HeadBodyRelation::separated(),
                DefinitionBodyAlignment::AfterTerm,
                1,
                None,
            ),
            (
                HeadBodyRelation::joined(),
                DefinitionBodyAlignment::Indented,
                1,
                None,
            ),
            (
                HeadBodyRelation::Separate,
                DefinitionBodyAlignment::Indented,
                1,
                None,
            ),
        ] {
            item.head_body_relation = relation;
            item.layout.body_alignment = alignment;
            item.layout.min_term_gap_columns = gap;
            assert_eq!(aligned_definition_term(&item), expected);
        }
    }

    #[test]
    fn preceding_labels_and_hard_rows_do_not_measure_the_final_open_row() {
        assert_eq!(
            definition_run_in_width(&[vec![text("long-label")], vec![text("-b")]]),
            Some(2)
        );
        assert_eq!(
            definition_run_in_width(&[vec![text("long-label\n-b")]]),
            Some(2)
        );
        assert_eq!(
            definition_run_in_width(&[vec![text("-b")], vec![text("long-label")]]),
            Some(10)
        );
        assert_eq!(definition_run_in_width(&[vec![text("  -b ")]]), Some(5));
    }

    #[test]
    fn anchors_and_wrappers_do_not_reopen_a_closed_label_row() {
        let anchor = Inline::anchor("target");
        for boundary in [Inline::line_break(), text("\n")] {
            assert_eq!(
                definition_run_in_width(&[vec![text("-b"), boundary, anchor.clone()]]),
                None
            );
        }
        assert_eq!(
            definition_run_in_width(&[vec![text("-b")], vec![anchor]]),
            Some(2)
        );
        assert_eq!(
            definition_run_in_width(&[vec![Inline::Emphasis {
                children: Vec::new()
            }]]),
            None
        );
        assert_eq!(
            definition_run_in_width(&[vec![text("-b"), Inline::line_break(), text("-c")]]),
            Some(2)
        );
    }

    #[test]
    fn styled_link_fragments_share_unicode_cell_measurement() {
        let terms = vec![vec![
            text("long-label"),
            Inline::line_break(),
            Inline::Strong {
                children: vec![text("日")],
            },
            Inline::Link {
                target: crate::LinkTarget::External {
                    uri: "https://example.org".into(),
                },
                title: None,
                children: vec![text("本e")],
            },
            Inline::Emphasis {
                children: vec![text("\u{301}")],
            },
        ]];
        assert_eq!(definition_run_in_width(&terms), Some(5));
    }
}
