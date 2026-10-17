//! Outline layout, presentation and its final scrollbar geometry.
use crate::app::{App, PointerDrag, fit_to_width};
use crate::{navigation, scrollbar::VerticalScrollbar, text::sanitize_terminal_text, theme};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
};
use unicode_width::UnicodeWidthStr;
impl App {
    pub(super) fn draw_navigation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.geometry.outline_region = area;
        frame.render_widget(
            Block::default().style(theme::style(theme::StyleRole::OutlineSurface)),
            area,
        );
        let [header_area, outline_label_area, navigation_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(area);

        self.draw_navigation_header(frame, header_area);
        frame.render_widget(
            Paragraph::new(
                if self.reading_focus_visible()
                    && self.reading_focus() == crate::app::focus::ReaderFocus::Outline
                {
                    " ▶ OUTLINE"
                } else {
                    " OUTLINE"
                },
            )
            .style(
                theme::style(theme::StyleRole::OutlineSurface).patch(theme::style(
                    if self.reading_focus_visible()
                        && self.reading_focus() == crate::app::focus::ReaderFocus::Outline
                    {
                        theme::StyleRole::PaneFocus
                    } else {
                        theme::StyleRole::Metadata
                    },
                )),
            ),
            outline_label_area,
        );

        let search_reflow = self.outline_search.input.is_open()
            && self.geometry.navigation.width != navigation_area.width;
        self.geometry.navigation = navigation_area;
        let visible = self.visible_navigation_indices();
        let height = usize::from(navigation_area.height);
        let rows = self.outline_rows(&visible, navigation_area.width, height);
        let row_count = rows.len();
        let maximum = row_count.saturating_sub(height);
        self.navigation_scroll = self.navigation_scroll.min(maximum);
        let request = self
            .navigation_viewport_request
            .take()
            .filter(|request| request.node_index() == self.selected)
            .or_else(|| {
                search_reflow.then_some(crate::app::NavigationViewportRequest::Reveal {
                    node_index: self.selected,
                })
            });
        let search_row = if self.outline_search.input.is_open() {
            rows.iter().position(|row| {
                row.node_index == self.selected
                    && row
                        .line
                        .spans
                        .iter()
                        .any(|span| span.style.bg == Some(theme::SEARCH_ACTIVE))
            })
        } else {
            None
        };
        if let Some((request, range)) =
            request.zip(navigation::node_row_range(&rows, self.selected))
        {
            match request {
                crate::app::NavigationViewportRequest::Reveal { .. } => {
                    if let Some(row) = search_row {
                        self.keep_selected_navigation_visible(row..row.saturating_add(1), height);
                    } else {
                        self.keep_selected_navigation_visible(range, height);
                    }
                }
                crate::app::NavigationViewportRequest::PreserveRow { row, .. } => {
                    if let Some(row) = search_row {
                        self.keep_selected_navigation_visible(row..row.saturating_add(1), height);
                    } else {
                        self.keep_selected_navigation_at_row(range, height, row, maximum);
                    }
                }
            }
        }
        let visible_rows = rows
            .into_iter()
            .skip(self.navigation_scroll)
            .take(height)
            .collect::<Vec<_>>();
        self.geometry.navigation_rows = visible_rows.iter().map(|row| row.node_index).collect();
        let lines = visible_rows
            .into_iter()
            .map(|row| row.line)
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Text::from(lines)).style(theme::style(theme::StyleRole::OutlineSurface)),
            navigation_area,
        );
        self.draw_navigation_scrollbar(frame, navigation_area, row_count, height);
    }

    pub(in crate::app) fn outline_rows(
        &self,
        visible: &[usize],
        width: u16,
        height: usize,
    ) -> Vec<navigation::NavigationRow> {
        let tree = navigation::TreePlan::new(self.session.document.navigation())
            .with_entry_labels(self.session.document.entry_labels());
        let expanded = self.effective_expanded();
        let presentation = navigation::search::OutlinePresentation::new(
            if self.outline_search.input.highlighted {
                &self.outline_search.matches
            } else {
                &[]
            },
            self.outline_search.active_match,
        );
        // Build against the scrollbar-bearing width first. If those rows fit,
        // the full-width layout cannot overflow and no gutter is necessary.
        // When they do overflow, every label is laid out one column earlier so
        // the scrollbar never replaces its final cell (or half of a wide one).
        let gutter_width = width.saturating_sub(1);
        let labels = navigation::search::OutlineLabels {
            selected: self.selected,
            full: self.full_outline_labels,
        };
        let gutter_rows = presentation.rows(
            &tree,
            visible,
            labels,
            &expanded,
            usize::from(gutter_width),
            self.session.document.reference_badges(),
        );
        if gutter_rows.len() > height {
            gutter_rows
        } else {
            presentation.rows(
                &tree,
                visible,
                labels,
                &expanded,
                usize::from(width),
                self.session.document.reference_badges(),
            )
        }
    }

    fn draw_navigation_header(&self, frame: &mut Frame<'_>, area: Rect) {
        let metadata = sidebar_metadata(
            self.session.document.navigation().len(),
            self.session.document.has_tldr(),
            area.width,
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!(
                        " {} · {}",
                        sanitize_terminal_text(self.session.document.source_label()),
                        sanitize_terminal_text(self.session.document.label())
                    ),
                    Style::default().fg(theme::SUBTEXT_BRIGHT),
                )),
                Line::from(Span::styled(metadata, Style::default().fg(theme::SUBTEXT))),
            ])
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(theme::BORDER)),
            )
            .style(theme::style(theme::StyleRole::OutlineSurface)),
            area,
        );
    }

    fn draw_navigation_scrollbar(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        row_count: usize,
        viewport_height: usize,
    ) {
        self.geometry.navigation_scrollbar =
            VerticalScrollbar::new(area, row_count, viewport_height, self.navigation_scroll);
        if let Some(scrollbar) = self.geometry.navigation_scrollbar {
            scrollbar.render(frame);
        } else if matches!(self.pointer.drag(), PointerDrag::NavigationScrollbar(_)) {
            self.pointer.finish_drag();
        }
    }
}

pub(in crate::app) fn sidebar_metadata(node_count: usize, has_tldr: bool, width: u16) -> String {
    let suffix = if has_tldr { " · TLDR" } else { "" };
    let candidates = [
        format!(" {node_count} outline nodes{suffix}"),
        format!(" {node_count} nodes{suffix}"),
        format!(" {node_count}{suffix}"),
    ];
    let available = usize::from(width);
    candidates
        .into_iter()
        .find(|candidate| candidate.width() <= available)
        .unwrap_or_else(|| {
            if has_tldr && available >= " TLDR".width() {
                " TLDR".to_owned()
            } else {
                fit_to_width(&format!(" {node_count}"), available)
            }
        })
}
