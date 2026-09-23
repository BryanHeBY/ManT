//! Borrowed section navigation over one validated Fixed display body.
//!
//! The native heading parent is the only hierarchy authority. This reader
//! stores child keys, not copied text or ancestor subtree selections; a read
//! borrows final run slices from the one display surface and merges them in
//! final display order.

use std::{
    fmt,
    num::{NonZeroU32, NonZeroUsize},
};

use crate::{
    DisplayPoint, FixedBody, FixedBodyError, HeadingMark, OutlinePath, OutputSlice, TextJoin,
    TextSelection,
};

/// Failure to construct or read a checked Fixed section view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixedSectionReadError {
    /// The underlying display/mark references were malformed.
    InvalidBody(FixedBodyError),
    /// Two structural heading selections claim the same visible byte.
    OverlappingSelection,
    /// A structural boundary cannot be resolved to a UTF-8 byte boundary.
    AmbiguousBoundary,
}

impl fmt::Display for FixedSectionReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBody(error) => write!(formatter, "{error}"),
            Self::OverlappingSelection => {
                formatter.write_str("fixed sections claim overlapping display bytes")
            }
            Self::AmbiguousBoundary => formatter.write_str("fixed section boundary is ambiguous"),
        }
    }
}

impl std::error::Error for FixedSectionReadError {}

/// Whether a borrowed slice came from a heading title or its direct body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedSectionPartKind {
    /// Visible title bytes.
    Title,
    /// Visible body bytes directly owned by this section, excluding descendants.
    DirectBody,
    /// Content preceding the first native section.
    RootPreface,
}

/// A borrowed, final-display slice; text is never copied into the reader.
#[derive(Debug, Clone, Copy)]
pub struct FixedSectionPart<'a> {
    /// Native section identity, or `None` for root preface.
    pub section: Option<NonZeroU32>,
    /// Structural role of this slice.
    pub kind: FixedSectionPartKind,
    /// Final run-relative byte range.
    pub slice: OutputSlice,
    /// UTF-8 bytes borrowed from the sole display arena.
    pub text: &'a str,
    /// Final physical row, not a section-relative row.
    pub row: NonZeroU32,
    /// Run's native starting terminal column; `slice` is a byte range and
    /// must not be added to this column as if bytes were cells.
    pub run_column: u32,
}

/// A checked borrowed view of a Fixed section hierarchy.
///
/// Its sidecar is linear in the number of headings. It keeps neither a body
/// string nor per-ancestor subtree selections.
pub struct FixedSectionReader<'a> {
    fixed: &'a FixedBody,
    // Slot 0 is the document root; slot key.get() contains that heading's
    // immediate children. Dense native keys make a separate map unnecessary.
    children: Vec<Vec<NonZeroU32>>,
    sibling_ordinals: Vec<NonZeroUsize>,
}

impl<'a> FixedSectionReader<'a> {
    /// Validate the Fixed body and index only its native parent edges.
    ///
    /// # Errors
    /// Returns malformed body references or overlapping section bytes.
    pub fn new(fixed: &'a FixedBody) -> Result<Self, FixedSectionReadError> {
        fixed
            .validate()
            .map_err(FixedSectionReadError::InvalidBody)?;
        let mut children = vec![Vec::new(); fixed.headings.len() + 1];
        let mut sibling_ordinals = Vec::with_capacity(fixed.headings.len());
        for heading in &fixed.headings {
            let parent = heading.parent.map_or(0, |key| key.get() as usize);
            let ordinal = children[parent]
                .len()
                .checked_add(1)
                .and_then(NonZeroUsize::new)
                .ok_or(FixedSectionReadError::AmbiguousBoundary)?;
            children[parent].push(heading.key);
            sibling_ordinals.push(ordinal);
        }
        let reader = Self {
            fixed,
            children,
            sibling_ordinals,
        };
        // FixedBody validates individual selections, not exclusivity among
        // different sections. A duplicate selected byte would double-render
        // subtree content and must be rejected at this shared read boundary.
        let mut parts = reader
            .all_section_parts()
            .ok_or(FixedSectionReadError::AmbiguousBoundary)?;
        parts.extend(reader.root_preface_parts()?);
        sort_parts(&mut parts);
        ensure_disjoint(&parts)?;
        Ok(reader)
    }

