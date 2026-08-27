use crate::numeric::evaluate_register_expression;

use super::EnvironmentError;

pub(super) struct NameExpansion {
    pub(super) bytes: Vec<u8>,
    pub(super) steps: usize,
    pub(super) missing_references: Vec<Vec<u8>>,
}

pub(super) struct BasicInteger {
    pub(super) value: i64,
    pub(super) relative: Option<i8>,
}

/// Convert unbounded expression evaluation into roff's fixed-width register.
pub(super) fn wrapping_i64_to_i32(value: i64) -> i32 {
    let bits = u32::try_from(value.rem_euclid(1_i64 << 32))
        .expect("modulo 2^32 always fits the public register representation");
    i32::from_ne_bytes(bits.to_ne_bytes())
}

pub(super) fn parse_basic_integer(
    expression: &[u8],
    scale: bool,
    leading_relative: bool,
) -> Result<Option<BasicInteger>, EnvironmentError> {
    let parsed = evaluate_register_expression(expression, scale, leading_relative)
        .map_err(|_| EnvironmentError::RegisterExpression)?;
    Ok(parsed.map(|parsed| BasicInteger {
        value: parsed.value,
        relative: parsed.relative,
    }))
}

pub(super) fn push_expanded_bytes(
    output: &mut Vec<u8>,
    bytes: &[u8],
    maximum_output_bytes: usize,
) -> Result<(), EnvironmentError> {
    let length = output
        .len()
        .checked_add(bytes.len())
        .ok_or(EnvironmentError::OutputLimit)?;
    if length > maximum_output_bytes {
        return Err(EnvironmentError::OutputLimit);
    }
    output.extend_from_slice(bytes);
    Ok(())
}

pub(super) fn read_name(bytes: &[u8]) -> Option<(&[u8], usize)> {
    if bytes.first() == Some(&b'[') {
        let mut cursor = 1;
        let mut depth = 1_usize;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'[' => depth = depth.saturating_add(1),
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return (!bytes[1..cursor].is_empty())
                            .then_some((&bytes[1..cursor], cursor + 1));
                    }
                }
                _ => {}
            }
            cursor += 1;
        }
        return None;
    }
    if bytes.first() == Some(&b'(') {
        if bytes.get(1) == Some(&b'\\') && matches!(bytes.get(2), Some(b'*' | b'n')) {
            let inner_length = reference_name_length(&bytes[3..])?;
            let end = 3_usize.checked_add(inner_length)?;
            return (end <= bytes.len()).then(|| (&bytes[1..end], end));
        }
        return (bytes.len() >= 3).then(|| (&bytes[1..3], 3));
    }
    (!bytes.is_empty()).then(|| (&bytes[..1], 1))
}

fn reference_name_length(bytes: &[u8]) -> Option<usize> {
    match bytes.first().copied()? {
        b'[' => {
            let mut cursor = 1;
            let mut depth = 1_usize;
            while cursor < bytes.len() {
                match bytes[cursor] {
                    b'[' => depth = depth.saturating_add(1),
                    b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(cursor + 1);
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
            None
        }
        b'(' => (bytes.len() >= 3).then_some(3),
        _ => Some(1),
    }
}

pub(super) fn escaped_by_previous(bytes: &[u8], index: usize, escape: u8) -> bool {
    let mut count = 0_usize;
    let mut cursor = index;
    while cursor > 0 && bytes[cursor - 1] == escape {
        count += 1;
        cursor -= 1;
    }
    count % 2 == 1
}

pub(super) fn contains_reference_escape(bytes: &[u8], escape: u8) -> bool {
    bytes.iter().enumerate().any(|(index, byte)| {
        *byte == escape
            && !escaped_by_previous(bytes, index, escape)
            && matches!(bytes.get(index + 1), Some(b'*' | b'n'))
    })
}

pub(super) fn contains_literal_string_name_escape(bytes: &[u8], escape: u8) -> bool {
    bytes.iter().enumerate().any(|(index, byte)| {
        *byte == escape
            && !escaped_by_previous(bytes, index, escape)
            && matches!(bytes.get(index + 1), Some(b'\\' | b'e'))
    })
}
