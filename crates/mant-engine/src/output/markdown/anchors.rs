//! Source-map markers are HTML events, never lookalike code or escaped text.
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::ops::Range;

pub(crate) struct AnchorMarker {
    pub(crate) range: Range<usize>,
    pub(crate) item_end: Option<usize>,
}

/// Index only complete zero-width anchors in the emitted `CommonMark` syntax.
/// Tracking enclosing item ranges also avoids reverse-searching display text
/// for semantic ownership. Code fences/spans cannot contribute HTML markers.
pub(crate) fn anchor_markers(markdown: &str) -> Vec<AnchorMarker> {
    let mut items = Vec::new();
    let mut markers = Vec::new();
    for (event, range) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Start(Tag::Item) => items.push(range.end),
            Event::End(TagEnd::Item) => {
                items.pop();
            }
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
                        markers.push(AnchorMarker {
                            range: start..end,
                            item_end: items.last().copied(),
                        });
                    }
                    cursor = end.min(range.end);
                }
            }
            _ => {}
        }
    }
    markers
}