    /// The one final native display surface underlying all views.
    #[must_use]
    pub const fn fixed(&self) -> &'a FixedBody {
        self.fixed
    }

    /// Top-level native headings in identity/source order.
    #[must_use]
    pub fn roots(&self) -> &[NonZeroU32] {
        &self.children[0]
    }

    /// Immediate children of a section; an invalid key has no view.
    #[must_use]
    pub fn children(&self, section: NonZeroU32) -> Option<&[NonZeroU32]> {
        self.children.get(section.get() as usize).map(Vec::as_slice)
    }

    /// The native heading for one validated key.
    #[must_use]
    pub fn heading(&self, section: NonZeroU32) -> Option<&'a HeadingMark> {
        self.fixed.headings.get((section.get() - 1) as usize)
    }

    /// The native final-display section start. Empty headings keep this
    /// location even when both title and direct body have no selected bytes.
    #[must_use]
    pub fn position(&self, section: NonZeroU32) -> Option<DisplayPoint> {
        Some(self.heading(section)?.at)
    }

    /// Derive an outline path from native parent edges, not rendered depth.
    #[must_use]
    pub fn path(&self, section: NonZeroU32) -> Option<OutlinePath> {
        let mut current = self.heading(section)?;
        let mut coordinates = Vec::new();
        loop {
            coordinates.push(self.sibling_ordinals[(current.key.get() - 1) as usize]);
            let Some(parent) = current.parent else { break };
            current = self.heading(parent)?;
        }
        coordinates.reverse();
        Some(OutlinePath::Section(coordinates))
    }

    /// Resolve an exact one-based section path, preserving duplicate titles.
    #[must_use]
    pub fn section_at(&self, path: &OutlinePath) -> Option<&'a HeadingMark> {
        let OutlinePath::Section(coordinates) = path else {
            return None;
        };
        let mut parent = 0;
        for coordinate in coordinates {
            parent = self.children.get(parent)?.get(coordinate.get() - 1)?.get() as usize;
        }
        self.fixed.headings.get(parent.checked_sub(1)?)
    }

    /// Borrow the native ancestor chain from the top-level heading through
    /// this section. Consumers derive breadcrumb labels on demand from the
    /// visible title, without caching another title or inferred depth.
    #[must_use]
    pub fn breadcrumbs(&self, section: NonZeroU32) -> Option<Vec<&'a HeadingMark>> {
        let mut current = self.heading(section)?;
        let mut chain = vec![current];
        while let Some(parent) = current.parent {
            current = self.heading(parent)?;
            chain.push(current);
        }
        chain.reverse();
        Some(chain)
    }

    /// Borrow exactly this section's title and direct body, excluding children.
    /// Each returned part retains its native final row, column and byte range.
    #[must_use]
    pub fn direct_parts(&self, section: NonZeroU32) -> Option<Vec<FixedSectionPart<'a>>> {
        let heading = self.heading(section)?;
        let mut parts =
            Vec::with_capacity(heading.title.parts.len() + heading.direct_body.parts.len());
        self.extend_parts(
            &mut parts,
            section,
            FixedSectionPartKind::Title,
            &heading.title,
        )?;
        self.extend_parts(
            &mut parts,
            section,
            FixedSectionPartKind::DirectBody,
            &heading.direct_body,
        )?;
        sort_parts(&mut parts);
        Some(parts)
    }

    /// Borrow this section's title, direct body and all descendants once in
    /// final display order. A child's text is never copied into its ancestors.
    #[must_use]
    pub fn subtree_parts(&self, section: NonZeroU32) -> Option<Vec<FixedSectionPart<'a>>> {
        self.heading(section)?;
        let mut result = Vec::new();
        let mut pending = vec![section];
        while let Some(key) = pending.pop() {
            let heading = self.heading(key)?;
            self.extend_parts(
                &mut result,
                key,
                FixedSectionPartKind::Title,
                &heading.title,
            )?;
            self.extend_parts(
                &mut result,
                key,
                FixedSectionPartKind::DirectBody,
                &heading.direct_body,
            )?;
            pending.extend(self.children[key.get() as usize].iter().rev());
        }
        sort_parts(&mut result);
        Some(result)
    }

    /// Borrow every final visible run byte before the first section start.
    /// For a document without sections, this is the entire surface. Layout
    /// spaces emitted before the first native section point remain root
    /// preface bytes; consumers may choose not to search layout-role runs.
    /// Blank physical rows have no text slice and remain in `fixed().surface.rows`.
    /// A point inside a run must be a `RunBoundary`: terminal columns cannot
    /// safely be converted to UTF-8 byte offsets after overprint folding.
    ///
    /// # Errors
    /// Returns an ambiguous row/column boundary that bisects a final run.
    pub fn root_preface_parts(&self) -> Result<Vec<FixedSectionPart<'a>>, FixedSectionReadError> {
        let mut boundary = self.fixed.surface.text.len();
        for heading in &self.fixed.headings {
            boundary = boundary.min(self.byte_offset(heading.at)?);
        }
        let mut output = Vec::new();
        for run in &self.fixed.surface.runs {
            let start = usize::try_from(run.byte_start)
                .map_err(|_| FixedSectionReadError::AmbiguousBoundary)?;
            if start >= boundary {
                break;
            }
            let count = usize::try_from(run.byte_count)
                .map_err(|_| FixedSectionReadError::AmbiguousBoundary)?;
            let end = start
                .checked_add(count)
                .ok_or(FixedSectionReadError::AmbiguousBoundary)?
                .min(boundary);
            let text = self
                .fixed
                .surface
                .text
                .get(start..end)
                .ok_or(FixedSectionReadError::AmbiguousBoundary)?;
            output.push(FixedSectionPart {
                section: None,
                kind: FixedSectionPartKind::RootPreface,
                slice: OutputSlice {
                    run: run.key,
                    start_byte: 0,
                    end_byte: u64::try_from(end - start)
                        .map_err(|_| FixedSectionReadError::AmbiguousBoundary)?,
                },
                text,
                row: run.row,
                run_column: run.column,
            });
        }
        Ok(output)
    }

    /// Derive a safe one-line navigation label from the visible native title.
    /// This is an on-demand presentation string, not another stored body.
    #[must_use]
    pub fn label(&self, section: NonZeroU32) -> Option<String> {
        let title = &self.heading(section)?.title;
        let mut text = String::new();
        for (index, part) in title.parts.iter().enumerate() {
            if let Some(join) = index
                .checked_sub(1)
                .and_then(|index| title.joins.get(index))
            {
                match join {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator) => text.push_str(separator),
                    TextJoin::HardBoundary | TextJoin::Unknown => text.push(' '),
                }
            }
            let run = self.fixed.surface.run_text(part.run)?;
            let fragment = run.get(
                usize::try_from(part.start_byte).ok()?..usize::try_from(part.end_byte).ok()?,
            )?;
            text.push_str(fragment);
        }
        let mut label = String::new();
        for word in text.split_whitespace() {
            if !label.is_empty() {
                label.push(' ');
            }
            label.push_str(word);
        }
        Some(label)
    }

    fn extend_parts(
        &self,
        output: &mut Vec<FixedSectionPart<'a>>,
        section: NonZeroU32,
        kind: FixedSectionPartKind,
        selection: &TextSelection,
    ) -> Option<()> {
        for slice in &selection.parts {
            let run = self
                .fixed
                .surface
                .runs
                .get((slice.run.get() - 1) as usize)?;
            let run_text = self.fixed.surface.run_text(slice.run)?;
            let text = run_text.get(
                usize::try_from(slice.start_byte).ok()?..usize::try_from(slice.end_byte).ok()?,
            )?;
            output.push(FixedSectionPart {
                section: Some(section),
                kind,
                slice: *slice,
                text,
                row: run.row,
                run_column: run.column,
            });
        }
        Some(())
    }

    fn all_section_parts(&self) -> Option<Vec<FixedSectionPart<'a>>> {
        let mut result = Vec::new();
        for heading in &self.fixed.headings {
            self.extend_parts(
                &mut result,
                heading.key,
                FixedSectionPartKind::Title,
                &heading.title,
            )?;
            self.extend_parts(
                &mut result,
                heading.key,
                FixedSectionPartKind::DirectBody,
                &heading.direct_body,
            )?;
        }
        Some(result)
    }

    fn byte_offset(&self, point: DisplayPoint) -> Result<usize, FixedSectionReadError> {
        match point {
            DisplayPoint::RunBoundary { run, byte } => {
                let run = &self.fixed.surface.runs[(run.get() - 1) as usize];
                let offset = run
                    .byte_start
                    .checked_add(byte)
                    .ok_or(FixedSectionReadError::AmbiguousBoundary)?;
                usize::try_from(offset).map_err(|_| FixedSectionReadError::AmbiguousBoundary)
            }
            DisplayPoint::DocumentEnd { .. } => Ok(self.fixed.surface.text.len()),
            DisplayPoint::RowColumn { row, column } => {
                // `DisplaySurface::validate()` makes each row's runs a
                // contiguous, nonoverlapping, column-sorted interval. Search
                // only that interval: many empty headings must not rescan
                // every run when deriving the root preface boundary.
                let display_row = &self.fixed.surface.rows[(row.get() - 1) as usize];
                let first = (display_row.first_run.get() - 1) as usize;
                let end = first + display_row.run_count as usize;
                let row_runs = &self.fixed.surface.runs[first..end];
                let next = row_runs.partition_point(|run| {
                    run.column
                        .checked_add(run.width)
                        .is_some_and(|end_column| end_column <= column)
                });
                if let Some(run) = row_runs.get(next) {
                    let end_column = run
                        .column
                        .checked_add(run.width)
                        .ok_or(FixedSectionReadError::AmbiguousBoundary)?;
                    if run.column < column && column < end_column {
                        return Err(FixedSectionReadError::AmbiguousBoundary);
                    }
                    return usize::try_from(run.byte_start)
                        .map_err(|_| FixedSectionReadError::AmbiguousBoundary);
                }
                self.fixed.surface.runs.get(end).map_or(
                    Ok(self.fixed.surface.text.len()),
                    |next_run| {
                        usize::try_from(next_run.byte_start)
                            .map_err(|_| FixedSectionReadError::AmbiguousBoundary)
                    },
                )
            }
        }
    }
}

