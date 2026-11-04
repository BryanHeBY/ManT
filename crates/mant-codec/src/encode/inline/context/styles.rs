//! Choose delimiters over the complete phrasing sequence, including both seams.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::InlinePiece;

mod projection;

// A piece has three choices: its body alone, asterisk, or underscore. A
// transition knows the preceding and following choices, so delimiter runs
// cannot silently merge after a locally valid choice was emitted.
const CHOICES: usize = 3;
const STATES: usize = CHOICES * CHOICES;

#[derive(Clone, Copy)]
pub(super) enum Framing {
    Raw,
    Rows {
        manual_links: bool,
        interruptible: bool,
    },
}

impl Framing {
    pub(super) fn render(self, raw: &str) -> String {
        match self {
            Self::Raw => raw.to_owned(),
            Self::Rows {
                manual_links,
                interruptible,
            } => super::super::render_inline_rows_with_origin(raw, manual_links, interruptible),
        }
    }
}

#[derive(Clone, Copy, Default, Eq, Ord, PartialEq, PartialOrd)]
struct Score {
    styles: usize,
    preferred: usize,
}

pub(super) fn render_inline_pieces(
    pieces: &mut [InlinePiece<'_>],
    framing: Framing,
) -> Option<String> {
    if !pieces.iter().any(|piece| piece.styled) {
        let mut output =
            String::with_capacity(pieces.iter().map(|piece| piece.rendered.len()).sum());
        for piece in pieces.iter() {
            append_safe_piece(&mut output, &piece.rendered);
        }
        if matches!(framing, Framing::Raw) {
            return Some(output);
        }
        let output = framing.render(&output);
        return projection::equivalent_framed(
            pieces.iter().map(|piece| (piece.rendered.as_str(), None)),
            &output,
            framing,
        )
        .then_some(output);
    }
    let choices = choose_sequence(pieces);
    let mut output = String::with_capacity(pieces.iter().map(|piece| piece.rendered.len()).sum());
    for (piece, choice) in pieces.iter().zip(choices.iter().copied()) {
        append_choice(&mut output, piece, choice);
    }
    let output = framing.render(&output);
    // Delimiter pairing is global: local flank checks alone cannot prove
    // that a distant run did not steal a close. Compare the complete parse
    // with the independently accepted pieces before committing its bytes.
    if projection::equivalent_framed(
        pieces.iter().zip(choices).map(|(piece, choice)| {
            (
                piece.rendered.as_str(),
                marker(piece, choice).map(projection::Style::from_marker),
            )
        }),
        &output,
        framing,
    ) {
        return Some(output);
    }
    if matches!(framing, Framing::Rows { .. }) {
        // An author-edge entity may invalidate a newly selected outer style
        // while its accepted child remains representable. Keep those child
        // styles when the same final-byte check proves them safe. This is one
        // additional complete parse, never a parse per accumulated prefix.
        let mut unwrapped = String::new();
        for piece in pieces.iter() {
            append_safe_piece(&mut unwrapped, &piece.rendered);
        }
        let unwrapped = framing.render(&unwrapped);
        if projection::equivalent_framed(
            pieces.iter().map(|piece| (piece.rendered.as_str(), None)),
            &unwrapped,
            framing,
        ) {
            return Some(unwrapped);
        }
    }
    None
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
        let mut child_valid = [None; CHOICES];
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
                    let child_is_valid = *child_valid[current]
                        .get_or_insert_with(|| choice_preserves_child(piece, current));
                    child_is_valid
                        && choice_is_valid(
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

fn append_choice(output: &mut String, piece: &InlinePiece<'_>, choice: usize) {
    let Some(marker) = marker(piece, choice) else {
        append_safe_piece(output, &piece.rendered);
        return;
    };
    let core = core(piece);
    let start = piece.rendered.len() - piece.rendered.trim_start_matches(char::is_whitespace).len();
    let end = start + core.len();
    output.push_str(&piece.rendered[..start]);
    output.push_str(marker);
    output.push_str(core);
    output.push_str(marker);
    output.push_str(&piece.rendered[end..]);
}

/// A generated link opener can acquire image meaning across an otherwise
/// transparent seam. Inspect only emitted piece boundaries, so bangs inside
/// destinations/autolinks and bangs followed by active style markers stay
/// untouched. Empty metadata pieces cannot hide the actual next opener.
pub(super) fn append_safe_piece(output: &mut String, piece: &str) {
    if piece.starts_with('[')
        && output.ends_with('!')
        && output[..output.len() - 1]
            .bytes()
            .rev()
            .take_while(|byte| *byte == b'\\')
            .count()
            .is_multiple_of(2)
    {
        output.insert(output.len() - 1, '\\');
    }
    output.push_str(piece);
}

fn choice_preserves_child(piece: &InlinePiece<'_>, choice: usize) -> bool {
    let Some(marker) = marker(piece, choice) else {
        return true;
    };
    let mut candidate = String::with_capacity(piece.rendered.len() + 2 * marker.len());
    append_choice(&mut candidate, piece, choice);
    projection::preserves_child(
        &piece.rendered,
        projection::Style::from_marker(marker),
        &candidate,
    )
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
    piece.rendered.trim_matches(char::is_whitespace)
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
    if !character.is_some_and(char::is_whitespace)
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
    let before = if piece.rendered.starts_with(char::is_whitespace) {
        None
    } else {
        preceding.and_then(|(piece, choice)| edge(piece, choice, false))
    };
    let after = if piece.rendered.ends_with(char::is_whitespace) {
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
    static PROJECTED_SCALARS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
            PROJECTED_SCALARS.with(|work| work.set(0));
            let output = render_inline_pieces(&mut pieces, Framing::Raw).unwrap();
            let work = PARSED_BYTES.with(std::cell::Cell::get);
            // Two cached child checks add at most four piece parsings to
            // the existing bounded flank witnesses. No historical prefix
            // is rescanned as the sequence grows.
            assert!(work <= 24 * (bytes + 12 * count) + 4 * output.len());
            assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 1);
            assert!(PROJECTED_SCALARS.with(std::cell::Cell::get) <= 3 * bytes);
            if let Some(previous) = previous {
                assert!(work <= previous * 5);
            }
            previous = Some(work);
        }
    }

    #[test]
    fn nested_parser_work_is_bounded_for_separate_length_and_depth_axes() {
        // Recursive formatting revisits an accepted child at every outer
        // style. Keep depth separate from flat sequence length: this budget
        // is O(depth * (text bytes + depth)), not an arbitrary-depth linearity
        // claim. Both counters observe actual parser/projection loop work.
        let axes = [
            (4, 64),
            (4, 256),
            (4, 1024),
            (1, 128),
            (4, 128),
            (16, 128),
            (64, 128),
        ];
        for (depth, bytes) in axes {
            let mut node = mant_ir::Inline::Text {
                value: "x".repeat(bytes),
            };
            for level in 0..depth {
                node = if level % 2 == 0 {
                    mant_ir::Inline::Strong {
                        children: vec![node],
                    }
                } else {
                    mant_ir::Inline::Emphasis {
                        children: vec![node],
                    }
                };
            }
            PARSED_BYTES.with(|work| work.set(0));
            PROJECTED_SCALARS.with(|work| work.set(0));
            let _ = super::super::render_inline_raw(
                &[node],
                crate::encode::MarkdownOptions::default(),
                false,
            );
            assert!(PARSED_BYTES.with(std::cell::Cell::get) <= 8 * depth * (bytes + 8 * depth + 8));
            assert!(PROJECTED_SCALARS.with(std::cell::Cell::get) <= 3 * depth * bytes);
        }
    }

    #[test]
    fn final_row_validation_has_separate_flat_long_piece_and_link_budgets() {
        let framing = Framing::Rows {
            manual_links: false,
            interruptible: true,
        };
        // The only complete parse belongs to the final byte sequence. Each
        // independent expected child is visited once; no prefix is reparsed.
        for count in [64, 256, 1024] {
            let mut pieces = (0..count)
                .map(|_| InlinePiece::plain("word ".into()))
                .collect::<Vec<_>>();
            let bytes: usize = pieces.iter().map(|piece| piece.rendered.len()).sum();
            PARSED_BYTES.with(|work| work.set(0));
            WHOLE_PARSES.with(|work| work.set(0));
            assert!(render_inline_pieces(&mut pieces, framing).is_some());
            assert!(PARSED_BYTES.with(std::cell::Cell::get) <= 2 * bytes + 4 * count + 8);
            assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 1);
        }
        for bytes in [64, 1024, 16_384] {
            let mut pieces = [InlinePiece::plain("x".repeat(bytes))];
            PARSED_BYTES.with(|work| work.set(0));
            WHOLE_PARSES.with(|work| work.set(0));
            assert!(render_inline_pieces(&mut pieces, framing).is_some());
            assert!(PARSED_BYTES.with(std::cell::Cell::get) <= 2 * bytes + 4);
            assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 1);
        }
        for count in [16, 64, 256] {
            let mut pieces = (0..count)
                .map(|_| {
                    InlinePiece::plain(format!("[a](https://a.test/{} \"owner\")", "x".repeat(128)))
                })
                .collect::<Vec<_>>();
            let bytes: usize = pieces.iter().map(|piece| piece.rendered.len()).sum();
            PARSED_BYTES.with(|work| work.set(0));
            WHOLE_PARSES.with(|work| work.set(0));
            assert!(render_inline_pieces(&mut pieces, framing).is_some());
            assert!(PARSED_BYTES.with(std::cell::Cell::get) <= 2 * bytes + 4 * count);
            assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 1);
        }
    }

    #[test]
    fn final_recovery_keeps_accepted_children_with_at_most_two_whole_parses() {
        let mut pieces = [
            InlinePiece::styled("a _**b**_".into(), "*", "_"),
            InlinePiece::plain(" ".into()),
        ];
        PARSED_BYTES.with(|work| work.set(0));
        WHOLE_PARSES.with(|work| work.set(0));
        let markdown = render_inline_pieces(
            &mut pieces,
            Framing::Rows {
                manual_links: false,
                interruptible: true,
            },
        )
        .unwrap();
        assert_eq!(markdown, "a _**b**_&#32;");
        assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 2);
        assert!(PARSED_BYTES.with(std::cell::Cell::get) < 2048);
        // Canonical paragraph and interruptible roots have different leading
        // spellings; both obey the exact same independently checked hard rows.
        for interruptible in [false, true] {
            let mut pieces = [InlinePiece::plain("\n \t\n".into())];
            WHOLE_PARSES.with(|work| work.set(0));
            let markdown = render_inline_pieces(
                &mut pieces,
                Framing::Rows {
                    manual_links: false,
                    interruptible,
                },
            )
            .unwrap();
            assert!(markdown.starts_with(if interruptible { "&#10;" } else { "<br />" }));
            assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 1);
        }
    }
}
