use super::{
    Font, Inline, TextEventState, TextExecutionPolicy, WordOutput, append_text_event, flush_segment,
};
use crate::mandoc::inline::font::style::styled_segment;
use crate::mandoc::reference::{
    SPHINX_EMPTY_DESTINATION, manual_section_operand, trailing_sphinx_manual_reference,
};

pub(super) fn append_empty_destination(
    word: &mut WordOutput<'_>,
    policy: TextExecutionPolicy,
    text_state: &mut TextEventState,
) {
    let eligible = policy.recognize_generated_references
        && word.link.is_none()
        && !matches!(
            word.font,
            Font::Code | Font::CodeStrong | Font::CodeEmphasis
        )
        && !word.zero_advance.is_armed()
        && !word.zero_advance.has_buffered_glyph();
    // term_word() writes an invisible graph for \%, followed by two normal
    // glyphs. Those glyphs participate in overstrike and word break effects
    // even if their source spelling may later qualify for navigation.
    text_state.note_graph();
    text_state.zero_graph_seen = true;
    append_text_event(
        "<>",
        WordOutput {
            output: &mut *word.output,
            buffer: &mut *word.buffer,
            font: word.font,
            link: word.link,
            zero_advance: &mut *word.zero_advance,
        },
        text_state,
        policy.field_authoritative,
        None,
    );
    let Some(before_marker) = word.buffer.strip_suffix("<>").filter(|_| eligible) else {
        return;
    };
    let span = trailing_sphinx_manual_reference(before_marker)
        .map(|reference| (reference.prefix.len(), reference.display))
        .or_else(|| {
            // man_term.c::pre_alternate executes the section in a separate
            // Roman word after the bold/italic name. Carry only the exact
            // marker's span here. The name must survive alongside it.
            let suffix = before_marker.strip_suffix(' ')?;
            (word.font == Font::Regular && manual_section_operand(suffix).is_some())
                .then_some((0, suffix))
        });
    let Some((prefix_bytes, display)) = span else {
        return;
    };
    let display = display.to_owned();
    word.buffer.truncate(prefix_bytes);
    flush_segment(word.output, word.buffer, word.font, None);
    let target = word
        .zero_advance
        .annotations
        .get_or_insert_with(Default::default)
        .register_source_candidate();
    word.output.push(Inline::Link {
        target,
        children: vec![styled_segment(
            format!("{display}{SPHINX_EMPTY_DESTINATION}"),
            word.font,
        )],
        // Match the exact accepted span before committing. A native field
        // may retain only a prefix, or split an occurrence across owners.
        title: Some(display),
    });
}
