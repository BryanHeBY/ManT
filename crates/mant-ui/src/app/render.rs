//! Composes the application frame and records geometry for later input events.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::{
    App, PointerDrag,
    finder::{FinderTreeRow, document_category},
    fit_to_width,
};
use crate::{
    layout::{
        CONTENT_MARGIN, CONTENT_SCROLLBAR_GAP, MIN_CONTENT_WIDTH, MIN_SIDEBAR_WIDTH,
        SIDEBAR_SPLITTER_WIDTH, maximum_sidebar_width,
    },
    navigation,
    scrollbar::VerticalScrollbar,
    text::sanitize_terminal_text,
    theme,
};

impl App {
    /// Render one complete frame and retain its geometry for input hit testing.
    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let [menu_area, body_area, status_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        self.draw_menu(frame, menu_area);
        self.geometry.body = body_area;
        if self.show_sidebar
            && body_area.width > MIN_SIDEBAR_WIDTH + SIDEBAR_SPLITTER_WIDTH + MIN_CONTENT_WIDTH
        {
            self.sidebar_width = self
                .sidebar_width
                .clamp(MIN_SIDEBAR_WIDTH, maximum_sidebar_width(body_area.width));
            let [navigation_area, splitter_area, content_area] = Layout::horizontal([
                Constraint::Length(self.sidebar_width),
                Constraint::Length(SIDEBAR_SPLITTER_WIDTH),
                Constraint::Min(1),
            ])
            .areas(body_area);
            self.draw_navigation(frame, navigation_area);
            self.draw_sidebar_splitter(frame, splitter_area);
            self.draw_content(frame, content_area);
        } else {
            self.geometry.navigation = Rect::default();
            self.geometry.navigation_scrollbar = None;
            self.geometry.sidebar_splitter = Rect::default();
            self.geometry.navigation_rows.clear();
            if matches!(
                self.pointer_drag,
                PointerDrag::Sidebar | PointerDrag::NavigationScrollbar(_)
            ) {
                self.sidebar_resize.cancel();
                self.pointer_drag = PointerDrag::None;
            }
            self.draw_content(frame, body_area);
        }
        if self.search.is_open() {
            self.draw_search(frame, status_area);
        } else {
            self.draw_status(frame, status_area);
        }
        self.geometry.status = status_area;
        self.draw_overlay(frame);
        self.draw_copy_toast(frame);
    }