fn sort_parts(parts: &mut [FixedSectionPart<'_>]) {
    parts.sort_unstable_by_key(|part| (part.slice.run, part.slice.start_byte, part.slice.end_byte));
}

fn ensure_disjoint(parts: &[FixedSectionPart<'_>]) -> Result<(), FixedSectionReadError> {
    for pair in parts.windows(2) {
        let [previous, next] = pair else {
            unreachable!()
        };
        if previous.slice.run == next.slice.run && previous.slice.end_byte > next.slice.start_byte {
            return Err(FixedSectionReadError::OverlappingSelection);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DisplayLabel, DisplayRole, DisplayRow, DisplayRun, DisplayStyle, DisplaySurface, NodeId,
    };

    fn key(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap()
    }

    fn selection(ranges: &[(u64, u64)]) -> TextSelection {
        TextSelection {
            parts: ranges
                .iter()
                .map(|&(start_byte, end_byte)| OutputSlice {
                    run: key(1),
                    start_byte,
                    end_byte,
                })
                .collect(),
            joins: vec![TextJoin::HardBoundary; ranges.len().saturating_sub(1)],
        }
    }

    fn heading(
        key_value: u32,
        parent: Option<u32>,
        at: u64,
        title: &[(u64, u64)],
        body: &[(u64, u64)],
    ) -> HeadingMark {
        HeadingMark {
            key: key(key_value),
            id: NodeId::from(format!("section-{key_value}")),
            fragment_aliases: Vec::new(),
            generated_fragment_aliases: Vec::new(),
            rendered_fragment_aliases: Vec::new(),
            parent: parent.map(key),
            level_hint: if parent.is_some() { 2 } else { 1 },
            at: DisplayPoint::RunBoundary {
                run: key(1),
                byte: at,
            },
            title: selection(title),
            direct_body: selection(body),
            source: None,
        }
    }

    fn sample() -> FixedBody {
        // Synthetic IR: root preface, a parent body split around its child,
        // repeated visible title, and a structurally empty trailing section.
        let text = "preSbodyCchildtailS";
        FixedBody {
            surface: DisplaySurface {
                text: text.to_owned(),
                rows: vec![DisplayRow {
                    key: key(1),
                    first_run: key(1),
                    run_count: 1,
                    column_count: u32::try_from(text.len()).unwrap(),
                    break_after: false,
                }],
                runs: vec![DisplayRun {
                    key: key(1),
                    row: key(1),
                    column: 0,
                    width: u32::try_from(text.len()).unwrap(),
                    byte_start: 0,
                    byte_count: text.len() as u64,
                    label: DisplayLabel {
                        owner: None,
                        link: None,
                        source: None,
                        style: DisplayStyle {
                            bold: false,
                            underline: false,
                        },
                        role: DisplayRole::Body,
                    },
                }],
            },
            headings: vec![
                heading(1, None, 3, &[(3, 4)], &[(4, 8), (14, 18)]),
                heading(2, Some(1), 8, &[(8, 9)], &[(9, 14)]),
                heading(3, None, 18, &[(18, 19)], &[]),
                heading(4, None, 19, &[], &[]),
            ],
            owners: Vec::new(),
            links: Vec::new(),
            anchors: Vec::new(),
            regions: Vec::new(),
        }
    }

    #[test]
    fn derives_parent_paths_without_making_depth_into_layout() {
        let fixed = sample();
        let reader = FixedSectionReader::new(&fixed).unwrap();
        assert_eq!(reader.roots(), &[key(1), key(3), key(4)]);
        assert_eq!(reader.children(key(1)), Some(&[key(2)][..]));
        assert_eq!(reader.path(key(1)).unwrap().to_string(), "1");
        assert_eq!(reader.path(key(2)).unwrap().to_string(), "1.1");
        assert_eq!(reader.path(key(3)).unwrap().to_string(), "2");
        assert_eq!(reader.path(key(4)).unwrap().to_string(), "3");
        assert_eq!(
            reader
                .section_at(&OutlinePath::section(&[1, 1]).unwrap())
                .unwrap()
                .key,
            key(2)
        );
        assert_eq!(reader.label(key(1)).as_deref(), Some("S"));
        assert_eq!(reader.label(key(3)).as_deref(), Some("S"));
        assert_eq!(reader.label(key(4)).as_deref(), Some(""));
        assert_eq!(
            reader
                .breadcrumbs(key(2))
                .unwrap()
                .iter()
                .map(|heading| heading.key)
                .collect::<Vec<_>>(),
            vec![key(1), key(2)]
        );
        assert_eq!(
            reader.position(key(4)),
            Some(DisplayPoint::RunBoundary {
                run: key(1),
                byte: 19
            })
        );
    }

    #[test]
    fn direct_and_subtree_reads_keep_native_order_and_no_duplicate_bytes() {
        let fixed = sample();
        let reader = FixedSectionReader::new(&fixed).unwrap();
        let root = reader.root_preface_parts().unwrap();
        assert_eq!(root.iter().map(|part| part.text).collect::<String>(), "pre");
        let direct = reader.direct_parts(key(1)).unwrap();
        assert_eq!(
            direct.iter().map(|part| part.text).collect::<String>(),
            "Sbodytail"
        );
        let subtree = reader.subtree_parts(key(1)).unwrap();
        assert_eq!(
            subtree.iter().map(|part| part.text).collect::<String>(),
            "SbodyCchildtail"
        );
        assert_eq!(
            subtree.iter().map(|part| part.row).collect::<Vec<_>>(),
            vec![key(1); 5]
        );
        assert!(reader.subtree_parts(key(4)).unwrap().is_empty());
    }

    #[test]
    fn subtree_read_retains_physical_rows_and_columns_across_runs() {
        let mut fixed = sample();
        fixed.surface.rows = vec![
            DisplayRow {
                key: key(1),
                first_run: key(1),
                run_count: 1,
                column_count: 8,
                break_after: true,
            },
            DisplayRow {
                key: key(2),
                first_run: key(2),
                run_count: 1,
                column_count: 15,
                break_after: false,
            },
        ];
        fixed.surface.runs[0].byte_count = 8;
        fixed.surface.runs[0].width = 8;
        let label = fixed.surface.runs[0].label;
        fixed.surface.runs.push(DisplayRun {
            key: key(2),
            row: key(2),
            column: 4,
            width: 11,
            byte_start: 8,
            byte_count: 11,
            label,
        });
        fixed.headings[0].direct_body.parts[1] = OutputSlice {
            run: key(2),
            start_byte: 6,
            end_byte: 10,
        };
        fixed.headings[1].at = DisplayPoint::RunBoundary {
            run: key(2),
            byte: 0,
        };
        fixed.headings[1].title.parts[0] = OutputSlice {
            run: key(2),
            start_byte: 0,
            end_byte: 1,
        };
        fixed.headings[1].direct_body.parts[0] = OutputSlice {
            run: key(2),
            start_byte: 1,
            end_byte: 6,
        };
        fixed.headings[2].at = DisplayPoint::RunBoundary {
            run: key(2),
            byte: 10,
        };
        fixed.headings[2].title.parts[0] = OutputSlice {
            run: key(2),
            start_byte: 10,
            end_byte: 11,
        };
        fixed.headings[3].at = DisplayPoint::RowColumn {
            row: key(2),
            column: 15,
        };

        let reader = FixedSectionReader::new(&fixed).unwrap();
        assert_eq!(reader.root_preface_parts().unwrap()[0].text, "pre");
        assert_eq!(
            reader
                .byte_offset(DisplayPoint::RowColumn {
                    row: key(2),
                    column: 2
                })
                .unwrap(),
            8
        );
        assert_eq!(
            reader.byte_offset(DisplayPoint::RowColumn {
                row: key(2),
                column: 8
            }),
            Err(FixedSectionReadError::AmbiguousBoundary)
        );
        let subtree = reader.subtree_parts(key(1)).unwrap();
        assert_eq!(
            subtree.iter().map(|part| part.text).collect::<String>(),
            "SbodyCchildtail"
        );
        assert_eq!(
            subtree.iter().map(|part| part.row).collect::<Vec<_>>(),
            vec![key(1), key(1), key(2), key(2), key(2)]
        );
        assert_eq!(
            subtree
                .iter()
                .map(|part| part.run_column)
                .collect::<Vec<_>>(),
            vec![0, 0, 4, 4, 4]
        );
    }

    #[test]
    fn root_without_sections_borrows_entire_surface() {
        let mut fixed = sample();
        fixed.headings.clear();
        let reader = FixedSectionReader::new(&fixed).unwrap();
        assert_eq!(
            reader.root_preface_parts().unwrap()[0].text,
            fixed.surface.text
        );
    }

    #[test]
    fn rejects_duplicate_section_bytes_and_ambiguous_column_boundary() {
        let mut fixed = sample();
        fixed.headings[2].title = selection(&[(3, 4)]);
        fixed.headings[2].at = DisplayPoint::RunBoundary {
            run: key(1),
            byte: 3,
        };
        assert_eq!(
            FixedSectionReader::new(&fixed).err(),
            Some(FixedSectionReadError::OverlappingSelection)
        );

        let mut fixed = sample();
        fixed.headings[0].at = DisplayPoint::RowColumn {
            row: key(1),
            column: 3,
        };
        assert_eq!(
            FixedSectionReader::new(&fixed).err(),
            Some(FixedSectionReadError::AmbiguousBoundary)
        );

        let mut fixed = sample();
        fixed.headings[0].at = DisplayPoint::DocumentEnd { row_count: 1 };
        assert_eq!(
            FixedSectionReader::new(&fixed).err(),
            Some(FixedSectionReadError::OverlappingSelection),
            "root preface must not copy bytes owned by a heading"
        );
    }
}
