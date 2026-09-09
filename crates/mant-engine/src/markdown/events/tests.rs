use super::*;
use pulldown_cmark::TagEnd;

#[test]
fn nesting_rejection_does_not_consume_events_or_an_extra_level() {
    let mut cursor = EventCursor::new(vec![(Event::Text("é".into()), 11..13)]);
    for _ in 0..MAX_NESTING_DEPTH {
        assert!(cursor.try_descend());
    }
    assert!(!cursor.try_descend());
    assert_eq!(cursor.peek().unwrap().1, 11..13);
    cursor.ascend();
    assert!(cursor.try_descend());
    for _ in 0..=MAX_NESTING_DEPTH {
        cursor.ascend();
    }
    assert_eq!(cursor.depth, 0);
    assert_eq!(cursor.next().unwrap().1, 11..13);
    assert!(cursor.next().is_none());
}

#[test]
fn balanced_skip_retains_original_offsets_and_stops_before_the_next_item() {
    let mut cursor = EventCursor::new(vec![
        (Event::Start(Tag::Strong), 4..10),
        (Event::Text("é".into()), 6..8),
        (Event::End(TagEnd::Strong), 4..10),
        (Event::End(TagEnd::Item), 2..12),
        (Event::Start(Tag::Item), 14..22),
    ]);
    assert_eq!(cursor.consume_balanced(2..12), 2..12);
    assert!(matches!(cursor.peek(), Some((Event::Start(Tag::Item), range)) if *range == (14..22)));
}

#[test]
fn nested_loose_paragraphs_do_not_change_the_parent_item_tightness() {
    let mut cursor = EventCursor::new(vec![
        (Event::Start(Tag::List(None)), 2..20),
        (Event::Start(Tag::Item), 4..20),
        (Event::Start(Tag::Paragraph), 6..10),
        (Event::End(TagEnd::Paragraph), 6..10),
        (Event::End(TagEnd::Item), 4..20),
        (Event::End(TagEnd::List(false)), 2..20),
        (Event::End(TagEnd::Item), 0..20),
    ]);
    assert!(!cursor.item_has_direct_paragraph());
    cursor.position = 2;
    assert!(cursor.item_has_direct_paragraph());
}
