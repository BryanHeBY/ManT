//! Measure the open final label row, independently of preceding label rows.
use crate::{ContentContext, Inline};

/// Display cells occupied by the final open definition-label row.
///
/// Each nonempty term starts a separate physical label. Earlier rows are
/// already complete and cannot prevent the last row from running into a body.
/// Anchors and empty wrappers do not create rows; a trailing explicit break
/// closes a row and must not be erased by trimming. Width is measured after
/// joining style/link fragments, preserving combining and wide characters.
///
/// # Panics
///
/// Panics if a retained key or range does not resolve in `content`.
#[must_use]
pub fn definition_run_in_width(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
) -> Option<usize> {
    content
        .definition_run_in_width(terms)
        .expect("definition terms resolve in their authoritative content store")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ContentFixture;

    #[test]
    fn preceding_labels_and_hard_rows_do_not_measure_the_final_open_row() {
        let mut fixture = ContentFixture::body();
        let long_1 = fixture.text("long-label");
        let short_1 = fixture.text("-b");
        let joined = fixture.text("long-label\n-b");
        let short_2 = fixture.text("-b");
        let long_2 = fixture.text("long-label");
        let padded = fixture.text("  -b ");
        let store = fixture.finish();
        let content = store.content();
        assert_eq!(
            definition_run_in_width(content, &[vec![long_1], vec![short_1]]),
            Some(2)
        );
        assert_eq!(definition_run_in_width(content, &[vec![joined]]), Some(2));
        assert_eq!(
            definition_run_in_width(content, &[vec![short_2], vec![long_2]]),
            Some(10)
        );
        assert_eq!(definition_run_in_width(content, &[vec![padded]]), Some(5));
    }

    #[test]
    fn anchors_and_wrappers_do_not_reopen_a_closed_label_row() {
        let mut fixture = ContentFixture::body();
        let anchor = fixture.anchor("target");
        let hard_break = fixture.hard_break();
        let newline = fixture.text("\n");
        let labels = [fixture.text("-b"), fixture.text("-b")];
        let standalone_label = fixture.text("-b");
        let before_break = fixture.text("-b");
        let final_break = fixture.hard_break();
        let after_break = fixture.text("-c");
        let store = fixture.finish();
        let content = store.content();
        for (label, boundary) in labels.into_iter().zip([hard_break, newline]) {
            assert_eq!(
                definition_run_in_width(content, &[vec![label, boundary, anchor.clone()]]),
                None
            );
        }
        assert_eq!(
            definition_run_in_width(content, &[vec![standalone_label], vec![anchor]]),
            Some(2)
        );
        assert_eq!(
            definition_run_in_width(
                content,
                &[vec![Inline::Emphasis {
                    children: Vec::new()
                }]]
            ),
            None
        );
        assert_eq!(
            definition_run_in_width(content, &[vec![before_break, final_break, after_break]]),
            Some(2)
        );
    }

    #[test]
    fn styled_link_fragments_share_unicode_cell_measurement() {
        let mut fixture = ContentFixture::body();
        let long = fixture.text("long-label");
        let line_break = fixture.hard_break();
        let japanese = fixture.text("日");
        let link = fixture.link_text(
            crate::LinkTarget::External {
                uri: "https://example.org".into(),
            },
            None,
            "本e",
            false,
        );
        let combining = fixture.text("\u{301}");
        let terms = vec![vec![
            long,
            line_break,
            Inline::Strong {
                children: vec![japanese],
            },
            link,
            Inline::Emphasis {
                children: vec![combining],
            },
        ]];
        let store = fixture.finish();
        assert_eq!(definition_run_in_width(store.content(), &terms), Some(5));
    }
}
