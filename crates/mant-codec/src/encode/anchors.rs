//! Source-map markers are HTML events, never lookalike code or escaped text.
use pulldown_cmark::{Event, Parser};
use std::ops::Range;

pub(crate) struct AnchorMarker<'a> {
    pub(super) id: &'a str,
    pub(crate) range: Range<usize>,
}

/// Index only complete zero-width anchors in the emitted `CommonMark` syntax.
/// These ranges hide zero-width presentation only; semantic owner ranges are
/// composed independently by the renderer. Code fences/spans are not markers.
pub(crate) fn anchor_markers(markdown: &str) -> Vec<AnchorMarker<'_>> {
    let mut markers = Vec::new();
    for (event, range) in
        Parser::new_ext(markdown, pulldown_cmark::Options::ENABLE_MATH).into_offset_iter()
    {
        match event {
            Event::Html(_) | Event::InlineHtml(_) => {
                markers.extend(markers_in_html(markdown, range, "<a id=\"", "\"></a>"));
            }
            _ => {}
        }
    }
    markers
}

/// Find complete zero-width markers whose opening tags are in an accepted
/// HTML event. The close may be a separate inline event.
pub(super) fn markers_in_html<'a>(
    markdown: &'a str,
    range: Range<usize>,
    prefix: &'static str,
    suffix: &'static str,
) -> impl Iterator<Item = AnchorMarker<'a>> {
    let mut cursor = range.start;
    std::iter::from_fn(move || {
        while let Some(relative) = markdown[cursor..range.end].find(prefix) {
            let start = cursor + relative;
            cursor = start + prefix.len();
            let length = markdown[cursor..range.end].find(['<', '>', '\n', '\r', '"'])?;
            let id_end = cursor + length;
            if markdown[id_end..].starts_with(suffix) {
                let id = &markdown[cursor..id_end];
                let end = id_end + suffix.len();
                cursor = end.min(range.end);
                return Some(AnchorMarker {
                    id,
                    range: start..end,
                });
            }
        }
        None
    })
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
