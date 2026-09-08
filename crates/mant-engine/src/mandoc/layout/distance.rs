//! Bounded source-device distances, before conversion to terminal cells.
//!
//! mandoc's character device uses 24 basic units per cell. Accumulating in
//! these units matters: three nested 0.4n offsets are not three zero offsets.
//! Source expressions and formatter registers deliberately do not enter IR.

const UNITS_PER_CELL: i32 = 24;
const LIMIT: i32 = 4096 * UNITS_PER_CELL;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Distance(i32);

impl Distance {
    /// Parse one finite literal distance, using ens for a bare number.
    /// Expressions, registers and unsupported units remain explicit failures.
    pub(super) fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let end = value
            .find(|c: char| c.is_ascii_alphabetic())
            .unwrap_or(value.len());
        let scale = value[..end].parse::<f64>().ok()?;
        let ratio = match value[end..].trim() {
            "" | "n" | "m" => 24.0,
            "u" => 1.0,
            "c" => 240.0 / 2.54,
            "f" => 65_536.0,
            "i" => 240.0,
            "M" => 0.24,
            "P" | "v" => 40.0,
            "p" => 10.0 / 3.0,
            _ => return None,
        };
        let units = scale * ratio;
        if !units.is_finite() || units.abs() > f64::from(LIMIT) {
            return None;
        }
        // The source character device truncates basic units, not cells. Its
        // tiny bias protects exact physical-unit conversions from FP noise.
        #[allow(clippy::cast_possible_truncation)]
        let units = (units + units.signum() * 0.01) as i32;
        Some(Self(units))
    }

    pub(super) fn cells(value: i32) -> Self {
        Self(value.saturating_mul(UNITS_PER_CELL).clamp(-LIMIT, LIMIT))
    }

    /// Compose source positions without premature cell rounding. The bool
    /// reports bounding so the producer can issue its source diagnostic.
    pub(super) fn add(self, other: Self) -> (Self, bool) {
        let sum = self.0.saturating_add(other.0);
        (Self(sum.clamp(-LIMIT, LIMIT)), sum.abs() > LIMIT)
    }

    pub(super) fn columns(self) -> i32 {
        if self.0 < 0 {
            -((-self.0 + UNITS_PER_CELL / 2) / UNITS_PER_CELL)
        } else {
            (self.0 + UNITS_PER_CELL / 2) / UNITS_PER_CELL
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Distance;

    #[test]
    fn fractional_offsets_accumulate_before_cell_rounding() {
        let part = Distance::parse("0.4n").unwrap();
        let second = part.add(part).0;
        let third = second.add(part).0;
        assert_eq!(
            [part.columns(), second.columns(), third.columns()],
            [0, 1, 1]
        );
        assert_eq!(Distance::parse("-2n").unwrap().columns(), -2);
        assert_eq!(Distance::parse("+2n").unwrap().columns(), 2);
    }

    #[test]
    fn literal_units_and_bounds_have_one_conversion() {
        for value in ["1n", "1m", "24u", "0.1i", "0.254c", "100M", "7.2p"] {
            assert_eq!(Distance::parse(value).unwrap().columns(), 1, "{value}");
        }
        for value in ["1+2", "NaN", "inf", "1e8", "2q", "65535n", ""] {
            assert!(Distance::parse(value).is_none(), "{value}");
        }
        assert_eq!(
            Distance::cells(4095).add(Distance::cells(2)),
            (Distance::cells(4096), true)
        );
    }
}
