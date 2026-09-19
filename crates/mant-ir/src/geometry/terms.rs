//! Measure the open final label row, independently of preceding label rows.
use crate::{DefinitionItem, DefinitionPlacement, Inline};

/// Shared tab stop used by resolved definition geometry.
pub const DEFINITION_TAB_STOP_COLUMNS: usize = 8;

/// One reader's resolved placement and description origins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefinitionPlacementResolution {
    /// Whether the eligible first description paragraph shares the last label row.
    pub run_in: bool,
    /// Final open label width, excluding its parent origin.
    pub final_label_width_columns: Option<usize>,
    /// Absolute, padded body origin.
    pub body_origin_columns: usize,
    /// Absolute first-description origin after applying placement and paragraph layout.
    pub first_description_origin_columns: usize,
    /// Absolute first-description origin before any run-in label displacement.
    pub stacked_description_origin_columns: usize,
    /// Absolute continuation origin for later rows of the first paragraph.
    pub continuation_origin_columns: usize,
}

/// Width-independent definition facts retained until a reader knows its
/// allocated width. This is presentation geometry, not source-parser state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefinitionPlacementPlan {
    placement: DefinitionPlacement,
    parent_origin_columns: i32,
    body_indent_columns: i32,
    final_label_width_by_tab_origin: Option<[usize; DEFINITION_TAB_STOP_COLUMNS]>,
    first_description_indent_columns: i32,
    continuation_indent_columns: i32,
    min_gap_columns: usize,
    candidate: bool,
}

impl DefinitionPlacementPlan {
    /// Translate the plan with its containing layout without resolving it.
    #[must_use]
    pub const fn translated(self, delta_columns: i32) -> Self {
        Self {
            parent_origin_columns: self.parent_origin_columns.saturating_add(delta_columns),
            ..self
        }
    }

    /// Resolve the retained policy for the allocated container width.
    #[must_use]
    pub fn resolve(self, available_width_columns: Option<usize>) -> DefinitionPlacementResolution {
        let label_origin = super::padding(self.parent_origin_columns);
        let final_label_width_columns = self
            .final_label_width_by_tab_origin
            .map(|widths| widths[label_origin % DEFINITION_TAB_STOP_COLUMNS]);
        let body_origin =
            super::compose_origin(self.parent_origin_columns, self.body_indent_columns);
        let body_origin_columns = super::padding(body_origin);
        let run_in = self.candidate
            && match self.placement {
                DefinitionPlacement::Stacked => false,
                DefinitionPlacement::RunIn => true,
                DefinitionPlacement::Fit => {
                    let field_end = available_width_columns.map_or(body_origin_columns, |width| {
                        body_origin_columns.min(label_origin.saturating_add(width))
                    });
                    final_label_width_columns.is_some_and(|width| {
                        label_origin
                            .saturating_add(width)
                            .saturating_add(self.min_gap_columns)
                            <= field_end
                    })
                }
            };
        let first = super::padding(super::compose_origin(
            body_origin,
            self.first_description_indent_columns,
        ));
        let continuation = super::padding(super::compose_origin(
            super::compose_origin(body_origin, self.first_description_indent_columns),
            self.continuation_indent_columns,
        ));
        let first = if run_in {
            first.max(
                label_origin
                    .saturating_add(final_label_width_columns.unwrap_or_default())
                    .saturating_add(self.min_gap_columns),
            )
        } else {
            first
        };
        DefinitionPlacementResolution {
            run_in,
            final_label_width_columns,
            body_origin_columns,
            first_description_origin_columns: first,
            stacked_description_origin_columns: super::padding(super::compose_origin(
                body_origin,
                self.first_description_indent_columns,
            )),
            continuation_origin_columns: continuation,
        }
    }
}

/// Retain one definition's structural eligibility and width-independent facts.
#[must_use]
pub fn definition_placement_plan(
    item: &DefinitionItem,
    parent_origin_columns: i32,
) -> DefinitionPlacementPlan {
    let description = item.run_in_description();
    let final_label_width_by_tab_origin = definition_run_in_widths(&item.terms);
    let (first_description_indent_columns, continuation_indent_columns) = description
        .map(|(_, layout)| (layout.indent_columns, layout.continuation_indent_columns))
        .unwrap_or_default();
    DefinitionPlacementPlan {
        placement: item.layout.placement,
        parent_origin_columns,
        body_indent_columns: item.layout.body_indent_columns,
        final_label_width_by_tab_origin,
        first_description_indent_columns,
        continuation_indent_columns,
        min_gap_columns: usize::from(item.layout.min_term_gap_columns),
        candidate: description.is_some() && final_label_width_by_tab_origin.is_some(),
    }
}

