//! Deterministic roff scaled-number parsing for scanner and execution stages.

use std::cmp::Ordering;

/// One recognized roff scale suffix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScaleUnit {
    /// Device-independent basic units without an explicit suffix.
    Basic,
    /// One twelfth of an em.
    En,
    /// One em.
    Em,
    /// Printer points.
    Point,
    /// Picas.
    Pica,
    /// Inches.
    Inch,
    /// Centimeters.
    Centimeter,
    /// Millimeters.
    Millimeter,
    /// Vertical line spacing units.
    Vertical,
    /// Current font-size units.
    FontSize,
}

/// One exact decimal scaled value without binary floating point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScaledValue {
    /// Signed magnitude in [`Self::FRACTION_SCALE`]ths of one `unit`.
    pub(crate) magnitude: i64,
    /// Dimension selected by the optional roff suffix.
    pub(crate) unit: ScaleUnit,
    /// Whether a leading `+` or `-` requested relative application by a caller.
    pub(crate) relative: bool,
}

impl ScaledValue {
    /// Decimal precision retained by the scanner-stage evaluator.
    pub(crate) const FRACTION_SCALE: i64 = 1_000_000;

    /// Add two values only when they use the same scale dimension.
    pub(crate) fn checked_add(self, other: Self) -> Result<Self, NumericError> {
        let (left, right, unit) = if self.unit == other.unit {
            (self.magnitude, other.magnitude, self.unit)
        } else {
            let left = self
                .physical_magnitude()
                .ok_or(NumericError::MismatchedUnits)?;
            let right = other
                .physical_magnitude()
                .ok_or(NumericError::MismatchedUnits)?;
            (left, right, ScaleUnit::Point)
        };
        let magnitude = left.checked_add(right).ok_or(NumericError::Overflow)?;
        Ok(Self {
            magnitude,
            unit,
            relative: self.relative || other.relative,
        })
    }

    /// Compare exact device-independent physical dimensions, or matching
    /// context-dependent scale units.  The latter deliberately do not invent
    /// a device/font environment merely to compare a number.
    pub(crate) fn compare(self, other: Self) -> Option<Ordering> {
        if self.unit == other.unit {
            return Some(self.magnitude.cmp(&other.magnitude));
        }
        Some(self.physical_magnitude()?.cmp(&other.physical_magnitude()?))
    }

    /// Convert physical units to one 127th of a printer point.  This is the
    /// least common exact denominator for inches, centimetres, and millimetres
    /// (one inch is 72 points and 2.54 centimetres).
    fn physical_magnitude(self) -> Option<i64> {
        let factor = match self.unit {
            ScaleUnit::Point => 127,
            ScaleUnit::Pica => 1_524,
            ScaleUnit::Inch => 9_144,
            ScaleUnit::Centimeter => 3_600,
            ScaleUnit::Millimeter => 360,
            ScaleUnit::Basic
            | ScaleUnit::En
            | ScaleUnit::Em
            | ScaleUnit::Vertical
            | ScaleUnit::FontSize => return None,
        };
        self.magnitude.checked_mul(factor)
    }
}

/// Stable reason a scanner-stage scaled expression could not be evaluated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NumericError {
    /// Input contained no number.
    Empty,
    /// A sign or decimal point was not followed by digits.
    MissingDigits,
    /// More than six nonzero decimal places would lose precision.
    Precision,
    /// An unsupported byte occurred where a scale suffix or operator was expected.
    InvalidByte,
    /// Decimal accumulation or arithmetic exceeded `i64`.
    Overflow,
    /// An arithmetic expression tried to combine incompatible scale units.
    MismatchedUnits,
}

/// Parse one signed or relative roff scaled literal.
pub(crate) fn parse_scaled(input: &[u8]) -> Result<ScaledValue, NumericError> {
    let (value, consumed) = parse_prefix(input)?;
    (consumed == input.len())
        .then_some(value)
        .ok_or(NumericError::InvalidByte)
}

