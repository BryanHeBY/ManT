//! Prepared decoration receipts permit soft wrapping without a second source
//! visit. Their byte ranges index the local visible result, never document IR.
use super::{LayoutText, RowAlignment};
use crate::line_layout::{PreparedLine, UnitKind, WrapMode};
use std::ops::Range;

#[derive(Clone, Debug)]
pub(super) struct PaintedRun {
    pub(super) range: Range<usize>,
    prefix: String,
    suffix: String,
    sliceable: bool,
    generated: bool,
    fixed_space: bool,
}

impl PaintedRun {
    pub(super) fn rows(visible: &str, rendered: &str) -> Vec<Self> {
        let mut result = Vec::new();
        let mut offset = 0;
        for (source, paint) in visible.split('\n').zip(rendered.split('\n')) {
            let run = Self::new(source, paint);
            if run.sliceable {
                result.push(Self {
                    range: offset..offset + source.len(),
                    ..run
                });
            } else {
                // Production decoration leaves authored edge whitespace
                // outside terminal wrappers. Retain those exact three runs
                // rather than treating the complete callback as opaque.
                let core = source.trim();
                let start = source.len() - source.trim_start().len();
                let end = start + core.len();
                let retained = paint
                    .strip_prefix(&source[..start])
                    .and_then(|paint| paint.strip_suffix(&source[end..]));
                let core_run = retained.map(|paint| Self::new(core, paint));
                if let Some(core_run) = core_run.filter(|run| run.sliceable) {
                    for range in [0..start, end..source.len()] {
                        if !range.is_empty() {
                            result.push(Self {
                                range: offset + range.start..offset + range.end,
                                generated: false,
                                ..Self::plain(0..0)
                            });
                        }
                    }
                    result.push(Self {
                        range: offset + start..offset + end,
                        ..core_run
                    });
                } else {
                    result.push(Self {
                        range: offset..offset + source.len(),
                        ..run
                    });
                }
            }
            offset += source.len() + 1;
        }
        result.sort_by_key(|run| run.range.start);
        result
    }

    pub(super) fn plain(range: Range<usize>) -> Self {
        Self {
            range,
            prefix: String::new(),
            suffix: String::new(),
            sliceable: true,
            generated: true,
            fixed_space: false,
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
            fixed_space: false,
        }
    }

    pub(super) fn fixed_space_end(&self) -> usize {
        if self.fixed_space { self.range.end } else { 0 }
    }
}

