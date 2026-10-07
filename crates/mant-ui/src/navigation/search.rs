//! Project logical outline hits onto grapheme-safe, width-dependent rows.

use mant_render::cells::graphemes;
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};
use unicode_width::UnicodeWidthStr;

use super::{NavigationRow, TreePlan, bounded_tree_prefix};
use crate::{
    NavKind,
    document::{LiteralSearch, OutlineField, OutlineRecord},
    theme,
};

const MAX_RESULTS: usize = 4096;
const MAX_RANGES: usize = 32768;

#[derive(Debug, Clone)]
pub(crate) struct FieldHit {
    pub(crate) field: OutlineField,
    pub(crate) ranges: Vec<Range<usize>>,
}

#[derive(Debug, Clone)]
pub(crate) struct OutlineHit {
    pub(crate) node_index: usize,
    label: Arc<str>,
    pub(crate) fields: Vec<FieldHit>,
}

pub(crate) fn find(records: &[OutlineRecord], query: &str) -> (Vec<OutlineHit>, bool) {
    let search = LiteralSearch::new(query);
    let mut matches = Vec::new();
    let mut remaining = MAX_RANGES;
    let mut limited = false;
    for record in records {
        let mut fields = Vec::new();
        for field in &record.fields {
            if !field.target
                && field.text != record.label
                && record.label.contains(field.text.as_ref())
            {
                continue;
            }
            let mut ranges = search.ranges(&field.text, remaining.saturating_add(1));
            if ranges.is_empty() {
                continue;
            }
            if matches.len() == MAX_RESULTS {
                limited = true;
                break;
            }
            if ranges.len() > remaining {
                ranges.truncate(remaining);
                limited = true;
            }
            remaining -= ranges.len();
            if !ranges.is_empty() {
                fields.push(FieldHit {
                    field: field.clone(),
                    ranges,
                });
            }
            if limited {
                break;
            }
        }
        if !fields.is_empty() {
            matches.push(OutlineHit {
                node_index: record.node_index,
                label: Arc::clone(&record.label),
                fields,
            });
        }
        if limited {
            break;
        }
    }
    (matches, limited)
}

/// Match lookup is built once per frame, not once per visible row.
pub(crate) struct OutlinePresentation<'a> {
    matches: HashMap<usize, &'a OutlineHit>,
    active: Option<usize>,
}

#[derive(Clone, Copy)]
pub(crate) struct OutlineLabels {
    pub(crate) selected: usize,
    pub(crate) full: bool,
}

impl<'a> OutlinePresentation<'a> {
    pub(crate) fn new(matches: &'a [OutlineHit], active: usize) -> Self {
        Self {
            matches: matches.iter().map(|hit| (hit.node_index, hit)).collect(),
            active: matches.get(active).map(|hit| hit.node_index),
        }
    }

    pub(crate) fn rows(
        &self,
        plan: &TreePlan<'_>,
        visible: &[usize],
        labels: OutlineLabels,
        expanded: &HashSet<String>,
        width: usize,
        badges: &HashMap<String, String>,
    ) -> Vec<NavigationRow> {
        let ordinary = super::rows_with_references(
            plan,
            visible,
            labels.selected,
            expanded,
            labels.full,
            width,
            badges,
        );
        let mut rows = Vec::new();
        let mut previous = None;
        for row in ordinary {
            if let Some(hit) = self.matches.get(&row.node_index) {
                if previous != Some(row.node_index) {
                    rows.extend(self.hit_rows(plan, hit, labels, expanded, width, badges));
                }
            } else {
                rows.push(row);
            }
            previous = Some(rows.last().map_or(usize::MAX, |row| row.node_index));
        }
        rows
    }