/// Evaluate a left-to-right sum of scaled literals joined by `+` or `-`.
///
/// Roff execution supplies register/string interpolation before this layer and
/// decides how a relative first term applies to request state. This evaluator
/// deliberately has no global device metrics or mutable state.
pub(crate) fn evaluate_sum(input: &[u8]) -> Result<ScaledValue, NumericError> {
    let (mut total, mut cursor) = parse_prefix(input)?;
    while cursor < input.len() {
        let operator = input[cursor];
        if !matches!(operator, b'+' | b'-') {
            return Err(NumericError::InvalidByte);
        }
        cursor += 1;
        let (mut next, consumed) = parse_prefix(&input[cursor..])?;
        if operator == b'-' {
            next.magnitude = next.magnitude.checked_neg().ok_or(NumericError::Overflow)?;
        }
        total = total.checked_add(next)?;
        cursor += consumed;
    }
    Ok(total)
}

/// One parsed `.nr` value, including the request-level relative sign.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RegisterExpression {
    /// Evaluated integer in mandoc's deterministic basic units.
    pub(crate) value: i64,
    /// Whether the `.nr` value began with an explicit request-level `+` or
    /// `-`; `None` means replacement rather than adjustment.
    pub(crate) relative: Option<i8>,
}

/// Evaluate the integer expression accepted by a roff `.nr` request.
///
/// mandoc evaluates this grammar strictly left-to-right; multiplication has
/// no higher precedence than addition.  A non-numeric suffix is left for the
/// request parser (for example a malformed increment), while a recognized
/// operator without a following operand invalidates the entire expression.
/// When `leading_relative` is set, the initial `+` or `-` is the `.nr`
/// request's adjustment sign rather than an operand sign.
pub(crate) fn evaluate_register_expression(
    input: &[u8],
    scale: bool,
    leading_relative: bool,
) -> Result<Option<RegisterExpression>, NumericError> {
    let mut cursor = 0;
    let relative = if leading_relative {
        match input.first().copied() {
            Some(b'+') => {
                cursor = 1;
                Some(1)
            }
            Some(b'-') => {
                cursor = 1;
                Some(-1)
            }
            _ => None,
        }
    } else {
        None
    };
    let Some(value) = evaluate_register_inner(input, &mut cursor, scale, false)? else {
        return Ok(None);
    };
    Ok(Some(RegisterExpression { value, relative }))
}

fn evaluate_register_inner(
    input: &[u8],
    cursor: &mut usize,
    scale: bool,
    whitespace: bool,
) -> Result<Option<i64>, NumericError> {
    skip_numeric_whitespace(input, cursor, whitespace);
    let Some(mut value) = evaluate_register_operand(input, cursor, scale, whitespace)? else {
        return Ok(None);
    };
    loop {
        skip_numeric_whitespace(input, cursor, whitespace);
        let Some(operator) = read_register_operator(input, cursor) else {
            return Ok(Some(value));
        };
        skip_numeric_whitespace(input, cursor, whitespace);
        let Some(operand) = evaluate_register_operand(input, cursor, scale, whitespace)? else {
            return Ok(None);
        };
        value = match operator {
            RegisterOperator::Add => value.checked_add(operand).ok_or(NumericError::Overflow)?,
            RegisterOperator::Subtract => {
                value.checked_sub(operand).ok_or(NumericError::Overflow)?
            }
            RegisterOperator::Multiply => {
                value.checked_mul(operand).ok_or(NumericError::Overflow)?
            }
            // mandoc reports a source diagnostic for division by zero, then
            // deterministically stores zero. The parser's typed-diagnostic
            // mapping is added with the complete M3 recovery taxonomy.
            RegisterOperator::Divide | RegisterOperator::Modulo if operand == 0 => 0,
            RegisterOperator::Divide => value / operand,
            RegisterOperator::Modulo => value % operand,
            RegisterOperator::Less => i64::from(value < operand),
            RegisterOperator::Greater => i64::from(value > operand),
            RegisterOperator::LessEqual => i64::from(value <= operand),
            RegisterOperator::GreaterEqual => i64::from(value >= operand),
            RegisterOperator::Equal => i64::from(value == operand),
            RegisterOperator::NotEqual => i64::from(value != operand),
            RegisterOperator::And => i64::from(value != 0 && operand != 0),
            RegisterOperator::Or => i64::from(value != 0 || operand != 0),
            RegisterOperator::Minimum => value.min(operand),
            RegisterOperator::Maximum => value.max(operand),
        };
    }
}

