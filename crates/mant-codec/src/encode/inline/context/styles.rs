//! Choose delimiters over the complete phrasing sequence, including both seams.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::InlinePiece;

mod projection;

// A piece has three choices: its body alone, asterisk, or underscore. A
// transition knows the preceding and following choices, so delimiter runs
// cannot silently merge after a locally valid choice was emitted.
const CHOICES: usize = 3;
const STATES: usize = CHOICES * CHOICES;

#[derive(Clone, Copy, Default, Eq, Ord, PartialEq, PartialOrd)]
struct Score {
    styles: usize,
    preferred: usize,
}

pub(super) fn render_inline_pieces(pieces: &mut [InlinePiece<'_>]) -> Option<String> {
    if !pieces.iter().any(|piece| piece.styled) {
        if let [piece] = pieces {
            return Some(std::mem::take(&mut piece.rendered));
        }
        let mut output =
            String::with_capacity(pieces.iter().map(|piece| piece.rendered.len()).sum());
        for piece in pieces {
            output.push_str(&piece.rendered);
        }
        return Some(output);
    }
    let choices = choose_sequence(pieces);
    let mut output = String::with_capacity(pieces.iter().map(|piece| piece.rendered.len()).sum());
    let mut spans = Vec::with_capacity(pieces.len());
    for (piece, choice) in pieces.iter().zip(choices) {
        let beginning = output.len();
        let Some(marker) = marker(piece, choice) else {
            output.push_str(&piece.rendered);
            spans.push(beginning..output.len());
            continue;
        };
        let core = core(piece);
        let start =
            piece.rendered.len() - piece.rendered.trim_start_matches([' ', '\t', '\n']).len();
        let end = start + core.len();
        output.push_str(&piece.rendered[..start]);
        output.push_str(marker);
        output.push_str(core);
        output.push_str(marker);
        output.push_str(&piece.rendered[end..]);
        spans.push(beginning..output.len());
    }
    // Delimiter pairing is global: local flank checks alone cannot prove
    // that a distant run did not steal a close. Compare the complete parse
    // with the independently accepted pieces before committing its bytes.
    projection::equivalent(spans.iter().map(|span| &output[span.clone()]), &output)
        .then_some(output)
}

fn choose_sequence(pieces: &[InlinePiece<'_>]) -> Vec<usize> {
    let mut scores = [None; STATES];
    for score in scores.iter_mut().take(choice_count(&pieces[0])) {
        *score = Some(Score::default());
    }
    let mut history = Vec::with_capacity(pieces.len());
    for (index, piece) in pieces.iter().enumerate() {
        let previous = index.checked_sub(1).map(|index| &pieces[index]);
        let following = pieces.get(index + 1);
        let mut next_scores = [None; STATES];
        let mut predecessors = [0; STATES];
        // At most 2 * 3 * 3 parsings of this piece, independent of the
        // accumulated output. There are only nine live sequence states.
        let mut valid = [[[None; CHOICES]; CHOICES]; CHOICES];
        for (state, score) in scores.into_iter().enumerate() {
            let Some(score) = score else { continue };
            let before = state / CHOICES;
            let current = state % CHOICES;
            for (after, witness) in valid[before][current]
                .iter_mut()
                .take(following.map_or(1, choice_count))
                .enumerate()
            {
                let legal = *witness.get_or_insert_with(|| {
                    choice_is_valid(
                        piece,
                        current,
                        previous.map(|piece| (piece, before)),
                        following.map(|piece| (piece, after)),
                    )
                });
                if !legal {
                    continue;
                }
                let next = current * CHOICES + after;
                let candidate = Score {
                    styles: score.styles + usize::from(current != 0),
                    preferred: score.preferred + usize::from(is_preferred(piece, current)),
                };
                if next_scores[next].is_none_or(|best| candidate > best) {
                    next_scores[next] = Some(candidate);
                    predecessors[next] = state;
                }
            }
        }
        history.push(predecessors);
        scores = next_scores;
    }
    let mut state = scores
        .iter()
        .enumerate()
        .filter_map(|(state, score)| score.map(|score| (score, state)))
        .max()
        .expect("unstyled sequence is always available")
        .1;
    let mut choices = vec![0; pieces.len()];
    for index in (0..pieces.len()).rev() {
        choices[index] = state / CHOICES;
        state = history[index][state];
    }
    choices
}

fn choice_count(piece: &InlinePiece<'_>) -> usize {
    if piece.styled { CHOICES } else { 1 }
}

fn marker(piece: &InlinePiece<'_>, choice: usize) -> Option<&'static str> {
    let markers = piece.markers?;
    match choice {
        1 => Some(markers.primary),
        2 => Some(markers.alternate),
        _ => None,
    }
}

fn core<'a>(piece: &'a InlinePiece<'_>) -> &'a str {
    piece.rendered.trim_matches([' ', '\t', '\n'])
}

fn is_preferred(piece: &InlinePiece<'_>, choice: usize) -> bool {
    let Some(markers) = piece.markers else {
        return false;
    };
    choice
        == if core(piece).contains(markers.primary) {
            2
        } else {
            1
        }
}

fn edge(piece: &InlinePiece<'_>, choice: usize, first: bool) -> Option<char> {
    let character = if first {
        piece.rendered.chars().next()
    } else {
        piece.rendered.chars().next_back()
    };
    if !character.is_some_and(|character| matches!(character, ' ' | '\t' | '\n'))
        && let Some(marker) = marker(piece, choice)
    {
        marker.chars().next()
    } else {
        character
    }
}

fn choice_is_valid(
    piece: &InlinePiece<'_>,
    choice: usize,
    preceding: Option<(&InlinePiece<'_>, usize)>,
    following: Option<(&InlinePiece<'_>, usize)>,
) -> bool {
    let Some(marker) = marker(piece, choice) else {
        return true;
    };
    let before = if piece.rendered.starts_with([' ', '\t', '\n']) {
        None
    } else {
        preceding.and_then(|(piece, choice)| edge(piece, choice, false))
    };
    let after = if piece.rendered.ends_with([' ', '\t', '\n']) {
        None
    } else {
        following.and_then(|(piece, choice)| edge(piece, choice, true))
    };
    let delimiter = marker.chars().next();
    if before == delimiter
        && preceding.is_some_and(|(piece, choice)| active_edge(piece, choice, false))
        || after == delimiter
            && following.is_some_and(|(piece, choice)| active_edge(piece, choice, true))
    {
        // Adjacent generated runs are one CommonMark delimiter run. Select
        // another marker or omit this style; never add a source separator.
        return false;
    }
    // Use the actual CommonMark parser for inner runs, escapes and Unicode
    // punctuation. An inert underscore in reuse_level does not conflict.
    // Outside characters carry the complete accepted sequence's flanks.
    let mut probe = String::new();
    append_boundary(&mut probe, before);
    let start = probe.len();
    probe.push_str(marker);
    projection::append_phrasing_rows(&mut probe, core(piece));
    probe.push_str(marker);
    let end = probe.len();
    append_boundary(&mut probe, after);
    #[cfg(test)]
    PARSED_BYTES.with(|bytes| bytes.set(bytes.get() + probe.len()));
    let mut full_span = false;
    let mut strong = 0;
    let mut emphasis = 0;
    for (event, range) in Parser::new(&probe).into_offset_iter() {
        full_span |=
            range == (start..end) && matches!(&event, Event::Start(Tag::Strong | Tag::Emphasis));
        match event {
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong -= 1,
            Event::Start(Tag::Emphasis) => emphasis += 1,
            Event::End(TagEnd::Emphasis) => emphasis -= 1,
            // ***word*** may reverse Strong/Emphasis nesting while
            // retaining both styles on exactly the original scalars.
            Event::Text(_) | Event::Code(_)
                if range.start < end - marker.len()
                    && range.end > start + marker.len()
                    && if marker.len() == 2 {
                        strong == 0
                    } else {
                        emphasis == 0
                    } =>
            {
                return false;
            }
            _ => {}
        }
    }
    full_span
}

fn active_edge(piece: &InlinePiece<'_>, choice: usize, first: bool) -> bool {
    if choice != 0 || first {
        return true;
    }
    // A literal escaped final star/underscore cannot join the next run.
    piece.rendered[..piece.rendered.len() - 1]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count()
        .is_multiple_of(2)
}

fn append_boundary(output: &mut String, boundary: Option<char>) {
    let character = boundary.unwrap_or(' ');
    if character.is_whitespace() {
        // Only the flanking class belongs to this witness. A literal TAB
        // at its artificial block origin would incorrectly open code.
        output.push(' ');
        return;
    }
    if character.is_ascii_punctuation() {
        output.push('\\');
    }
    output.push(character);
}

#[cfg(test)]
std::thread_local! {
    static PARSED_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static WHOLE_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_parser_work_is_bounded_by_piece_bytes_not_output_history() {
        let mut previous = None;
        for count in [64, 256, 1024] {
            let mut pieces = (0..count)
                .map(|index| {
                    InlinePiece::styled(
                        if index % 2 == 0 {
                            "--arg=".into()
                        } else {
                            "reuse_level".into()
                        },
                        if index % 2 == 0 { "**" } else { "*" },
                        if index % 2 == 0 { "__" } else { "_" },
                    )
                })
                .collect::<Vec<_>>();
            let bytes: usize = pieces.iter().map(|piece| piece.rendered.len()).sum();
            PARSED_BYTES.with(|work| work.set(0));
            WHOLE_PARSES.with(|work| work.set(0));
            let output = render_inline_pieces(&mut pieces).unwrap();
            let work = PARSED_BYTES.with(std::cell::Cell::get);
            assert!(work <= 20 * (bytes + 12 * count) + 4 * output.len());
            assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 1);
            if let Some(previous) = previous {
                assert!(work <= previous * 5);
            }
            previous = Some(work);
        }
    }
}
