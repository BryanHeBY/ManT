//! Source-map markers are HTML events, never lookalike code or escaped text.
use pulldown_cmark::{Event, Parser};
use std::ops::Range;

pub(crate) struct AnchorMarker {
    pub(crate) range: Range<usize>,
}

/// Index only complete zero-width anchors in the emitted `CommonMark` syntax.
/// These ranges hide zero-width presentation only; semantic owner ranges are
/// composed independently by the renderer. Code fences/spans are not markers.
pub(crate) fn anchor_markers(markdown: &str) -> Vec<AnchorMarker> {
    let mut markers = Vec::new();
    for (event, range) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Html(_) | Event::InlineHtml(_) => {
                let mut cursor = range.start;
                while let Some(relative) = markdown[cursor..range.end].find("<a id=\"") {
                    let start = cursor + relative;
                    let Some(end) = markdown[start..].find("\"></a>") else {
                        break;
                    };
                    let end = start + end + "\"></a>".len();
                    // The opening tag is itself an HTML event. Require its
                    // immediate empty closing tag, not arbitrary later prose.
                    if !markdown[start + 7..end - 6].contains(['<', '>', '\n', '"']) {
                        markers.push(AnchorMarker { range: start..end });
                    }
                    cursor = end.min(range.end);
                }
            }
            _ => {}
        }
    }
    markers
}

#[cfg(test)]
mod tests {
    use super::anchor_markers;

    #[test]
    fn scanner_indexes_only_complete_empty_inline_anchors() {
        let text = "before<a id=\"node\"></a>after";
        let ranges: Vec<_> = anchor_markers(text).into_iter().map(|m| m.range).collect();
        assert_eq!(ranges, vec![6..23]);
        assert_eq!(&text[ranges[0].clone()], "<a id=\"node\"></a>");
        for retained in [
            "before<a id=\"node\">payload</a>after",
            "before<a id=\"node\"after",
        ] {
            assert!(anchor_markers(retained).is_empty(), "{retained}");
        }
    }
}
