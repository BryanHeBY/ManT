//! Terminal table stroke selection shared by CLI and TUI.
//!
//! These glyphs are presentation choices, not document IR or native formatter
//! execution. Callers retain layout, style and source-coordinate ownership.
//! Top/bottom flags select outside boundaries; top takes precedence if both
//! are set. With neither flag, intersections remain interior/side tees.
use mant_ir::TableBorderStyle as Stroke;
/// A complete horizontal stroke.
#[must_use]
pub fn horizontal_glyph(stroke: Stroke) -> char {
    match stroke {
        Stroke::Single => '─',
        Stroke::Double => '═',
    }
}

/// A full crossing or top/bottom tee of the requested strengths.
#[must_use]
pub fn crossing(horizontal: Stroke, vertical: Stroke, top: bool, bottom: bool) -> char {
    match (horizontal, vertical, top, bottom) {
        (Stroke::Single, Stroke::Single, true, _) => '┬',
        (Stroke::Single, Stroke::Single, _, true) => '┴',
        (Stroke::Single, Stroke::Single, _, _) => '┼',
        (Stroke::Double, Stroke::Double, true, _) => '╦',
        (Stroke::Double, Stroke::Double, _, true) => '╩',
        (Stroke::Double, Stroke::Double, _, _) => '╬',
        (Stroke::Single, Stroke::Double, true, _) => '╥',
        (Stroke::Single, Stroke::Double, _, true) => '╨',
        (Stroke::Single, Stroke::Double, _, _) => '╫',
        (Stroke::Double, Stroke::Single, true, _) => '╤',
        (Stroke::Double, Stroke::Single, _, true) => '╧',
        (Stroke::Double, Stroke::Single, _, _) => '╪',
    }
}

/// A complete vertical stroke.
#[must_use]
pub fn vertical_glyph(style: Stroke) -> char {
    match style {
        Stroke::Single => '│',
        Stroke::Double => '║',
    }
}

/// A partial horizontal/vertical intersection; absent strokes remain blank.
#[must_use]
pub fn junction(
    left: Option<Stroke>,
    right: Option<Stroke>,
    vertical: Option<Stroke>,
    top: bool,
    bottom: bool,
) -> char {
    let horizontal = match (left, right) {
        (Some(left), Some(right)) => Some(left.stronger(right)),
        (left, right) => left.or(right),
    };
    let Some(vertical) = vertical else {
        return horizontal.map_or(' ', horizontal_glyph);
    };
    let Some(horizontal) = horizontal else {
        return vertical_glyph(vertical);
    };
    if left.is_some() && right.is_some() {
        return crossing(horizontal, vertical, top, bottom);
    }
    match (horizontal, vertical, right.is_some()) {
        (Stroke::Single, Stroke::Single, true) => '├',
        (Stroke::Single, Stroke::Single, false) => '┤',
        (Stroke::Double, Stroke::Double, true) => '╠',
        (Stroke::Double, Stroke::Double, false) => '╣',
        (Stroke::Single, Stroke::Double, true) => '╟',
        (Stroke::Single, Stroke::Double, false) => '╢',
        (Stroke::Double, Stroke::Single, true) => '╞',
        (Stroke::Double, Stroke::Single, false) => '╡',
    }
}

fn corner(stroke: Stroke, left: bool, top: bool, bottom: bool) -> char {
    match (stroke, left, top, bottom) {
        (Stroke::Single, true, true, _) => '┌',
        (Stroke::Single, false, true, _) => '┐',
        (Stroke::Single, true, _, true) => '└',
        (Stroke::Single, false, _, true) => '┘',
        (Stroke::Single, true, _, _) => '├',
        (Stroke::Single, false, _, _) => '┤',
        (Stroke::Double, true, true, _) => '╔',
        (Stroke::Double, false, true, _) => '╗',
        (Stroke::Double, true, _, true) => '╚',
        (Stroke::Double, false, _, true) => '╝',
        (Stroke::Double, true, _, _) => '╠',
        (Stroke::Double, false, _, _) => '╣',
    }
}

/// An outside corner or side tee with independent stroke strengths.
#[must_use]
pub fn frame_corner(
    horizontal: Stroke,
    vertical: Stroke,
    left: bool,
    top: bool,
    bottom: bool,
) -> char {
    if !top && !bottom {
        return junction(
            (!left).then_some(horizontal),
            left.then_some(horizontal),
            Some(vertical),
            false,
            false,
        );
    }
    if horizontal == vertical {
        return corner(vertical, left, top, bottom);
    }
    match (horizontal, vertical, left, top) {
        (Stroke::Single, Stroke::Double, true, true) => '╓',
        (Stroke::Single, Stroke::Double, false, true) => '╖',
        (Stroke::Single, Stroke::Double, true, false) => '╙',
        (Stroke::Single, Stroke::Double, false, false) => '╜',
        (Stroke::Double, Stroke::Single, true, true) => '╒',
        (Stroke::Double, Stroke::Single, false, true) => '╕',
        (Stroke::Double, Stroke::Single, true, false) => '╘',
        (Stroke::Double, Stroke::Single, false, false) => '╛',
        _ => unreachable!("matching strengths returned above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_strokes_keep_the_shared_terminal_character_contract() {
        use Stroke::{Double, Single};
        let cases = [
            (Single, Single, ['┬', '┼', '┴'], ['┌', '┐', '└', '┘']),
            (Single, Double, ['╥', '╫', '╨'], ['╓', '╖', '╙', '╜']),
            (Double, Single, ['╤', '╪', '╧'], ['╒', '╕', '╘', '╛']),
            (Double, Double, ['╦', '╬', '╩'], ['╔', '╗', '╚', '╝']),
        ];
        for (horizontal, vertical, crosses, corners) in cases {
            for ((top, bottom), expected) in [(true, false), (false, false), (false, true)]
                .into_iter()
                .zip(crosses)
            {
                assert_eq!(crossing(horizontal, vertical, top, bottom), expected);
                assert_eq!(
                    junction(
                        Some(horizontal),
                        Some(horizontal),
                        Some(vertical),
                        top,
                        bottom
                    ),
                    expected
                );
            }
            for ((left, top, bottom), expected) in [
                (true, true, false),
                (false, true, false),
                (true, false, true),
                (false, false, true),
            ]
            .into_iter()
            .zip(corners)
            {
                assert_eq!(
                    frame_corner(horizontal, vertical, left, top, bottom),
                    expected
                );
            }
        }
        assert_eq!(junction(None, None, None, false, false), ' ');
        assert_eq!(
            junction(Some(Single), Some(Double), None, false, false),
            '═'
        );
        assert_eq!(junction(None, None, Some(Single), false, false), '│');
        assert_eq!(frame_corner(Single, Double, true, false, false), '╟');
        assert_eq!(frame_corner(Double, Single, false, false, false), '╡');
    }
}
