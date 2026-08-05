//! Composes the application frame and records geometry for later input events.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::{App, PointerDrag, fit_to_width};
use crate::{
    layout::{
        CONTENT_MARGIN, CONTENT_SCROLLBAR_GAP, MIN_CONTENT_WIDTH, MIN_SIDEBAR_WIDTH,
        SIDEBAR_SPLITTER_WIDTH, maximum_sidebar_width,
    },
    navigation,
    scrollbar::VerticalScrollbar,
    theme,
};

impl App {
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
                self.pending_sidebar_resize = None;
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
        let [header_area, section_label_area, navigation_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(area);

        let metadata = sidebar_metadata(
            self.document.top_level_count(),
            self.document.section_count(),
            self.document.has_tldr(),
            navigation_area.width,
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!(
                        " {} · {}",
                        self.document.source_label(),
                        self.document.label()
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
            Paragraph::new(" SECTIONS")
                .style(Style::default().fg(theme::SUBTEXT).bg(theme::SIDEBAR)),
            section_label_area,
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
            .then(|| navigation::item_row_range(&rows, self.selected))
            .flatten();
        if let Some(range) = selected_range {
            self.keep_selected_navigation_visible(range, height);
        }
        let visible_rows = rows
            .into_iter()
            .skip(self.navigation_scroll)
            .take(height)
            .collect::<Vec<_>>();
        self.geometry.navigation_rows = visible_rows.iter().map(|row| row.item_index).collect();
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
            (!matches.is_empty()).then_some(self.search.active_match),
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
        let suffix = if !self.search.query.is_empty() && !self.search.matches.is_empty() {
            format!(
                "Find “{}” · {} matches ",
                self.search.query,
                self.search.matches.len()
            )
        } else if self.document.has_tldr() {
            format!("{} visible sections · TLDR ", self.visible_section_count())
        } else {
            format!("{} visible sections ", self.visible_section_count())
        };
        frame.render_widget(
            Paragraph::new(suffix)
                .alignment(Alignment::Right)
                .style(style.fg(theme::SUBTEXT)),
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
            if self.search.matches.is_empty() {
                " No matches · Edit query · Esc close ".to_owned()
            } else {
                format!(
                    " {}/{} · Enter next · Esc close ",
                    self.search.active_match + 1,
                    self.search.matches.len()
                )
            }
        } else {
            " Enter search · Esc cancel ".to_owned()
        };
        let suffix_style = if self.search.matches.is_empty()
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
}

pub(super) fn sidebar_metadata(
    top_level_count: usize,
    section_count: usize,
    has_tldr: bool,
    width: u16,
) -> String {
    let suffix = if has_tldr { " · TLDR" } else { "" };
    let candidates = [
        format!(" {top_level_count} top-level · {section_count} sections{suffix}"),
        format!(" {top_level_count} top · {section_count} sections{suffix}"),
        format!(" {top_level_count} · {section_count} sections{suffix}"),
        format!(" {top_level_count} · {section_count}{suffix}"),
    ];
    let available = usize::from(width);
    candidates
        .into_iter()
        .find(|candidate| candidate.width() <= available)
        .unwrap_or_else(|| {
            if has_tldr && available >= " TLDR".width() {
                " TLDR".to_owned()
            } else {
                fit_to_width(&format!(" {top_level_count} · {section_count}"), available)
            }
        })
}

const fn virtual_content_rows(row_count: usize, viewport_height: usize) -> usize {
    row_count.saturating_add(viewport_height.saturating_sub(1))
}