    fn hit_rows(
        &self,
        plan: &TreePlan<'_>,
        hit: &OutlineHit,
        labels: OutlineLabels,
        expanded: &HashSet<String>,
        width: usize,
        badges: &HashMap<String, String>,
    ) -> Vec<NavigationRow> {
        if width == 0 {
            return vec![NavigationRow {
                node_index: hit.node_index,
                line: Line::default(),
            }];
        }
        let node = &plan.nodes[hit.node_index];
        let selected = hit.node_index == labels.selected;
        let full_labels = labels.full;
        let prefixes = plan.prefixes(hit.node_index, selected, expanded.contains(&node.id), width);
        let first = bounded_tree_prefix(&prefixes.first, width);
        let continuation = bounded_tree_prefix(&prefixes.continuation, width);
        let available = width
            .saturating_sub(first.width().max(continuation.width()))
            .max(1);
        let style = theme::navigation_style(node.kind, selected);
        let prefix_style = theme::style(if selected {
            theme::StyleRole::TreeFocus
        } else {
            theme::StyleRole::TreeGuide
        })
        .patch(Style {
            bg: style.bg,
            ..Style::default()
        });
        let continuation_style = theme::style(if selected {
            theme::StyleRole::TreeContinuationFocus
        } else {
            theme::StyleRole::TreeGuide
        })
        .patch(Style {
            bg: style.bg,
            ..Style::default()
        });
        let role = if self.active == Some(hit.node_index) {
            theme::InteractionRole::SearchActive
        } else {
            theme::InteractionRole::SearchMatch
        };
        let mut runs = Vec::new();
        if selected || full_labels {
            runs = complete_labels(hit, node.kind == NavKind::Reference, style, role);
        } else if let Some(field) = hit.fields.first() {
            runs.push(compact_context(
                field,
                node.kind == NavKind::Reference,
                available,
                style,
                role,
            ));
        }
        if let Some(badge) = badges.get(&node.id)
            && (selected || full_labels)
            && let Some(run) = runs.first_mut()
        {
            run.push((
                format!(" {badge}"),
                theme::navigation_reference_style(node.kind, selected),
            ));
        }
        wrap_runs(
            &runs,
            hit.node_index,
            &first,
            &continuation,
            [prefix_style, continuation_style],
            available,
            width,
        )
    }
}

fn complete_labels(
    hit: &OutlineHit,
    reference: bool,
    style: Style,
    role: theme::InteractionRole,
) -> Vec<Vec<(String, Style)>> {
    let label_ranges = hit
        .fields
        .iter()
        .find(|field| field.field.text == hit.label)
        .map_or(&[][..], |field| field.ranges.as_slice());
    let mut main = Vec::new();
    if reference {
        main.push(("↗ ".to_owned(), style));
    }
    main.extend(styled_graphemes(
        &hit.label,
        label_ranges,
        style,
        role,
        0..hit.label.len(),
    ));
    let mut runs = vec![main];
    for field in &hit.fields {
        if field.field.text == hit.label {
            continue;
        }
        let mut run = vec![(
            if field.field.target { "→ " } else { "= " }.to_owned(),
            style,
        )];
        run.extend(styled_graphemes(
            &field.field.text,
            &field.ranges,
            style,
            role,
            0..field.field.text.len(),
        ));
        runs.push(run);
    }
    runs
}

fn compact_context(
    field: &FieldHit,
    reference: bool,
    available: usize,
    style: Style,
    role: theme::InteractionRole,
) -> Vec<(String, Style)> {
    let budget = if field.field.target || reference {
        available.saturating_sub(2).max(1)
    } else {
        available
    };
    let range = context_range(&field.field.text, &field.ranges[0], budget);
    // On very deep/narrow rows, drop decorative context markers before a
    // real hit. A compact result must not turn into several marker-only rows.
    let content_width = field.field.text[range.clone()].width().min(available);
    let mut leading = range.start != 0;
    let mut trailing = range.end != field.field.text.len();
    let mut prefix = field.field.target || reference;
    if content_width + usize::from(leading) + usize::from(trailing) + usize::from(prefix) * 2
        > available
    {
        trailing = false;
    }
    if content_width + usize::from(leading) + usize::from(prefix) * 2 > available {
        leading = false;
    }
    if content_width + usize::from(prefix) * 2 > available {
        prefix = false;
    }
    let mut run = Vec::new();
    if prefix && field.field.target {
        run.push(("→ ".to_owned(), style));
    } else if prefix && reference {
        run.push(("↗ ".to_owned(), style));
    }
    if leading {
        run.push(("…".to_owned(), style));
    }
    run.extend(styled_graphemes(
        &field.field.text,
        &field.ranges,
        style,
        role,
        range.clone(),
    ));
    if trailing {
        run.push(("…".to_owned(), style));
    }
    run
}

fn context_range(text: &str, hit: &Range<usize>, width: usize) -> Range<usize> {
    let glyphs = graphemes(text).collect::<Vec<_>>();
    let mut bytes = 0;
    let mut start = 0;
    for (index, glyph) in glyphs.iter().enumerate() {
        if bytes + glyph.text().len() > hit.start {
            start = index;
            break;
        }
        bytes += glyph.text().len();
    }
    let mut before = width.saturating_sub(2) / 3;
    while start > 0 && glyphs[start - 1].columns() <= before {
        start -= 1;
        before -= glyphs[start].columns();
    }
    let start_byte = glyphs[..start].iter().map(|glyph| glyph.text().len()).sum();
    let mut end = start_byte;
    let mut columns = 0;
    for glyph in &glyphs[start..] {
        if columns + glyph.columns() > width.saturating_sub(2).max(1) {
            break;
        }
        columns += glyph.columns();
        end += glyph.text().len();
    }
    // A too-wide grapheme is replaced as a whole by the row wrapper.
    if end == start_byte && start < glyphs.len() {
        end += glyphs[start].text().len();
    }
    start_byte..end
}

