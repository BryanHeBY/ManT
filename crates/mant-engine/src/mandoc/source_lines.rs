//! Indexed access to physical roff source lines.
//!
//! The native parser exposes one-based source coordinates. Lowering occasionally
//! needs to recover syntax that the native AST intentionally flattens, so all
//! coordinate conversion and source-line access belongs here.  Building the
//! index once avoids repeatedly walking `str::lines()` from the beginning for
//! every AST node.

#[derive(Debug)]
pub(super) struct SourceLineIndex<'a> {
    source: &'a str,
    starts: Vec<u32>,
}

impl<'a> SourceLineIndex<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        let mut starts = Vec::new();
        if !source.is_empty() {
            starts.push(0);
        }
        for start in source
            .match_indices('\n')
            .map(|(index, _)| index.saturating_add(1))
            .filter(|start| *start < source.len())
        {
            let Ok(start) = u32::try_from(start) else {
                break;
            };
            starts.push(start);
        }
        Self { source, starts }
    }

    /// Return one physical source line addressed by a one-based coordinate.
    pub(super) fn line(&self, line: u32) -> Option<&'a str> {
        let index = usize::try_from(line).ok()?.checked_sub(1)?;
        self.line_at_index(index)
    }

    /// Iterate physical source lines starting at a one-based coordinate.
    pub(super) fn lines_from(&self, first_line: u32) -> impl Iterator<Item = (u32, &'a str)> + '_ {
        let first = usize::try_from(first_line)
            .ok()
            .and_then(|line| line.checked_sub(1))
            .unwrap_or(self.starts.len());
        self.starts
            .get(first..)
            .unwrap_or_default()
            .iter()
            .enumerate()
            .filter_map(move |(offset, _)| {
                let index = first.saturating_add(offset);
                let line_number = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
                self.line_at_index(index).map(|line| (line_number, line))
            })
    }

    /// Return the physical lines strictly between two one-based coordinates.
    pub(super) fn lines_between(
        &self,
        previous_line: u32,
        current_line: u32,
    ) -> impl Iterator<Item = &'a str> + '_ {
        let (start, end) = if current_line <= previous_line.saturating_add(1) {
            (0, 0)
        } else {
            let start = usize::try_from(previous_line)
                .unwrap_or(self.starts.len())
                .min(self.starts.len());
            let end = usize::try_from(current_line.saturating_sub(1))
                .unwrap_or(self.starts.len())
                .min(self.starts.len());
            if start < end { (start, end) } else { (0, 0) }
        };
        self.starts[start..end]
            .iter()
            .enumerate()
            .filter_map(move |(offset, _)| self.line_at_index(start.saturating_add(offset)))
    }

    fn line_at_index(&self, index: usize) -> Option<&'a str> {
        let start = usize::try_from(*self.starts.get(index)?).ok()?;
        let end = self
            .starts
            .get(index.saturating_add(1))
            .and_then(|end| usize::try_from(*end).ok())
            .unwrap_or(self.source.len());
        let line = self.source.get(start..end)?;
        let line = line.strip_suffix('\n').unwrap_or(line);
        Some(line.strip_suffix('\r').unwrap_or(line))
    }
}

#[cfg(test)]
mod tests {
    use super::SourceLineIndex;

    #[test]
    fn converts_one_based_coordinates_once_at_the_boundary() {
        let index = SourceLineIndex::new("first\nsecond\nthird\nfourth\n");

        assert_eq!(index.line(0), None);
        assert_eq!(index.line(1), Some("first"));
        assert_eq!(index.line(4), Some("fourth"));
        assert_eq!(index.line(5), None);
        assert_eq!(
            index.lines_between(1, 4).collect::<Vec<_>>(),
            vec!["second", "third"]
        );
        assert_eq!(index.lines_between(2, 3).count(), 0);
        assert_eq!(
            index.lines_from(3).collect::<Vec<_>>(),
            vec![(3, "third"), (4, "fourth")]
        );
    }

    #[test]
    fn matches_rust_line_ending_and_trailing_newline_semantics() {
        let index = SourceLineIndex::new("first\r\nsecond\n\n");

        assert_eq!(index.line(1), Some("first"));
        assert_eq!(index.line(2), Some("second"));
        assert_eq!(index.line(3), Some(""));
        assert_eq!(index.line(4), None);
    }
}