/// Resolve one definition's conditional placement for an effective viewport.
///
/// `available_width_columns` is the container width measured from the padded
/// parent origin. `None` is the unbounded text/Markdown projection,
/// but `Fit` still uses the producer's declared body field rather than treating
/// every label as fitting. A bounded reader clips that field to its viewport.
#[must_use]
pub fn definition_placement(
    item: &DefinitionItem,
    parent_origin_columns: i32,
    available_width_columns: Option<usize>,
) -> DefinitionPlacementResolution {
    definition_placement_plan(item, parent_origin_columns).resolve(available_width_columns)
}

/// Display cells occupied by the final open definition-label row.
///
/// Each nonempty term starts a separate physical label. Earlier rows are
/// already complete and cannot prevent the last row from running into a body.
/// Anchors and empty wrappers do not create rows; a trailing explicit break
/// closes a row and must not be erased by trimming. Width is measured after
/// joining style/link fragments, preserving combining and wide characters.
#[must_use]
pub fn definition_run_in_width(terms: &[Vec<Inline>]) -> Option<usize> {
    definition_run_in_width_at(terms, 0)
}

/// Display cells occupied by the final open label row at an absolute origin.
/// Tabs advance to shared eight-column stops; styling, links and anchors do not
/// create cells. The returned width is relative to `origin_columns`.
#[must_use]
pub fn definition_run_in_width_at(terms: &[Vec<Inline>], origin_columns: usize) -> Option<usize> {
    definition_run_in_widths(terms)
        .map(|widths| widths[origin_columns % DEFINITION_TAB_STOP_COLUMNS])
}

fn definition_run_in_widths(terms: &[Vec<Inline>]) -> Option<[usize; DEFINITION_TAB_STOP_COLUMNS]> {
    let mut final_row = String::new();
    let mut present = false;
    for term in terms {
        let mut row = String::new();
        let mut term_present = false;
        append(term, &mut row, &mut term_present);
        if term_present {
            final_row = row;
            present = true;
        }
    }
    (present && !final_row.is_empty())
        .then(|| std::array::from_fn(|origin| row_width(&final_row, origin)))
}

fn row_width(row: &str, origin: usize) -> usize {
    let mut column = origin;
    let mut pieces = row.split('\t').peekable();
    while let Some(piece) = pieces.next() {
        column = column.saturating_add(unicode_width::UnicodeWidthStr::width(piece));
        if pieces.peek().is_some() {
            column = column
                .saturating_add(DEFINITION_TAB_STOP_COLUMNS - column % DEFINITION_TAB_STOP_COLUMNS);
        }
    }
    column.saturating_sub(origin)
}

