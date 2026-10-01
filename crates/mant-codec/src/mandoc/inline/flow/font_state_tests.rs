use super::{Font, FontState};

#[test]
fn crossed_body_pop_uses_live_lower_slot_and_keeps_previous_register() {
    // The exact Ao/.ft B/Bf/.Ac source was checked with pinned CVS
    // -Tascii. term.c::term_fontrepl() changes the active fontq slot;
    // term_fontpopq() only changes the index at the original BODY close.
    let mut font = FontState::new();
    let outer = font.checkpoint();
    font.select(Font::Strong);
    let inner = font.push_scope(Font::Emphasis);
    assert_eq!(font.current, Font::Emphasis);
    font.pop_scope(outer);
    assert_eq!(font.current, Font::Strong);
    assert_eq!(font.previous, Font::Strong);
    font.pop_scope(inner);
    assert_eq!(font.current, Font::Strong);
}