fn evaluate_register_operand(
    input: &[u8],
    cursor: &mut usize,
    scale: bool,
    whitespace: bool,
) -> Result<Option<i64>, NumericError> {
    if input.get(*cursor) == Some(&b'(') {
        *cursor += 1;
        let Some(value) = evaluate_register_inner(input, cursor, scale, true)? else {
            return Ok(None);
        };
        if input.get(*cursor) == Some(&b')') {
            *cursor += 1;
        }
        return Ok(Some(value));
    }
    parse_register_integer(input, cursor, scale, whitespace)
}

fn parse_register_integer(
    input: &[u8],
    cursor: &mut usize,
    scale: bool,
    whitespace: bool,
) -> Result<Option<i64>, NumericError> {
    let start = *cursor;
    let negative = match input.get(*cursor).copied() {
        Some(b'-') => {
            *cursor += 1;
            true
        }
        Some(b'+') => {
            *cursor += 1;
            false
        }
        _ => false,
    };
    skip_numeric_whitespace(input, cursor, whitespace);
    let digit_start = *cursor;
    let mut value = 0_i64;
    while let Some(byte) = input.get(*cursor).copied() {
        if !byte.is_ascii_digit() {
            break;
        }
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(i64::from(byte - b'0')))
            .ok_or(NumericError::Overflow)?;
        *cursor += 1;
    }
    if *cursor == digit_start {
        *cursor = start;
        return Ok(None);
    }
    if negative {
        value = value.checked_neg().ok_or(NumericError::Overflow)?;
    }
    if scale {
        value = match input.get(*cursor).copied() {
            Some(b'f') => {
                *cursor += 1;
                value.checked_mul(65_536).ok_or(NumericError::Overflow)?
            }
            Some(b'i') => {
                *cursor += 1;
                value.checked_mul(240).ok_or(NumericError::Overflow)?
            }
            Some(b'c') => {
                *cursor += 1;
                value.checked_mul(12_000).ok_or(NumericError::Overflow)? / 127
            }
            Some(b'v' | b'P') => {
                *cursor += 1;
                value.checked_mul(40).ok_or(NumericError::Overflow)?
            }
            Some(b'm' | b'n') => {
                *cursor += 1;
                value.checked_mul(24).ok_or(NumericError::Overflow)?
            }
            Some(b'p') => {
                *cursor += 1;
                value.checked_mul(10).ok_or(NumericError::Overflow)? / 3
            }
            Some(b'u') => {
                *cursor += 1;
                value
            }
            Some(b'M') => {
                *cursor += 1;
                value.checked_mul(6).ok_or(NumericError::Overflow)? / 25
            }
            _ => value,
        };
    }
    Ok(Some(value))
}

fn skip_numeric_whitespace(input: &[u8], cursor: &mut usize, enabled: bool) {
    if enabled {
        while input.get(*cursor).is_some_and(u8::is_ascii_whitespace) {
            *cursor += 1;
        }
    }
}

#[derive(Clone, Copy)]
enum RegisterOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Equal,
    NotEqual,
    And,
    Or,
    Minimum,
    Maximum,
}

