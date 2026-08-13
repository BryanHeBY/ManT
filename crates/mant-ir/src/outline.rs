//! Typed addresses for nodes in a projected document outline.

use std::{fmt, num::NonZeroUsize, str::FromStr};

/// A stable, one-based path in a query outline.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutlinePath {
    Tldr,
    DocumentRoot,
    Section(Vec<NonZeroUsize>),
    Entry {
        section: Option<Vec<NonZeroUsize>>,
        index: NonZeroUsize,
    },
}

impl OutlinePath {
    #[must_use]
    pub fn section(coordinates: &[usize]) -> Option<Self> {
        Some(Self::Section(non_zero_coordinates(coordinates)?))
    }

    #[must_use]
    pub fn entry(section: Option<&[usize]>, index: usize) -> Option<Self> {
        Some(Self::Entry {
            section: match section {
                Some(coordinates) => Some(non_zero_coordinates(coordinates)?),
                None => None,
            },
            index: NonZeroUsize::new(index)?,
        })
    }

    #[must_use]
    pub const fn is_document_root_entry(&self) -> bool {
        matches!(self, Self::Entry { section: None, .. })
    }
}

fn non_zero_coordinates(coordinates: &[usize]) -> Option<Vec<NonZeroUsize>> {
    (!coordinates.is_empty())
        .then(|| {
            coordinates
                .iter()
                .copied()
                .map(NonZeroUsize::new)
                .collect::<Option<Vec<_>>>()
        })
        .flatten()
}

impl fmt::Display for OutlinePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tldr => formatter.write_str("0"),
            Self::DocumentRoot => formatter.write_str("root"),
            Self::Section(coordinates) => write_coordinates(formatter, coordinates),
            Self::Entry {
                section: None,
                index,
            } => write!(formatter, "root/o{index}"),
            Self::Entry {
                section: Some(coordinates),
                index,
            } => {
                write_coordinates(formatter, coordinates)?;
                write!(formatter, "/o{index}")
            }
        }
    }
}

fn write_coordinates(
    formatter: &mut fmt::Formatter<'_>,
    coordinates: &[NonZeroUsize],
) -> fmt::Result {
    for (position, coordinate) in coordinates.iter().enumerate() {
        if position > 0 {
            formatter.write_str(".")?;
        }
        write!(formatter, "{coordinate}")?;
    }
    Ok(())
}

/// A string that is not a valid outline path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidOutlinePath;

impl fmt::Display for InvalidOutlinePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid outline path")
    }
}

impl std::error::Error for InvalidOutlinePath {}

impl FromStr for OutlinePath {
    type Err = InvalidOutlinePath;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "0" => return Ok(Self::Tldr),
            "root" => return Ok(Self::DocumentRoot),
            _ => {}
        }
        if let Some(index) = value.strip_prefix("root/o") {
            return Ok(Self::Entry {
                section: None,
                index: parse_index(index)?,
            });
        }
        let (section, entry) = value
            .split_once("/o")
            .map_or((value, None), |(section, entry)| (section, Some(entry)));
        let coordinates = parse_coordinates(section)?;
        match entry {
            Some(index) => Ok(Self::Entry {
                section: Some(coordinates),
                index: parse_index(index)?,
            }),
            None => Ok(Self::Section(coordinates)),
        }
    }
}

fn parse_coordinates(value: &str) -> Result<Vec<NonZeroUsize>, InvalidOutlinePath> {
    if value.is_empty() {
        return Err(InvalidOutlinePath);
    }
    value.split('.').map(parse_index).collect()
}

fn parse_index(value: &str) -> Result<NonZeroUsize, InvalidOutlinePath> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(InvalidOutlinePath);
    }
    value
        .parse::<usize>()
        .ok()
        .and_then(NonZeroUsize::new)
        .ok_or(InvalidOutlinePath)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_outline_path_family() {
        for value in ["0", "root", "root/o2", "1", "2.3", "2.3/o4"] {
            assert_eq!(value.parse::<OutlinePath>().unwrap().to_string(), value);
        }
    }

    #[test]
    fn rejects_zero_empty_and_malformed_indices() {
        for value in ["", "00", "01", "1.0", "root/o0", "1/o", "1/o2/o3", "x"] {
            assert!(value.parse::<OutlinePath>().is_err(), "accepted {value}");
        }
    }

    #[test]
    fn constructors_refuse_invalid_coordinates() {
        assert_eq!(OutlinePath::section(&[2, 3]).unwrap().to_string(), "2.3");
        assert_eq!(
            OutlinePath::entry(Some(&[2, 3]), 4).unwrap().to_string(),
            "2.3/o4"
        );
        assert!(OutlinePath::section(&[]).is_none());
        assert!(OutlinePath::entry(Some(&[1, 0]), 2).is_none());
    }
}
