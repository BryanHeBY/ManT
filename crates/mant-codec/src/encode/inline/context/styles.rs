//! Choose legal emphasis delimiters across the complete accepted phrasing stream.

use std::collections::VecDeque;

use super::InlinePiece;

pub(super) fn render_inline_pieces(pieces: &mut [InlinePiece<'_>]) -> String {
    let (preceding, following) = nonempty_neighbors(pieces);
    let mut pending = pieces
        .iter()
        .enumerate()
        .filter_map(|(index, piece)| {
            (piece.styled && !style_is_valid(pieces, index, &preceding, &following))
                .then_some(index)
        })
        .collect::<VecDeque<_>>();
    while let Some(index) = pending.pop_front() {
        if !pieces[index].styled || style_is_valid(pieces, index, &preceding, &following) {
            continue;
        }
        pieces[index].styled = false;
        for neighbor in [preceding[index], following[index]].into_iter().flatten() {
            if pieces[neighbor].styled {
                pending.push_back(neighbor);
            }
        }
    }

    let following = following_characters(pieces);
    let mut output = String::new();
    for (index, piece) in pieces.iter().enumerate() {
        if let Some(markers) = piece.markers.filter(|_| piece.styled) {
            output.push_str(&render_styled(
                &piece.rendered,
                markers.primary,
                markers.alternate,
                &output,
                following[index],
            ));
        } else {
            output.push_str(&piece.rendered);
        }
    }
    output
}

fn nonempty_neighbors(pieces: &[InlinePiece<'_>]) -> (Vec<Option<usize>>, Vec<Option<usize>>) {
    let mut preceding = vec![None; pieces.len()];
    let mut current = None;
    for (index, piece) in pieces.iter().enumerate() {
        preceding[index] = current;
        if !piece.rendered.is_empty() {
            current = Some(index);
        }
    }
    let mut following = vec![None; pieces.len()];
    current = None;
    for (index, piece) in pieces.iter().enumerate().rev() {
        following[index] = current;
        if !piece.rendered.is_empty() {
            current = Some(index);
        }
    }
    (preceding, following)
}

fn style_is_valid(
    pieces: &[InlinePiece<'_>],
    index: usize,
    preceding: &[Option<usize>],
    following: &[Option<usize>],
) -> bool {
    let piece = &pieces[index];
    let core = piece.rendered.trim_matches([' ', '\t', '\n']);
    let before = piece
        .rendered
        .starts_with([' ', '\t', '\n'])
        .then_some(' ')
        .or_else(|| preceding[index].and_then(|index| pieces[index].last_output_character()));
    let after = piece
        .rendered
        .ends_with([' ', '\t', '\n'])
        .then_some(' ')
        .or_else(|| following[index].and_then(|index| pieces[index].first_output_character()));
    let markers = piece.markers.expect("styled pieces carry markers");
    [markers.primary, markers.alternate]
        .into_iter()
        .any(|marker| {
            style_marker_is_available(core, marker)
                && can_delimit_style(core, marker, before, after)
        })
}

fn following_characters(pieces: &[InlinePiece<'_>]) -> Vec<Option<char>> {
    let mut following = vec![None; pieces.len()];
    let mut current = None;
    for (index, piece) in pieces.iter().enumerate().rev() {
        following[index] = current;
        current = piece.first_output_character().or(current);
    }
    following
}

/// Render one styled span with the ordinary asterisk marker, switching to the
/// equivalent underscore marker when adjacent styles would form an ambiguous
/// run of `*`. This keeps the output pure Markdown while preserving emphasis
/// within ordinary words, where underscore delimiters are intentionally inert.
fn render_styled(
    rendered: &str,
    primary_marker: &str,
    alternate_marker: &str,
    preceding: &str,
    following: Option<char>,
) -> String {
    let core = rendered.trim_matches([' ', '\t', '\n']);
    if core.is_empty() {
        return rendered.to_owned();
    }
    let leading_width = rendered.len() - rendered.trim_start_matches([' ', '\t', '\n']).len();
    let trailing_width = rendered.len() - rendered.trim_end_matches([' ', '\t', '\n']).len();
    let leading = &rendered[..leading_width];
    let trailing = &rendered[rendered.len() - trailing_width..];
    let prefer_alternate = preceding.ends_with('*') || core.contains(primary_marker);
    let markers = if prefer_alternate {
        [alternate_marker, primary_marker]
    } else {
        [primary_marker, alternate_marker]
    };
    let preceding = preceding.chars().next_back();
    let marker = markers.into_iter().find(|marker| {
        style_marker_is_available(core, marker)
            && can_delimit_style(core, marker, preceding, following)
    });
    marker.map_or_else(
        || rendered.to_owned(),
        |marker| format!("{leading}{marker}{core}{marker}{trailing}"),
    )
}

fn can_delimit_style(
    core: &str,
    marker: &str,
    preceding: Option<char>,
    following: Option<char>,
) -> bool {
    // CommonMark classifies an entire delimiter run using the characters
    // outside that run. A generated inner style with the same marker joins
    // the outer run; its marker is not the next content character.
    let marker_character = char::from(marker.as_bytes()[0]);
    let Some(first) = core.trim_start_matches(marker_character).chars().next() else {
        return false;
    };
    let Some(last) = core.trim_end_matches(marker_character).chars().next_back() else {
        return false;
    };
    can_open_delimiter(marker, preceding, Some(first))
        && can_close_delimiter(marker, Some(last), following)
}

fn style_marker_is_available(core: &str, marker: &str) -> bool {
    if !core.contains(marker) {
        return true;
    }
    // A single emphasis marker can enclose generated strong runs. Keep odd
    // inner runs on the alternate marker: two nested single runs would be
    // parsed as strong instead of preserving emphasis.
    if marker.len() != 1 {
        return false;
    }
    let marker_character = char::from(marker.as_bytes()[0]);
    core.split(|character| character != marker_character)
        .filter(|run| !run.is_empty())
        .all(|run| run.len().is_multiple_of(2))
}

fn can_open_delimiter(marker: &str, preceding: Option<char>, following: Option<char>) -> bool {
    let (left_flanking, right_flanking) = delimiter_flanking(preceding, following);
    if marker.starts_with('*') {
        left_flanking
    } else {
        left_flanking && (!right_flanking || preceding.is_some_and(is_commonmark_punctuation))
    }
}

fn can_close_delimiter(marker: &str, preceding: Option<char>, following: Option<char>) -> bool {
    let (left_flanking, right_flanking) = delimiter_flanking(preceding, following);
    if marker.starts_with('*') {
        right_flanking
    } else {
        right_flanking && (!left_flanking || following.is_some_and(is_commonmark_punctuation))
    }
}

fn delimiter_flanking(preceding: Option<char>, following: Option<char>) -> (bool, bool) {
    let preceding_whitespace = preceding.is_none_or(char::is_whitespace);
    let following_whitespace = following.is_none_or(char::is_whitespace);
    let preceding_punctuation = preceding.is_some_and(is_commonmark_punctuation);
    let following_punctuation = following.is_some_and(is_commonmark_punctuation);
    let left_flanking = !following_whitespace
        && (!following_punctuation || preceding_whitespace || preceding_punctuation);
    let right_flanking = !preceding_whitespace
        && (!preceding_punctuation || following_whitespace || following_punctuation);
    (left_flanking, right_flanking)
}

fn is_commonmark_punctuation(character: char) -> bool {
    // CommonMark uses Unicode punctuation and symbol categories. Treating the
    // remaining non-alphanumeric, non-whitespace scalars as punctuation is a
    // conservative superset: an unusual combining mark can lose styling, but
    // it cannot make delimiter bytes visible in the projected text.
    !character.is_alphanumeric() && !character.is_whitespace()
}