    fn draw_copy_toast(&self, frame: &mut Frame<'_>) {
        let Some(toast) = &self.copy_toast else {
            return;
        };
        let message = sanitize_terminal_text(&toast.message);
        let width = u16::try_from(message.width().saturating_add(4))
            .unwrap_or(u16::MAX)
            .min(frame.area().width);
        if width < 4 || frame.area().height < 4 {
            return;
        }
        let area = Rect::new(
            frame.area().x + frame.area().width.saturating_sub(width) / 2,
            frame.area().y + frame.area().height.saturating_sub(4),
            width,
            3,
        );
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(message)
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme::GREEN))
                        .style(Style::default().bg(theme::BASE)),
                )
                .style(Style::default().fg(theme::TEXT).bg(theme::BASE)),
            area,
        );
    }

    fn draw_sidebar_splitter(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.geometry.sidebar_splitter = area;
        frame.render_widget(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(theme::BORDER))
                .style(Style::default().bg(theme::SIDEBAR)),
            area,
        );
    }

    fn draw_navigation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::SIDEBAR)),
            area,
        );
        let [header_area, outline_label_area, navigation_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(area);

        let metadata = sidebar_metadata(
            self.document.navigation().len(),
            self.document.has_tldr(),
            navigation_area.width,
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!(
                        " {} · {}",
                        sanitize_terminal_text(self.document.source_label()),
                        sanitize_terminal_text(self.document.label())
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
            .style(Style::default().bg(theme::SIDEBAR)),
            header_area,
        );
        frame.render_widget(
            Paragraph::new(" OUTLINE")
                .style(Style::default().fg(theme::SUBTEXT).bg(theme::SIDEBAR)),
            outline_label_area,
        );

        self.geometry.navigation = navigation_area;
        let visible = self.visible_navigation_indices();
        let line_width = usize::from(navigation_area.width);
        let rows = navigation::rows(
            self.document.navigation(),
            &visible,
            self.selected,
            &self.expanded,
            line_width,
        );
        let row_count = rows.len();
        let height = usize::from(navigation_area.height);
        let maximum = row_count.saturating_sub(height);
        self.navigation_scroll = self.navigation_scroll.min(maximum);
        let selected_range = (self.navigation_visibility_target.take() == Some(self.selected))
            .then(|| navigation::node_row_range(&rows, self.selected))
            .flatten();
        if let Some(range) = selected_range {
            self.keep_selected_navigation_visible(range, height);
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
            Paragraph::new(Text::from(lines)).style(Style::default().bg(theme::SIDEBAR)),
            navigation_area,
        );
        self.draw_navigation_scrollbar(frame, navigation_area, row_count, height);
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
        } else if matches!(self.pointer_drag, PointerDrag::NavigationScrollbar(_)) {
            self.pointer_drag = PointerDrag::None;
        }
    }

    fn draw_content(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::CONTENT)),
            area,
        );
        let inner = area.inner(CONTENT_MARGIN);
        let viewport_height = usize::from(inner.height);
        // The scrollbar owns a gutter beside the document instead of
        // overwriting its final column. This is especially visible on the
        // right border of full-width TLDR panels.
        let scrollbar_gutter = CONTENT_SCROLLBAR_GAP.saturating_add(1);
        // Test the narrower, scrollbar-bearing layout first. Almost every
        // manual needs its virtual trailing rows, so this avoids rendering a
        // second full-width document merely to discover that fact.
        let sizing_width = inner.width.saturating_sub(scrollbar_gutter).max(1);
        let sizing_rows = self
            .rendered_cache
            .entry(sizing_width)
            .or_insert_with(|| self.document.render(sizing_width))
            .row_count;
        let needs_scrollbar = virtual_content_rows(sizing_rows, viewport_height) > viewport_height;
        let document_area = if needs_scrollbar && inner.width > scrollbar_gutter {
            Rect::new(
                inner.x,
                inner.y,
                inner.width.saturating_sub(scrollbar_gutter),
                inner.height,
            )
        } else {
            inner
        };
        self.geometry.content = document_area;
        let render_width = document_area.width.max(1);
        if self.content_render_width != 0 && self.content_render_width != render_width {
            self.selection = None;
            if matches!(self.pointer_drag, PointerDrag::ContentSelection { .. }) {
                self.pointer_drag = PointerDrag::None;
            }
        }
        let viewport_anchor = (self.content_render_width != 0
            && self.content_render_width != render_width)
            .then(|| {
                self.rendered_cache
                    .get(&self.content_render_width)
                    .and_then(|rendered| rendered.viewport_anchor(self.content_scroll))
            })
            .flatten();
        self.rendered_cache
            .entry(render_width)
            .or_insert_with(|| self.document.render(render_width));
        if !self.search.query.is_empty() && self.search.render_width != render_width {
            self.refresh_search(render_width);
        }
        let rendered = &self.rendered_cache[&render_width];
        if let Some(anchor) = viewport_anchor
            && let Some(row) = rendered.row_for_viewport_anchor(anchor)
        {
            self.content_scroll = row;
        }
        self.content_render_width = render_width;
        // Keep enough virtual trailing space for every addressable row,
        // including the final section heading, to become the viewport's first
        // line by retaining a terminal-height spacer after the document.
        let virtual_rows = virtual_content_rows(rendered.row_count, viewport_height);
        let maximum = virtual_rows.saturating_sub(viewport_height);
        self.content_scroll = self.content_scroll.min(maximum);
        let matches = if self.search.query.is_empty() {
            &[]
        } else {
            self.search.matches.as_slice()
        };
        let text = rendered.viewport_text(
            self.content_scroll,
            viewport_height,
            matches,
            self.active_rendered_search_match(),
            self.selection,
        );
        frame.render_widget(
            Paragraph::new(text).style(Style::default().bg(theme::CONTENT)),
            document_area,
        );
        self.geometry.content_scrollbar =
            VerticalScrollbar::new(inner, virtual_rows, viewport_height, self.content_scroll);
        if let Some(scrollbar) = self.geometry.content_scrollbar {
            scrollbar.render(frame);
        } else if matches!(self.pointer_drag, PointerDrag::ContentScrollbar(_)) {
            self.pointer_drag = PointerDrag::None;
        }
        // A width-dependent rendering can be large (notably for GCC). Keeping
        // the current width hot is useful; retaining every prior terminal
        // width turns repeated resizing into unbounded growth.
        self.rendered_cache
            .retain(|width, _| *width == render_width);
    }

    fn draw_status(&self, frame: &mut Frame<'_>, area: Rect) {
        let current = self
            .document
            .navigation()
            .get(self.selected)
            .map_or("document", |item| item.title.as_str());
        let current = sanitize_terminal_text(current);
        let visible = self.visible_navigation_indices();
        let selected_position = visible
            .iter()
            .position(|index| *index == self.selected)
            .map_or(0, |index| index + 1);
        let style = Style::default().bg(theme::BASE);
        frame.render_widget(Block::default().style(style), area);
        frame.render_widget(
            Paragraph::new(format!(
                " {}/{} · {current}",
                selected_position,
                visible.len()
            ))
            .style(style.fg(theme::TEXT)),
            area,
        );
        let suffix = if let Some(notice) = &self.notice {
            format!("{} ", notice.lines().next().unwrap_or_default())
        } else if !self.search.query.is_empty() && !self.search.scope_matches.is_empty() {
            format!(
                "Find “{}” · {} matches ",
                self.search.query,
                self.search.scope_matches.len()
            )
        } else if self.document.has_tldr() {
            format!("{} visible nodes · TLDR ", self.visible_node_count())
        } else {
            format!("{} visible nodes ", self.visible_node_count())
        };
        frame.render_widget(
            Paragraph::new(suffix)
                .alignment(Alignment::Right)
                .style(style.fg(if self.notice.is_some() {
                    theme::PEACH
                } else {
                    theme::SUBTEXT
                })),
            area,
        );
    }

    fn draw_search(&self, frame: &mut Frame<'_>, area: Rect) {
        let style = Style::default().bg(theme::MENU);
        frame.render_widget(Block::default().style(style), area);
        let (before_cursor, after_cursor) = self.search.draft.split_at(self.search.cursor);
        let cursor_character = after_cursor.chars().next();
        let cursor_bytes = cursor_character.map_or(0, char::len_utf8);
        let after_cursor = &after_cursor[cursor_bytes..];
        let prompt = format!(" Find: {}", self.search.draft);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Find: ", style.fg(theme::YELLOW)),
                Span::styled(
                    before_cursor.to_owned(),
                    style.fg(theme::TEXT).bg(theme::SURFACE),
                ),
                Span::styled(
                    cursor_character.map_or_else(|| " ".to_owned(), |value| value.to_string()),
                    style.fg(theme::BASE).bg(theme::TEXT),
                ),
                Span::styled(
                    after_cursor.to_owned(),
                    style.fg(theme::TEXT).bg(theme::SURFACE),
                ),
            ])),
            area,
        );
        let suffix = if !self.search.is_editing() && !self.search.query.is_empty() {
            if self.search.scope_matches.is_empty() {
                " No matches · Edit query · Esc close ".to_owned()
            } else {
                format!(
                    " {}/{} · Enter next · Esc close ",
                    self.search.active_match + 1,
                    self.search.scope_matches.len()
                )
            }
        } else {
            " Enter search · Esc cancel ".to_owned()
        };
        let suffix_style = if self.search.scope_matches.is_empty()
            && !self.search.is_editing()
            && !self.search.query.is_empty()
        {
            style
                .fg(theme::BASE)
                .bg(theme::PEACH)
                .add_modifier(Modifier::BOLD)
        } else {
            style.fg(theme::SUBTEXT)
        };
        let prompt_width = prompt.width();
        if prompt_width + suffix.width() < usize::from(area.width) {
            frame.render_widget(
                Paragraph::new(Span::styled(suffix, suffix_style)).alignment(Alignment::Right),
                area,
            );
        }
    }

    pub(super) fn draw_document_finder(&mut self, frame: &mut Frame<'_>) {
        let width = 76.min(frame.area().width.saturating_sub(2));
        let height = 18.min(frame.area().height.saturating_sub(2));
        if width < 24 || height < 7 {
            self.clear_finder_geometry();
            return;
        }
        let area = Rect::new(
            frame.area().x + frame.area().width.saturating_sub(width) / 2,
            frame.area().y + frame.area().height.saturating_sub(height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, area);
        let block = Block::default()
            .title(" Open Document ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::BLUE))
            .style(Style::default().bg(theme::BASE));
        let inner = block.inner(area).inner(Margin {
            horizontal: 1,
            vertical: 0,
        });
        frame.render_widget(block, area);
        let [query_area, hint_area, results_area] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(inner);
        self.geometry.finder_query = query_area;

        let (before_cursor, after_cursor) = self.finder.draft.split_at(self.finder.cursor);
        let cursor_character = after_cursor.chars().next();
        let cursor_bytes = cursor_character.map_or(0, char::len_utf8);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Search: ", Style::default().fg(theme::YELLOW)),
                Span::styled(before_cursor.to_owned(), Style::default().fg(theme::TEXT)),
                Span::styled(
                    cursor_character.map_or_else(|| " ".to_owned(), |value| value.to_string()),
                    Style::default().fg(theme::BASE).bg(theme::TEXT),
                ),
                Span::styled(
                    after_cursor[cursor_bytes..].to_owned(),
                    Style::default().fg(theme::TEXT),
                ),
            ]))
            .style(Style::default().bg(theme::BASE)),
            query_area,
        );
        let result_count = self.finder.total;
        let hint = if self.finder.draft.is_empty() {
            format!("{result_count} documents · click/Enter open/toggle · wheel scroll · Esc close")
        } else {
            format!("{result_count} matches · click/Enter open · wheel scroll · Esc close")
        };
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(theme::SUBTEXT)),
            hint_area,
        );

        self.draw_finder_results(frame, results_area);
    }

    fn draw_finder_results(&mut self, frame: &mut Frame<'_>, results_area: Rect) {
        let visible_height = usize::from(results_area.height);
        let row_count = self.finder.row_count();
        self.finder.ensure_selected_visible(visible_height);
        let scroll = self.finder.scroll;
        self.geometry.finder_results = results_area;
        self.geometry.finder_scrollbar =
            VerticalScrollbar::new(results_area, row_count, visible_height, scroll);
        let row_width = results_area
            .width
            .saturating_sub(u16::from(self.geometry.finder_scrollbar.is_some()));
        for visual_row in 0..visible_height.min(row_count.saturating_sub(scroll)) {
            let row = Rect::new(
                results_area.x,
                results_area.y + u16::try_from(visual_row).unwrap_or_default(),
                row_width,
                1,
            );
            let selected = scroll + visual_row == self.finder.selected;
            let value = if self.finder.draft.is_empty() {
                match &self.finder.tree[scroll + visual_row] {
                    FinderTreeRow::Folder { path, name, depth } => {
                        let marker = if self.finder.expanded(path) {
                            "▾"
                        } else {
                            "▸"
                        };
                        fit_to_width(
                            &format!("{}{marker} {name}/", "  ".repeat(*depth)),
                            usize::from(row.width),
                        )
                    }
                    FinderTreeRow::Document { index, depth } => {
                        let Some(document) = self.finder.catalog.get(*index) else {
                            continue;
                        };
                        fit_to_width(
                            &format!("{}  {}", "  ".repeat(*depth), document.address.name()),
                            usize::from(row.width),
                        )
                    }
                }
            } else {
                let match_index = self.finder.matches[scroll + visual_row];
                let Some(document) = self.finder.catalog.get(match_index) else {
                    continue;
                };
                let category = document_category(&document.address);
                let catalog_path = document.catalog_path();
                let name = sanitize_terminal_text(&catalog_path);
                let gap = usize::from(row.width)
                    .saturating_sub(name.width())
                    .saturating_sub(category.width())
                    .max(1);
                fit_to_width(
                    &format!("{name}{}{category}", " ".repeat(gap)),
                    usize::from(row.width),
                )
            };
            let style = if selected {
                Style::default()
                    .fg(theme::SELECTED_TEXT)
                    .bg(theme::SELECTED)
            } else {
                Style::default().fg(theme::TEXT).bg(theme::BASE)
            };
            frame.render_widget(Paragraph::new(Span::styled(value, style)), row);
        }
        if let Some(scrollbar) = self.geometry.finder_scrollbar {
            scrollbar.render(frame);
        } else if matches!(self.pointer_drag, PointerDrag::FinderScrollbar(_)) {
            self.pointer_drag = PointerDrag::None;
        }
    }

    pub(super) fn clear_finder_geometry(&mut self) {
        self.geometry.finder_query = Rect::default();
        self.geometry.finder_results = Rect::default();
        self.geometry.finder_scrollbar = None;
        if matches!(self.pointer_drag, PointerDrag::FinderScrollbar(_)) {
            self.pointer_drag = PointerDrag::None;
        }
    }
}

pub(super) fn sidebar_metadata(node_count: usize, has_tldr: bool, width: u16) -> String {
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

const fn virtual_content_rows(row_count: usize, viewport_height: usize) -> usize {
    row_count.saturating_add(viewport_height.saturating_sub(1))
}
