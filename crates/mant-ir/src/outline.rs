//! Typed addresses for nodes in a projected document outline.

use std::{fmt, num::NonZeroUsize, str::FromStr};

/// A stable, one-based path in a query outline.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutlinePath {
    /// The optional tldr quick-reference projection.
    Tldr,
    /// Content preceding the document's first section.
    DocumentRoot,
    /// A section addressed by one-based child indices.
    Section(Vec<NonZeroUsize>),
    /// A semantic entry within the document root or a section.
    Entry {
        /// One-based path of the containing section, or `None` for root content.
        section: Option<Vec<NonZeroUsize>>,
        /// One-based entry positions from the scope root to the nested entry.
        indices: Vec<NonZeroUsize>,
    },
}

impl OutlinePath {
    /// Construct a section path, returning `None` for empty or zero coordinates.
    #[must_use]
    pub fn section(coordinates: &[usize]) -> Option<Self> {
        Some(Self::Section(non_zero_coordinates(coordinates)?))
    }

    /// Construct an entry path, returning `None` for zero indices or coordinates.
    #[must_use]
    pub fn entry(section: Option<&[usize]>, index: usize) -> Option<Self> {
        Self::nested_entry(section, &[index])
    }

    /// Construct a possibly nested entry path.
    #[must_use]
    pub fn nested_entry(section: Option<&[usize]>, indices: &[usize]) -> Option<Self> {
        Some(Self::Entry {
            section: match section {
                Some(coordinates) => Some(non_zero_coordinates(coordinates)?),
                None => None,
            },
            indices: non_zero_coordinates(indices)?,
        })
    }

    /// Return whether this addresses an entry before the first section.
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
                indices,
            } => write_entry_coordinates(formatter, "root", indices),
            Self::Entry {
                section: Some(coordinates),
                indices,
            } => {
                write_coordinates(formatter, coordinates)?;
                write_entry_suffix(formatter, indices)
            }
        }
    }
}

fn write_entry_coordinates(
    formatter: &mut fmt::Formatter<'_>,
    root: &str,
    indices: &[NonZeroUsize],
) -> fmt::Result {
    formatter.write_str(root)?;
    write_entry_suffix(formatter, indices)
}

fn write_entry_suffix(formatter: &mut fmt::Formatter<'_>, indices: &[NonZeroUsize]) -> fmt::Result {
    for index in indices {
        write!(formatter, "/e{index}")?;
    }
    Ok(())
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
        if let Some(indices) = value.strip_prefix("root/") {
            return Ok(Self::Entry {
                section: None,
                indices: parse_entry_indices(indices)?,
            });
        }
        let (section, entry) = value
            .split_once('/')
            .map_or((value, None), |(section, entry)| (section, Some(entry)));
        let coordinates = parse_coordinates(section)?;
        match entry {
            Some(indices) => Ok(Self::Entry {
                section: Some(coordinates),
                indices: parse_entry_indices(indices)?,
            }),
            None => Ok(Self::Section(coordinates)),
        }
    }
}

fn parse_entry_indices(value: &str) -> Result<Vec<NonZeroUsize>, InvalidOutlinePath> {
    let indices = value
        .split('/')
        .map(|component| {
            component
                .strip_prefix('e')
                .ok_or(InvalidOutlinePath)
                .and_then(parse_index)
        })
        .collect::<Result<Vec<_>, _>>()?;
    (!indices.is_empty())
        .then_some(indices)
        .ok_or(InvalidOutlinePath)
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
        for value in [
            "0",
            "root",
            "root/e2",
            "root/e2/e1",
            "1",
            "2.3",
            "2.3/e4",
            "2.3/e4/e2",
        ] {
            assert_eq!(value.parse::<OutlinePath>().unwrap().to_string(), value);
        }
    }

    #[test]
    fn rejects_zero_empty_and_malformed_indices() {
        for value in ["", "00", "01", "1.0", "root/e0", "1/o", "1/e2/e0", "x"] {
            assert!(value.parse::<OutlinePath>().is_err(), "accepted {value}");
        }
    }

    #[test]
    fn constructors_refuse_invalid_coordinates() {
        assert_eq!(OutlinePath::section(&[2, 3]).unwrap().to_string(), "2.3");
        assert_eq!(
            OutlinePath::entry(Some(&[2, 3]), 4).unwrap().to_string(),
            "2.3/e4"
        );
        assert_eq!(
            OutlinePath::nested_entry(Some(&[2, 3]), &[4, 2])
                .unwrap()
                .to_string(),
            "2.3/e4/e2"
        );
        assert!(OutlinePath::section(&[]).is_none());
        assert!(OutlinePath::entry(Some(&[1, 0]), 2).is_none());
    }
}
