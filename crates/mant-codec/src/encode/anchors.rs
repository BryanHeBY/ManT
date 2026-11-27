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
                markers.extend(markers_in_html(markdown, range.clone()));
                markers.extend(find_markers_in_html(markdown, range, "<a id=\"", "\"></a>"));
            }
            _ => {}
        }
    }
    // Events are in source order; an HTML block can interleave current and
    // legacy empty destinations. Project both as one ordered set of ranges.
    markers.sort_unstable_by_key(|marker| marker.range.start);
    markers
}

/// Find complete zero-width markers whose opening tags are in an accepted
/// HTML event. The close may be a separate inline event.
pub(super) fn markers_in_html(
    markdown: &str,
    range: Range<usize>,
) -> impl Iterator<Item = AnchorMarker<'_>> {
    find_markers_in_html(markdown, range, "<span id=\"", "\"></span>")
}

fn find_markers_in_html<'a>(
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
        for tag in ["span", "a"] {
            let marker = format!("<{tag} id=\"node\"></{tag}>");
            let text = format!("before{marker}after");
            let ranges: Vec<_> = anchor_markers(&text).into_iter().map(|m| m.range).collect();
            assert_eq!(ranges, vec![6..6 + marker.len()]);
            for retained in [
                format!("before<{tag} id=\"node\">payload</{tag}>after"),
                format!("before<{tag} id=\"node\"after"),
                format!("\u{60}{marker}\u{60}"),
                format!("\\{marker}"),
                format!("~~~html\n{marker}\n~~~"),
            ] {
                assert!(anchor_markers(&retained).is_empty(), "{retained}");
            }
        }
    }

    #[test]
    fn scanner_orders_current_and_legacy_markers_in_one_html_block() {
        let text = "<br />\n<a id=\"first\"></a><span id=\"second\"></span>\n";
        assert_eq!(
            anchor_markers(text)
                .into_iter()
                .map(|marker| marker.id)
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }
}
