//! Finite a2roffsu numeric prefixes before temporary-indent clamping.

/// `roff_term_pre_ti()` accepts the `a2roffsu()/strtod()` numeric prefix,
/// including decimal exponents, and ignores a trailing unconsumed suffix.
/// Its own 72-EN cap applies after conversion; the unrelated bounded source
/// distance parser would incorrectly reject perfectly valid `5000n` here.
pub(super) fn temporary_length_units(argument: &str) -> Option<i32> {
    let argument = argument.trim_start();
    let (scale, end) = numeric_prefix(argument)?;
    let ratio = match argument.as_bytes().get(end) {
        Some(b'u') => 1.0,
        Some(b'c') => 240.0 / 2.54,
        Some(b'f') => 65_536.0,
        Some(b'i') => 240.0,
        Some(b'M') => 0.24,
        Some(b'P' | b'v') => 40.0,
        Some(b'p') => 10.0 / 3.0,
        _ => 24.0,
    };
    let units = scale * ratio;
    if !units.is_finite() {
        return None;
    }
    // ascii_hspan() truncates with a small conversion bias. Finite spans
    // remain signed: +-4n and --4n exercise different pre_ti branches.
    // Saturation keeps huge operands safe before the request's 72-EN cap.
    #[allow(clippy::cast_possible_truncation)]
    Some((units + units.signum() * 0.01).clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32)
}

fn numeric_prefix(argument: &str) -> Option<(f64, usize)> {
    let bytes = argument.as_bytes();
    let signed = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    if matches!(bytes.get(signed..signed + 2), Some(b"0x" | b"0X")) {
        return hexadecimal_prefix(argument, signed);
    }
    let mut end = signed;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let leading = end - signed;
    let mut fractional = 0;
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        let start = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        fractional = end - start;
    }
    if leading == 0 && fractional == 0 {
        return None;
    }
    end = exponent_end(bytes, end, b'e', b'E');
    Some((argument[..end].parse().ok()?, end))
}

fn exponent_end(bytes: &[u8], end: usize, lower: u8, upper: u8) -> usize {
    if !bytes
        .get(end)
        .is_some_and(|byte| *byte == lower || *byte == upper)
    {
        return end;
    }
    let mut next = end + 1;
    if matches!(bytes.get(next), Some(b'+' | b'-')) {
        next += 1;
    }
    let start = next;
    while bytes.get(next).is_some_and(u8::is_ascii_digit) {
        next += 1;
    }
    if next == start { end } else { next }
}

fn hexadecimal_prefix(argument: &str, signed: usize) -> Option<(f64, usize)> {
    let bytes = argument.as_bytes();
    let mut end = signed + 2;
    let mut digits = 0usize;
    let mut first_significant = None;
    let mut kept = 0usize;
    let mut significand = 0.0;
    let mut feed = |digit: u32| {
        if digit != 0 && first_significant.is_none() {
            first_significant = Some(digits);
        }
        if first_significant.is_some() && kept < 15 {
            significand = significand * 16.0 + f64::from(digit);
            kept += 1;
        }
        digits += 1;
    };
    while let Some(digit) = bytes
        .get(end)
        .and_then(|byte| char::from(*byte).to_digit(16))
    {
        feed(digit);
        end += 1;
    }
    let whole_digits = end - signed - 2;
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        while let Some(digit) = bytes
            .get(end)
            .and_then(|byte| char::from(*byte).to_digit(16))
        {
            feed(digit);
            end += 1;
        }
    }
    if digits == 0 {
        // strtod("0x...") falls back to the initial decimal zero.
        return Some((0.0, signed + 1));
    }
    let exponent_end = exponent_end(bytes, end, b'p', b'P');
    let exponent = if exponent_end == end {
        0
    } else {
        let token = &argument[end + 1..exponent_end];
        token.parse::<i64>().unwrap_or_else(|_| {
            if token.starts_with('-') {
                i64::MIN
            } else {
                i64::MAX
            }
        })
    };
    end = exponent_end;
    let Some(first_significant) = first_significant else {
        // Consume the whole exponent, but zero stays finite even where
        // powi() would overflow. Do not compute 0 * infinity or reject zero.
        return Some((0.0, end));
    };
    let places = i64::try_from(whole_digits)
        .unwrap_or(i64::MAX)
        .saturating_sub(i64::try_from(first_significant).unwrap_or(i64::MAX))
        .saturating_sub(i64::try_from(kept).unwrap_or(i64::MAX));
    let power = places.saturating_mul(4).saturating_add(exponent);
    let mut scale = if power < -1075 {
        0.0 // Finite underflow; every character-device span is zero.
    } else if power > 1024 {
        f64::INFINITY // Non-finite/C integer-conversion behavior is not copied.
    } else {
        significand * 2.0f64.powi(i32::try_from(power).ok()?)
    };
    if bytes.first() == Some(&b'-') {
        scale = -scale;
    }
    Some((scale, end))
}