fn read_register_operator(input: &[u8], cursor: &mut usize) -> Option<RegisterOperator> {
    let byte = *input.get(*cursor)?;
    let next = input.get(*cursor + 1).copied();
    let (operator, width) = match (byte, next) {
        (b'+', _) => (RegisterOperator::Add, 1),
        (b'-', _) => (RegisterOperator::Subtract, 1),
        (b'*', _) => (RegisterOperator::Multiply, 1),
        (b'/', _) => (RegisterOperator::Divide, 1),
        (b'%', _) => (RegisterOperator::Modulo, 1),
        (b'&', _) => (RegisterOperator::And, 1),
        (b':', _) => (RegisterOperator::Or, 1),
        (b'<', Some(b'=')) => (RegisterOperator::LessEqual, 2),
        (b'<', Some(b'>')) => (RegisterOperator::NotEqual, 2),
        (b'<', Some(b'?')) => (RegisterOperator::Minimum, 2),
        (b'<', _) => (RegisterOperator::Less, 1),
        (b'>', Some(b'=')) => (RegisterOperator::GreaterEqual, 2),
        (b'>', Some(b'?')) => (RegisterOperator::Maximum, 2),
        (b'>', _) => (RegisterOperator::Greater, 1),
        (b'=', Some(b'=')) => (RegisterOperator::Equal, 2),
        (b'=', _) => (RegisterOperator::Equal, 1),
        (b'!', _) => (RegisterOperator::NotEqual, 1),
        _ => return None,
    };
    *cursor += width;
    Some(operator)
}

fn parse_prefix(input: &[u8]) -> Result<(ScaledValue, usize), NumericError> {
    if input.is_empty() {
        return Err(NumericError::Empty);
    }
    let mut cursor = 0;
    let mut negative = false;
    let mut relative = false;
    if let Some(sign) = input.first().copied() {
        match sign {
            b'+' => {
                relative = true;
                cursor += 1;
            }
            b'-' => {
                negative = true;
                relative = true;
                cursor += 1;
            }
            _ => {}
        }
    }
    let integer_start = cursor;
    let mut integer = 0_i64;
    while let Some(byte) = input.get(cursor).copied() {
        if !byte.is_ascii_digit() {
            break;
        }
        integer = integer
            .checked_mul(10)
            .and_then(|value| value.checked_add(i64::from(byte - b'0')))
            .ok_or(NumericError::Overflow)?;
        cursor += 1;
    }
    let had_integer = cursor > integer_start;
    let mut fraction = 0_i64;
    let mut fraction_scale = 1_i64;
    if input.get(cursor) == Some(&b'.') {
        cursor += 1;
        let fraction_start = cursor;
        while let Some(byte) = input.get(cursor).copied() {
            if !byte.is_ascii_digit() {
                break;
            }
            if fraction_scale < SelfScale::LIMIT {
                fraction = fraction
                    .checked_mul(10)
                    .and_then(|value| value.checked_add(i64::from(byte - b'0')))
                    .ok_or(NumericError::Overflow)?;
                fraction_scale = fraction_scale
                    .checked_mul(10)
                    .ok_or(NumericError::Overflow)?;
            } else if byte != b'0' {
                return Err(NumericError::Precision);
            }
            cursor += 1;
        }
        if cursor == fraction_start && !had_integer {
            return Err(NumericError::MissingDigits);
        }
    } else if !had_integer {
        return Err(NumericError::MissingDigits);
    }
    let unit = input
        .get(cursor)
        .copied()
        .and_then(scale_unit)
        .map_or(ScaleUnit::Basic, |unit| {
            cursor += 1;
            unit
        });
    let magnitude = integer
        .checked_mul(ScaledValue::FRACTION_SCALE)
        .and_then(|value| {
            value.checked_add(
                fraction
                    .checked_mul(ScaledValue::FRACTION_SCALE / fraction_scale)
                    .expect("fraction scale divides one million"),
            )
        })
        .ok_or(NumericError::Overflow)?;
    Ok((
        ScaledValue {
            magnitude: if negative {
                magnitude.checked_neg().ok_or(NumericError::Overflow)?
            } else {
                magnitude
            },
            unit,
            relative,
        },
        cursor,
    ))
}