fn push_painted_span(
    run: &PaintedRun,
    text: &str,
    fixed_space: bool,
    visible: &mut String,
    rendered: &mut String,
    painted: &mut Vec<PaintedRun>,
) {
    if text.is_empty() {
        return;
    }
    let start = visible.len();
    visible.push_str(text);
    rendered.push_str(&run.prefix);
    rendered.push_str(text);
    rendered.push_str(&run.suffix);
    painted.push(PaintedRun {
        range: start..visible.len(),
        fixed_space,
        ..run.clone()
    });
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

    fn painted_units(
        &self,
        prepared: &PreparedLine<'_>,
        range: Range<usize>,
        offset: usize,
    ) -> Option<Self> {
        let units = &prepared.units()[range];
        let Some(first_unit) = units.first() else {
            return Some(Self::default());
        };
        let start = offset + first_unit.bytes.start;
        let end = offset + units.last().expect("nonempty units").bytes.end;
        let mut visible = String::new();
        let mut rendered = String::new();
        let mut painted = Vec::new();
        let first = self.painted.partition_point(|run| run.range.end <= start);
        let selected = &self.painted[first..];
        for run in selected.iter().take_while(|run| run.range.start < end) {
            let from = run.range.start.max(start);
            let to = run.range.end.min(end);
            if from >= to {
                continue;
            }
            if !run.sliceable {
                return None;
            }
            let mut span = String::new();
            let mut fixed_space = run.fixed_space;
            let first_unit = units.partition_point(|unit| offset + unit.bytes.end <= from);
            for unit in units[first_unit..]
                .iter()
                .take_while(|unit| offset + unit.bytes.start < to)
            {
                let tab = matches!(unit.kind, UnitKind::Tab { .. });
                let fixed = tab || run.fixed_space;
                if fixed != fixed_space {
                    push_painted_span(
                        run,
                        &span,
                        fixed_space,
                        &mut visible,
                        &mut rendered,
                        &mut painted,
                    );
                    span.clear();
                    fixed_space = fixed;
                }
                if tab {
                    span.push(' ');
                } else {
                    let fragment = &self.visible
                        [(offset + unit.bytes.start).max(from)..(offset + unit.bytes.end).min(to)];
                    span.push_str(fragment);
                }
            }
            push_painted_span(
                run,
                &span,
                fixed_space,
                &mut visible,
                &mut rendered,
                &mut painted,
            );
        }
        let mut value = Self {
            rendered,
            visible,
            painted,
            ..Self::default()
        };
        if let Some(receipt) = self.alignments.first() {
            value.alignments.push(RowAlignment { row: 0, ..*receipt });
        }
        Some(value)
    }

    /// Resolve TABs inside a prepared cell before outer rails or placement.
    pub(in crate::output::text) fn expanded_tabs(&self) -> Option<Self> {
        if !self.visible.contains('\t') {
            return Some(self.clone());
        }
        let prepared = PreparedLine::new(&self.visible, 0);
        self.painted_units(&prepared, 0..prepared.units().len(), 0)
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
        let prefix_width = super::display_columns(&self.visible[..prefix]);
        width.checked_sub(prefix_width).filter(|width| *width > 0)?;
        if prefix == self.visible.len() {
            return Some(vec![self.clone()]);
        }
        let prepared = PreparedLine::new(&self.visible[prefix..], prefix_width);
        prepared
            .wrap(width, prefix_width, prefix_width, WrapMode::Word)
            .into_iter()
            .map(|row| {
                self.painted_units(&prepared, row.units, prefix)
                    .map(|value| value.prefixed(&self.visible[..prefix]))
            })
            .collect()
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

    #[test]
    fn soft_seams_consume_only_internal_breakable_separators() {
        for (text, width, expected) in [
            (
                "long word here",
                3,
                vec!["lon", "g", "wor", "d", "her", "e"],
            ),
            ("long word here", 4, vec!["long", "word", "here"]),
            ("long word here", 5, vec!["long", "word", "here"]),
            ("long word here", 9, vec!["long word", "here"]),
            ("long word here", 14, vec!["long word here"]),
            ("long    word  here", 6, vec!["long", "word", "here"]),
            ("one  two  xx", 8, vec!["one  two", "xx"]),
            ("long\tword\there", 4, vec!["long", "word", "here"]),
            (" long ", 8, vec![" long "]),
            (" long ", 4, vec![" lon", "g "]),
            ("long ", 4, vec!["long", " "]),
            ("    ", 2, vec!["  ", "  "]),
            ("", 4, vec![""]),
            (
                "e\u{301}e\u{301} e\u{301}e\u{301}",
                2,
                vec!["e\u{301}e\u{301}", "e\u{301}e\u{301}"],
            ),
            ("界界 界界", 4, vec!["界界", "界界"]),
        ] {
            for (open, close) in [
                ("", ""),
                ("\x1b[1m", "\x1b[0m"),
                ("\x1b]8;;https://example.test\x1b\\", "\x1b]8;;\x1b\\"),
            ] {
                let value = LayoutText::decorated(text, format!("{open}{text}{close}"));
                let rows = value.soft_wrapped(width).unwrap();
                assert_eq!(
                    rows.iter()
                        .map(|row| row.visible.as_str())
                        .collect::<Vec<_>>(),
                    expected,
                    "{text:?}/{width}"
                );
                for row in rows {
                    assert_eq!(row.rendered, format!("{open}{}{close}", row.visible));
                }
            }
        }
    }

    #[test]
    fn nonbreaking_spaces_never_become_consumed_soft_seams() {
        for space in ['\u{a0}', '\u{2007}', '\u{202f}'] {
            let text = format!("aaa{space}bbbb");
            for width in [3, 4, 5, 8] {
                let value = LayoutText::decorated(&text, text.clone());
                let rows = value.soft_wrapped(width).unwrap();
                assert_eq!(
                    rows.iter()
                        .map(|row| row.visible.as_str())
                        .collect::<String>(),
                    text
                );
                assert_eq!(super::super::TableText::longest_word_width(&value), 8);
            }
        }
    }

    #[test]
    fn soft_seams_cross_style_and_link_receipts_without_losing_wrappers() {
        let mut text = LayoutText::decorated("long ", "\x1b[1mlong \x1b[0m".into());
        text.append(&LayoutText::decorated(
            " word",
            "\x1b]8;;uri\x1b\\ word\x1b]8;;\x1b\\".into(),
        ));
        let rows = text.soft_wrapped(4).unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| row.visible.as_str())
                .collect::<Vec<_>>(),
            ["long", "word"]
        );
        assert_eq!(rows[0].rendered, "\x1b[1mlong\x1b[0m");
        assert_eq!(rows[1].rendered, "\x1b]8;;uri\x1b\\word\x1b]8;;\x1b\\");
    }

    #[test]
    fn tab_expansion_uses_shared_columns_inside_terminal_receipts_and_authored_edges() {
        use super::super::TableText;
        for (source, expected) in [
            ("abc\tdef", "abc     def"),
            ("\tabc", "        abc"),
            ("abc\t", "abc     "),
            ("a\t\tb", "a               b"),
            ("中e\u{301}\t界", "中e\u{301}     界"),
            (" \tabc\t ", "        abc      "),
        ] {
            for (open, close) in [
                ("", ""),
                ("\x1b[1m", "\x1b[0m"),
                ("\x1b]8;;https://example.test\x1b\\", "\x1b]8;;\x1b\\"),
            ] {
                for edges_outside in [false, true] {
                    let rendered = if edges_outside {
                        let core = source.trim();
                        let start = source.len() - source.trim_start().len();
                        format!(
                            "{}{open}{core}{close}{}",
                            &source[..start],
                            &source[start + core.len()..]
                        )
                    } else {
                        format!("{open}{source}{close}")
                    };
                    let value = LayoutText::decorated(source, rendered);
                    let expanded = value.expanded_tabs().unwrap();
                    assert_eq!(expanded.visible, expected, "{source:?}/{edges_outside}");
                    assert_eq!(
                        expanded.rendered.replace(open, "").replace(close, ""),
                        expected
                    );
                    assert!(!expanded.rendered.contains('\t'));
                    assert_eq!(
                        expanded.width(),
                        crate::line_layout::PreparedLine::new(source, 0).columns()
                    );
                }
            }
        }
    }

    #[test]
    fn split_tab_parts_keep_balanced_decoration_and_significant_edge_space() {
        for width in [1, 4, 7, 8, 9] {
            let source = "\tX\t";
            let value = LayoutText::decorated(source, format!("\x1b[1m{source}\x1b[0m"));
            let rows = value.soft_wrapped(width).unwrap();
            assert_eq!(
                rows.iter()
                    .map(|row| row.visible.as_str())
                    .collect::<String>(),
                "        X       "
            );
            for row in rows {
                assert!(row.visible.len() <= width);
                assert_eq!(
                    row.rendered.matches("\x1b[1m").count(),
                    row.rendered.matches("\x1b[0m").count()
                );
                assert_eq!(
                    row.rendered.replace("\x1b[1m", "").replace("\x1b[0m", ""),
                    row.visible
                );
            }
        }
    }

    #[test]
    fn cross_style_tabs_share_one_hard_row_phase_and_fixed_tail_extent() {
        let mut text = LayoutText::decorated("a", "a".into());
        text.append(&LayoutText::decorated("\t", "\x1b[1m\t\x1b[0m".into()));
        text.append(&LayoutText::decorated(
            "中e\u{301}",
            "\x1b]8;;uri\x1b\\中e\u{301}\x1b]8;;\x1b\\".into(),
        ));
        text.append(&LayoutText::decorated("\tZ", "\x1b[3m\tZ\x1b[0m".into()));
        assert_eq!(
            text.expanded_tabs().unwrap().visible,
            "a       中e\u{301}     Z"
        );
        assert_eq!(
            text.soft_wrapped(8)
                .unwrap()
                .iter()
                .map(|row| row.visible.as_str())
                .collect::<Vec<_>>(),
            ["a", "中e\u{301}", "Z"]
        );
        let tail = LayoutText::decorated("A\t ", "A\t ".into())
            .expanded_tabs()
            .unwrap();
        assert_eq!(tail.field_width().content, 8);
        assert_eq!(tail.field_width().output, 9);
        let mut nested = LayoutText::from("  ");
        nested.append(&tail);
        assert_eq!(nested.field_width().content, 10);
        assert_eq!(nested.field_width().output, 11);
    }

    #[test]
    fn hard_row_splits_keep_production_edge_wrappers_and_restart_tab_phase() {
        use super::super::TableText;
        let source = " a\tb \n\tc ";
        let rendered = " \x1b[1ma\tb\x1b[0m \n\t\x1b[1mc\x1b[0m ";
        let rows = LayoutText::decorated(source, rendered.into()).physical_lines();
        assert_eq!(
            rows.iter()
                .map(|row| row.visible.as_str())
                .collect::<Vec<_>>(),
            [" a      b ", "        c "]
        );
        for row in rows {
            assert_eq!(
                row.rendered.replace("\x1b[1m", "").replace("\x1b[0m", ""),
                row.visible
            );
            assert_eq!(
                row.rendered.matches("\x1b[1m").count(),
                row.rendered.matches("\x1b[0m").count()
            );
        }
    }
}
