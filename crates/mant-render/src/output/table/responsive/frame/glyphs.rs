//! Unicode stroke selection; no source or viewport state.
use mant_ir::TableBorderStyle as Stroke;
pub(super) fn horizontal_glyph(stroke: Stroke) -> char {
    match stroke {
        Stroke::Single => '─',
        Stroke::Double => '═',
    }
}

pub(super) fn crossing(horizontal: Stroke, vertical: Stroke, top: bool, bottom: bool) -> char {
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

pub(super) fn vertical_glyph(style: Stroke) -> char {
    match style {
        Stroke::Single => '│',
        Stroke::Double => '║',
    }
}

pub(super) fn junction(
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

pub(super) fn frame_corner(
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