struct SelfScale;

impl SelfScale {
    const LIMIT: i64 = ScaledValue::FRACTION_SCALE;
}

fn scale_unit(byte: u8) -> Option<ScaleUnit> {
    match byte {
        b'u' => Some(ScaleUnit::Basic),
        b'n' => Some(ScaleUnit::En),
        b'm' => Some(ScaleUnit::Em),
        b'p' => Some(ScaleUnit::Point),
        b'P' => Some(ScaleUnit::Pica),
        b'i' => Some(ScaleUnit::Inch),
        b'c' => Some(ScaleUnit::Centimeter),
        b'M' => Some(ScaleUnit::Millimeter),
        b'v' => Some(ScaleUnit::Vertical),
        b'f' => Some(ScaleUnit::FontSize),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NumericError, ScaleUnit, ScaledValue, evaluate_register_expression, evaluate_sum,
        parse_scaled,
    };

    #[test]
    fn parses_exact_decimal_relative_scaled_literals() {
        assert_eq!(
            parse_scaled(b"+1.25i"),
            Ok(ScaledValue {
                magnitude: 1_250_000,
                unit: ScaleUnit::Inch,
                relative: true,
            })
        );
        assert_eq!(parse_scaled(b".5m").unwrap().magnitude, 500_000);
    }

    #[test]
    fn evaluates_compatible_sums_without_floating_point() {
        assert_eq!(
            evaluate_sum(b"1.25i+0.75i"),
            Ok(ScaledValue {
                magnitude: 2_000_000,
                unit: ScaleUnit::Inch,
                relative: false,
            })
        );
        assert_eq!(evaluate_sum(b"1i+1m"), Err(NumericError::MismatchedUnits));
        assert_eq!(evaluate_sum(b"1i-6P").unwrap().magnitude, 0);
    }

    #[test]
    fn compares_exact_physical_units_without_a_device_context() {
        assert_eq!(
            parse_scaled(b"1i")
                .unwrap()
                .compare(parse_scaled(b"2c").unwrap()),
            Some(std::cmp::Ordering::Greater)
        );
        assert_eq!(
            parse_scaled(b"1m")
                .unwrap()
                .compare(parse_scaled(b"1i").unwrap()),
            None
        );
    }

    #[test]
    fn rejects_nonrepresentable_or_malformed_literals() {
        assert_eq!(parse_scaled(b""), Err(NumericError::Empty));
        assert_eq!(parse_scaled(b"+"), Err(NumericError::MissingDigits));
        assert_eq!(parse_scaled(b"1.0000001i"), Err(NumericError::Precision));
        assert_eq!(parse_scaled(b"1x"), Err(NumericError::InvalidByte));
    }

    #[test]
    fn register_expressions_are_left_to_right_and_tolerate_request_suffixes() {
        assert_eq!(
            evaluate_register_expression(b"3+(3*(5==5*2)*4)+(3*5)/2", true, true),
            Ok(Some(super::RegisterExpression {
                value: 21,
                relative: None,
            }))
        );
        assert_eq!(
            evaluate_register_expression(b"2+3*3", true, true)
                .unwrap()
                .unwrap()
                .value,
            15
        );
        assert_eq!(
            evaluate_register_expression(b"1f+1", true, true)
                .unwrap()
                .unwrap()
                .value,
            65_537
        );
        assert_eq!(
            evaluate_register_expression(b"10c+1", true, true)
                .unwrap()
                .unwrap()
                .value,
            945
        );
        assert_eq!(evaluate_register_expression(b"4+", true, true), Ok(None));
        assert_eq!(
            evaluate_register_expression(b"2x", true, true)
                .unwrap()
                .unwrap()
                .value,
            2
        );
    }
}
