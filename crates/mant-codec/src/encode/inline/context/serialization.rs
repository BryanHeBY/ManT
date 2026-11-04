//! Commit delimiter choices only after checking the final reader bytes.

use super::{InlinePiece, projection, styles};
use styles::{append_choice, choose_sequence};

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
        if matches!(framing, Framing::Raw)
            && !pieces
                .iter()
                .any(|piece| matches!(piece.accepted, super::AcceptedMeaning::UnsupportedLink))
        {
            return Some(output);
        }
        let output = framing.render(&output);
        return projection::equivalent_pieces(pieces, None, &output, framing).then_some(output);
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
    if projection::equivalent_pieces(pieces, Some(&choices), &output, framing) {
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
        if projection::equivalent_pieces(pieces, None, &unwrapped, framing) {
            return Some(unwrapped);
        }
    }
    None
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

#[cfg(test)]
mod tests;
