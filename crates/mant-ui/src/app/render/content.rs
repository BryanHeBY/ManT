//! Document viewport, reflow and bounded rendered-cache retention.
use crate::app::{App, PointerDrag};
use crate::{
    layout::{CONTENT_MARGIN, CONTENT_SCROLLBAR_GAP},
    scrollbar::VerticalScrollbar,
    theme,
};
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Paragraph},
};
impl App {
    pub(super) fn draw_content(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.geometry.content_region = area;
        frame.render_widget(
            Block::default().style(theme::style(theme::StyleRole::ContentSurface)),
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
            .session
            .rendered_cache
            .entry(sizing_width)
            .or_insert_with(|| std::sync::Arc::new(self.session.document.render(sizing_width)))
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
        if self.session.content_render_width != 0
            && self.session.content_render_width != render_width
        {
            self.pointer.clear_selection();
        }
        let viewport_anchor = (self.session.content_render_width != 0
            && self.session.content_render_width != render_width)
            .then(|| {
                self.session
                    .rendered_cache
                    .get(&self.session.content_render_width)
                    .and_then(|rendered| rendered.viewport_anchor(self.session.content_scroll))
            })
            .flatten();
        self.session
            .rendered_cache
            .entry(render_width)
            .or_insert_with(|| std::sync::Arc::new(self.session.document.render(render_width)));
        if !self.outline_search.input.is_open()
            && (self.search.highlighted || self.search.job.is_some())
            && (!self.search.matching_query.is_empty() || !self.search.query.is_empty())
            && self.search.render_width != render_width
        {
            self.refresh_search(render_width);
        }
        let rendered = &self.session.rendered_cache[&render_width];
        if let Some(anchor) = viewport_anchor
            && let Some(row) = rendered.row_for_viewport_anchor(anchor)
        {
            self.session.content_scroll = row;
        }
        self.session.content_render_width = render_width;
        // Keep enough virtual trailing space for every addressable row,
        // including the final section heading, to become the viewport's first
        // line by retaining a terminal-height spacer after the document.
        let virtual_rows = virtual_content_rows(rendered.row_count, viewport_height);
        let maximum = virtual_rows.saturating_sub(viewport_height);
        self.session.content_scroll = self.session.content_scroll.min(maximum);
        let matches = if self.search.matching_query.is_empty() {
            &[]
        } else {
            self.search.matches.as_slice()
        };
        let text = rendered.viewport_text(
            self.session.content_scroll,
            viewport_height,
            matches,
            self.active_rendered_search_match(),
            self.pointer.selection(),
        );
        frame.render_widget(
            Paragraph::new(text).style(theme::style(theme::StyleRole::ContentSurface)),
            document_area,
        );
        self.geometry.content_scrollbar = VerticalScrollbar::new(
            inner,
            virtual_rows,
            viewport_height,
            self.session.content_scroll,
        );
        if let Some(scrollbar) = self.geometry.content_scrollbar {
            scrollbar.render(frame);
        } else if matches!(self.pointer.drag(), PointerDrag::ContentScrollbar(_)) {
            self.pointer.finish_drag();
        }
        // A width-dependent rendering can be large (notably for GCC). Keeping
        // the current width hot is useful; retaining every prior terminal
        // width turns repeated resizing into unbounded growth.
        self.session
            .rendered_cache
            .retain(|width, _| *width == render_width);
    }
}

const fn virtual_content_rows(row_count: usize, viewport_height: usize) -> usize {
    row_count.saturating_add(viewport_height.saturating_sub(1))
}