fn styled_graphemes(
    text: &str,
    ranges: &[Range<usize>],
    style: Style,
    role: theme::InteractionRole,
    slice: Range<usize>,
) -> Vec<(String, Style)> {
    let mut byte = slice.start;
    let mut range_index = ranges.partition_point(|range| range.end <= byte);
    graphemes(&text[slice])
        .map(|glyph| {
            let end = byte + glyph.text().len();
            while ranges
                .get(range_index)
                .is_some_and(|range| range.end <= byte)
            {
                range_index += 1;
            }
            let matched = ranges
                .get(range_index)
                .is_some_and(|range| range.start < end && range.end > byte);
            byte = end;
            (
                glyph.text().to_owned(),
                if matched {
                    theme::interact(style, role)
                } else {
                    style
                },
            )
        })
        .collect()
}

fn wrap_runs(
    runs: &[Vec<(String, Style)>],
    node_index: usize,
    first: &str,
    continuation: &str,
    prefix_styles: [Style; 2],
    available: usize,
    width: usize,
) -> Vec<NavigationRow> {
    let mut rows = Vec::new();
    for run in runs {
        let mut spans = vec![Span::styled(
            if rows.is_empty() {
                first.to_owned()
            } else {
                continuation.to_owned()
            },
            if rows.is_empty() {
                prefix_styles[0]
            } else {
                prefix_styles[1]
            },
        )];
        let mut used = 0;
        for (text, style) in run {
            for glyph in graphemes(text) {
                let columns = glyph.columns().min(available);
                if used + columns > available && used != 0 {
                    rows.push(NavigationRow {
                        node_index,
                        line: Line::from(std::mem::take(&mut spans)),
                    });
                    spans.push(Span::styled(continuation.to_owned(), prefix_styles[1]));
                    used = 0;
                }
                let text = if glyph.columns() > available {
                    "�"
                } else {
                    glyph.text()
                };
                // Keep same-style clusters in one span: Ratatui must not
                // re-segment a combining character independently of its base.
                let has_text = spans.len() > 1;
                if let Some(last) = spans.last_mut()
                    && last.style == *style
                    && has_text
                {
                    last.content.to_mut().push_str(text);
                } else {
                    spans.push(Span::styled(text.to_owned(), *style));
                }
                used += columns;
            }
        }
        let padding = width.saturating_sub(spans.iter().map(Span::width).sum::<usize>());
        spans.push(Span::styled(
            " ".repeat(padding),
            Style {
                bg: prefix_styles[0].bg,
                ..Style::default()
            },
        ));
        rows.push(NavigationRow {
            node_index,
            line: Line::from(spans),
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(index: usize, value: &str) -> OutlineRecord {
        let text: Arc<str> = Arc::from(value);
        OutlineRecord {
            node_index: index,
            label: Arc::clone(&text),
            fields: vec![OutlineField {
                text,
                target: false,
                anchor: format!("node-{index}"),
            }],
        }
    }

    #[test]
    fn result_limit_preserves_source_order_and_reports_incomplete_coverage() {
        let records = (0..=MAX_RESULTS)
            .map(|index| record(index, "needle"))
            .collect::<Vec<_>>();
        let (hits, limited) = find(&records, "needle");
        assert!(limited);
        assert_eq!(hits.len(), MAX_RESULTS);
        assert_eq!(hits[0].node_index, 0);
        assert_eq!(hits.last().unwrap().node_index, MAX_RESULTS - 1);
    }

    #[test]
    fn text_range_limit_keeps_the_first_real_hit_instead_of_claiming_a_miss() {
        let (hits, limited) = find(&[record(0, &"a".repeat(MAX_RANGES + 2))], "a");
        assert!(limited);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].fields[0].ranges.len(), MAX_RANGES);
        assert_eq!(hits[0].fields[0].ranges[0], 0..1);
    }

    #[test]
    fn independent_fields_never_manufacture_a_cross_field_match() {
        let mut source = record(0, "abc");
        source.fields.push(OutlineField {
            text: Arc::from("def"),
            target: true,
            anchor: "target".into(),
        });
        let (hits, limited) = find(&[source], "cde");
        assert!(!limited);
        assert!(hits.is_empty());
    }
}