fn append(nodes: &[Inline], row: &mut String, present: &mut bool) {
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => {
                *present |= !value.is_empty();
                if let Some((_, tail)) = value.rsplit_once('\n') {
                    row.clear();
                    row.push_str(tail);
                } else {
                    row.push_str(value);
                }
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => append(children, row, present),
            Inline::LineBreak => {
                row.clear();
                *present = true;
            }
            Inline::Anchor { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Block, DefinitionLayout, LayoutHint};

    fn text(value: &str) -> Inline {
        Inline::Text {
            value: value.into(),
        }
    }

    fn item(
        term: Vec<Inline>,
        placement: DefinitionPlacement,
        body_indent_columns: i32,
        gap: u16,
    ) -> DefinitionItem {
        DefinitionItem {
            source: None,
            entry: None,
            terms: vec![term],
            description: vec![Block::Paragraph {
                children: vec![text("body")],
                layout: LayoutHint::default(),
                source: None,
            }],
            layout: DefinitionLayout {
                placement,
                body_indent_columns,
                min_term_gap_columns: gap,
                spacing_before_lines: None,
            },
        }
    }

    #[test]
    fn preceding_labels_and_hard_rows_do_not_measure_the_final_open_row() {
        assert_eq!(
            definition_run_in_width(&[vec![text("long-label")], vec![text("-b")]]),
            Some(2)
        );
        assert_eq!(
            definition_run_in_width(&[vec![text("long-label\n-b")]]),
            Some(2)
        );
        assert_eq!(
            definition_run_in_width(&[vec![text("-b")], vec![text("long-label")]]),
            Some(10)
        );
        assert_eq!(definition_run_in_width(&[vec![text("  -b ")]]), Some(5));
    }

    #[test]
    fn anchors_and_wrappers_do_not_reopen_a_closed_label_row() {
        let anchor = Inline::anchor("target");
        for boundary in [Inline::LineBreak, text("\n")] {
            assert_eq!(
                definition_run_in_width(&[vec![text("-b"), boundary, anchor.clone()]]),
                None
            );
        }
        assert_eq!(
            definition_run_in_width(&[vec![text("-b")], vec![anchor]]),
            Some(2)
        );
        assert_eq!(
            definition_run_in_width(&[vec![Inline::Emphasis {
                children: Vec::new()
            }]]),
            None
        );
        assert_eq!(
            definition_run_in_width(&[vec![text("-b"), Inline::LineBreak, text("-c")]]),
            Some(2)
        );
    }

    #[test]
    fn styled_link_fragments_share_unicode_cell_measurement() {
        let terms = vec![vec![
            text("long-label"),
            Inline::LineBreak,
            Inline::Strong {
                children: vec![text("日")],
            },
            Inline::Link {
                target: crate::LinkTarget::External {
                    uri: "https://example.org".into(),
                },
                title: None,
                children: vec![text("本e")],
            },
            Inline::Emphasis {
                children: vec![text("\u{301}")],
            },
        ]];
        assert_eq!(definition_run_in_width(&terms), Some(5));
    }

    #[test]
    fn placement_modes_share_one_field_capacity_decision() {
        // Pinned CVS term.c::term_flushln() compares the final label field and
        // trailspace against the body field; man_term.c and mdoc_term.c select
        // HANG-like unconditional or BRTRSP-like conditional placement.
        let mut definition = item(vec![text("short")], DefinitionPlacement::Fit, 8, 2);
        assert!(definition_placement(&definition, 0, None).run_in);
        assert!(definition_placement(&definition, 5, None).run_in);
        assert!(definition_placement(&definition, 5, Some(8)).run_in);
        assert!(!definition_placement(&definition, 5, Some(6)).run_in);

        definition.terms = vec![vec![text("longlabel")]];
        assert!(!definition_placement(&definition, 0, None).run_in);
        definition.layout.placement = DefinitionPlacement::RunIn;
        assert!(definition_placement(&definition, 0, Some(1)).run_in);
        definition.layout.placement = DefinitionPlacement::Stacked;
        assert!(!definition_placement(&definition, 0, None).run_in);
    }

    #[test]
    fn tabs_unicode_trailing_cells_and_hard_rows_use_absolute_label_geometry() {
        assert_eq!(
            definition_run_in_width_at(&[vec![text("a\tb")]], 0),
            Some(9)
        );
        assert_eq!(
            definition_run_in_width_at(&[vec![text("a\tb")]], 3),
            Some(6)
        );
        assert_eq!(
            definition_run_in_width_at(&[vec![text("old\n日e\u{301} ")]], 2),
            Some(4)
        );
        assert_eq!(
            definition_run_in_width_at(
                &[vec![
                    text("closed"),
                    Inline::LineBreak,
                    Inline::anchor("end")
                ]],
                0
            ),
            None
        );
    }

    #[test]
    fn structural_ineligibility_stacks_without_losing_body_geometry() {
        let mut definition = item(vec![text("x")], DefinitionPlacement::RunIn, 4, 1);
        let Block::Paragraph { layout, .. } = &mut definition.description[0] else {
            unreachable!()
        };
        layout.spacing_before_lines = 1;
        let resolution = definition_placement(&definition, -2, None);
        assert!(!resolution.run_in);
        assert_eq!(resolution.body_origin_columns, 2);

        definition.description = vec![Block::ThematicBreak { source: None }];
        assert!(!definition_placement(&definition, 0, None).run_in);
        definition.description = vec![Block::Paragraph {
            children: vec![text("body")],
            layout: LayoutHint::default(),
            source: None,
        }];
        definition.terms = vec![vec![Inline::anchor("only-target")]];
        assert!(!definition_placement(&definition, 0, None).run_in);
        assert_eq!(definition.layout.placement, DefinitionPlacement::RunIn);
    }

    #[test]
    fn description_origins_are_translation_stable() {
        let mut definition = item(vec![text("x")], DefinitionPlacement::RunIn, 6, 2);
        let Block::Paragraph { layout, .. } = &mut definition.description[0] else {
            unreachable!()
        };
        layout.indent_columns = 1;
        layout.continuation_indent_columns = 3;
        let base = definition_placement_plan(&definition, 2);
        let translated = base.translated(7);
        let a = base.resolve(None);
        let b = translated.resolve(None);
        assert_eq!(b.body_origin_columns, a.body_origin_columns + 7);
        assert_eq!(
            b.first_description_origin_columns,
            a.first_description_origin_columns + 7
        );
        assert_eq!(
            b.continuation_origin_columns,
            a.continuation_origin_columns + 7
        );
    }

    #[test]
    fn translated_tab_plan_matches_direct_construction_at_the_new_origin() {
        // Pinned CVS term.c::term_fill() and term_tab.c::term_tab_next()
        // advance tabs from the current absolute device column. Translation
        // must therefore select the width for the new tab-stop residue.
        let definition = item(vec![text("a\tb")], DefinitionPlacement::Fit, 8, 1);
        for delta in 0..DEFINITION_TAB_STOP_COLUMNS {
            let translated = definition_placement_plan(&definition, 0)
                .translated(i32::try_from(delta).unwrap())
                .resolve(Some(16));
            let direct = definition_placement_plan(&definition, i32::try_from(delta).unwrap())
                .resolve(Some(16));
            assert_eq!(translated, direct, "absolute origin {delta}");
            assert_eq!(
                translated.final_label_width_columns,
                definition_run_in_width_at(&definition.terms, delta)
            );
        }
    }
}
