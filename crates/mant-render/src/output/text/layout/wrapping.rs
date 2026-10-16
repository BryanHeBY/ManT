//! Prepared decoration receipts permit soft wrapping without a second source
//! visit. Their byte ranges index the local visible result, never document IR.
use super::{LayoutText, RowAlignment};
use std::ops::Range;

#[derive(Clone, Debug)]
pub(super) struct PaintedRun {
    pub(super) range: Range<usize>,
    prefix: String,
    suffix: String,
    sliceable: bool,
    generated: bool,
}

impl PaintedRun {
    pub(super) fn plain(range: Range<usize>) -> Self {
        Self {
            range,
            prefix: String::new(),
            suffix: String::new(),
            sliceable: true,
            generated: true,
        }
    }

    pub(super) fn new(visible: &str, rendered: &str) -> Self {
        let start =
            decoration_end(rendered).filter(|start| rendered[*start..].starts_with(visible));
        let Some(start) = start.filter(|start| {
            decoration_end(&rendered[*start + visible.len()..])
                == Some(rendered.len() - *start - visible.len())
        }) else {
            return Self {
                sliceable: false,
                generated: false,
                ..Self::plain(0..visible.len())
            };
        };
        Self {
            range: 0..visible.len(),
            prefix: rendered[..start].into(),
            suffix: rendered[start + visible.len()..].into(),
            sliceable: true,
            generated: false,
        }
    }
}

impl LayoutText {
    /// Preserve the original paint receipt through existing hard-row splits.
    /// Sorted nonoverlapping runs are distributed in O(runs log rows + rows).
    pub(super) fn distribute_paint(&self, rows: &mut [Self]) {
        let mut starts = Vec::with_capacity(rows.len());
        let mut offset = 0;
        for row in rows.iter_mut() {
            starts.push(offset);
            offset += row.visible.len() + 1;
            row.painted.clear();
        }
        for run in &self.painted {
            let mut row = starts
                .partition_point(|start| *start <= run.range.start)
                .saturating_sub(1);
            while let Some(start) = starts.get(row).copied() {
                if start > run.range.end {
                    break;
                }
                let end = start + rows[row].visible.len();
                let from = run.range.start.max(start);
                let to = run.range.end.min(end);
                if from <= to {
                    let mut part = run.clone();
                    part.range = from - start..to - start;
                    rows[row].painted.push(part);
                }
                if run.range.end <= end {
                    break;
                }
                row += 1;
            }
        }
    }

    fn painted_slice(&self, range: Range<usize>) -> Option<Self> {
        let mut rendered = String::new();
        let first = self
            .painted
            .partition_point(|run| run.range.end <= range.start);
        let selected = &self.painted[first..];
        for run in selected
            .iter()
            .take_while(|run| run.range.start < range.end)
        {
            let from = run.range.start.max(range.start);
            let to = run.range.end.min(range.end);
            if from >= to {
                continue;
            }
            if !run.sliceable {
                return None;
            }
            rendered.push_str(&run.prefix);
            rendered.push_str(&self.visible[from..to]);
            rendered.push_str(&run.suffix);
        }
        let mut value = Self::decorated(&self.visible[range.clone()], rendered);
        value.painted = self.painted[first..]
            .iter()
            .take_while(|run| run.range.start < range.end)
            .filter_map(|run| {
                let start = run.range.start.max(range.start);
                let end = run.range.end.min(range.end);
                (start < end).then(|| PaintedRun {
                    range: start - range.start..end - range.start,
                    ..run.clone()
                })
            })
            .collect();
        if let Some(receipt) = self.alignments.first() {
            value.alignments.push(RowAlignment { row: 0, ..*receipt });
        }
        Some(value)
    }

    pub(in crate::output::text) fn leading_layout_padding(&self) -> usize {
        self.painted
            .iter()
            .take_while(|run| {
                run.generated
                    && self.visible[run.range.clone()]
                        .chars()
                        .all(|character| character == ' ')
            })
            .last()
            .map_or(0, |run| run.range.end)
    }

    pub(super) fn soft_wrapped(&self, width: usize) -> Option<Vec<Self>> {
        if self.visible.is_empty() {
            return Some(vec![self.clone()]);
        }
        let prefix = self.leading_layout_padding();
        let prefix_width = mant_ir::geometry::text_width(&self.visible[..prefix]);
        let available = width.checked_sub(prefix_width).filter(|width| *width > 0)?;
        let mut output = Vec::new();
        let mut start = prefix;
        if start == self.visible.len() {
            return Some(vec![self.clone()]);
        }
        while start < self.visible.len() {
            let mut used = 0_usize;
            let mut end = start;
            let mut word = None;
            for glyph in crate::cells::graphemes(&self.visible[start..]) {
                if used.saturating_add(glyph.columns()) > available && end > start {
                    break;
                }
                used = used.saturating_add(glyph.columns());
                end = start + glyph.bytes().end;
                if self.visible[start + glyph.bytes().start..end]
                    .chars()
                    .all(char::is_whitespace)
                {
                    word = Some(end);
                }
                if used >= available {
                    break;
                }
            }
            if end < self.visible.len()
                && let Some(word) = word
            {
                end = word;
            }
            let value = self
                .painted_slice(start..end)?
                .prefixed(&self.visible[..prefix]);
            output.push(value);
            start = end;
        }
        Some(output)
    }
}

/// Only terminal control wrappers can be split without interpreting arbitrary
/// decorator syntax. OSC targets may themselves contain the visible label;
/// scan the complete control sequence rather than searching for that text.
fn decoration_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while bytes.get(index) == Some(&0x1b) {
        match bytes.get(index + 1) {
            Some(b'[') => {
                index += 2;
                while bytes
                    .get(index)
                    .is_some_and(|byte| !(0x40..=0x7e).contains(byte))
                {
                    index += 1;
                }
                bytes.get(index)?;
                index += 1;
            }
            Some(b']') => {
                index += 2;
                loop {
                    match (bytes.get(index), bytes.get(index + 1)) {
                        (Some(7), _) => {
                            index += 1;
                            break;
                        }
                        (Some(0x1b), Some(b'\\')) => {
                            index += 2;
                            break;
                        }
                        (Some(_), _) => index += 1,
                        (None, _) => return None,
                    }
                }
            }
            _ => return None,
        }
    }
    Some(index)
}

#[cfg(test)]
mod tests {
    use super::LayoutText;

    #[test]
    fn prepared_ansi_and_osc_wrappers_remain_complete_on_every_visual_row() {
        let label = "https://example.test/long-label";
        let open = format!("\x1b]8;;{label}\x1b\\\x1b[1m");
        let close = "\x1b[0m\x1b]8;;\x1b\\";
        let text = LayoutText::decorated(label, format!("{open}{label}{close}"));
        let rows = text.soft_wrapped(8).unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| row.visible.as_str())
                .collect::<String>(),
            label
        );
        for row in rows {
            assert_eq!(row.rendered, format!("{open}{}{close}", row.visible));
        }
    }

    #[test]
    fn arbitrary_nonterminal_decorators_keep_the_unsplit_fallback() {
        let text = LayoutText::decorated("long authored body", "<long authored body>".into());
        assert!(text.soft_wrapped(4).is_none());
        assert_eq!(text.rendered, "<long authored body>");
    }
}
