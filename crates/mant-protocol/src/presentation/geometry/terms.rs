//! Measure the open final label row, independently of preceding label rows.
use mant_ir::Inline;

/// Display cells occupied by the final open definition-label row.
///
/// Each nonempty term starts a separate physical label. Earlier rows are
/// already complete and cannot prevent the last row from running into a body.
/// Anchors and empty wrappers do not create rows; a trailing explicit break
/// closes a row and must not be erased by trimming. Width is measured after
/// joining style/link fragments, preserving combining and wide characters.
#[must_use]
pub fn definition_run_in_width(terms: &[Vec<Inline>]) -> Option<usize> {
    let mut final_row = String::new();
    let mut present = false;
    for term in terms {
        let mut row = String::new();
        let mut term_present = false;
        append(term, &mut row, &mut term_present);
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
            Inline::Text { value } | Inline::Code { value } => {
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
            Inline::LineBreak => {
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
        for boundary in [Inline::LineBreak, text("\n")] {
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
            definition_run_in_width(&[vec![text("-b"), Inline::LineBreak, text("-c")]]),
            Some(2)
        );
    }

    #[test]
    fn styled_link_fragments_share_unicode_cell_measurement() {
        let terms = vec![vec![
            text("long-label"),
            Inline::LineBreak,
            Inline::Strong {
                children: vec![text("日")],
            },
            Inline::Link {
                target: mant_ir::LinkTarget::External {
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
