//! Reading-region focus never follows passive outline/scroll synchronization.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::{App, UpdateOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReaderFocus {
    Outline,
    Content,
}

impl App {
    pub(super) fn outline_is_available(&self) -> bool {
        self.viewport_width.map_or(self.show_sidebar, |width| {
            crate::layout::outline_visible(self.show_sidebar, width)
        })
    }

    /// Effective focus also supports hosts drawing a new frame without a
    /// preceding Resize event. Painting derives this fallback, never stores it.
    pub(super) fn reading_focus(&self) -> ReaderFocus {
        if self.outline_is_available() {
            self.focus
        } else {
            ReaderFocus::Content
        }
    }

    fn normalize_reading_focus(&mut self) {
        self.focus = self.reading_focus();
    }

    pub(crate) fn resize_reading_viewport(&mut self, width: u16) {
        self.viewport_width = Some(width);
        self.normalize_reading_focus();
    }

    pub(super) fn focus_label(&self) -> &'static str {
        match self.reading_focus() {
            ReaderFocus::Content => "Content",
            ReaderFocus::Outline => "Outline",
        }
    }
    pub(super) fn focus_pointer(&mut self, mouse: MouseEvent) -> bool {
        let previous = self.focus;
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return false;
        }
        self.normalize_reading_focus();
        if self
            .geometry
            .outline_region
            .contains((mouse.column, mouse.row).into())
            && self.outline_is_available()
        {
            self.focus = ReaderFocus::Outline;
        } else if self
            .geometry
            .content_region
            .contains((mouse.column, mouse.row).into())
        {
            self.focus = ReaderFocus::Content;
        }
        self.focus != previous
    }

    pub(super) fn reading_focus_visible(&self) -> bool {
        self.overlay == super::Overlay::None && !self.search_is_open()
    }

    pub(super) fn reading_hint(&self, width: usize) -> String {
        use unicode_width::UnicodeWidthStr;
        let (name, action, target) = match self.reading_focus() {
            ReaderFocus::Outline => ("Outline", "select", "Content"),
            ReaderFocus::Content => ("Content", "scroll", "Outline"),
        };
        let switch = if self.outline_is_available() && self.geometry.navigation.width > 0 {
            format!(" · Tab: {target}")
        } else {
            String::new()
        };
        let hints = [
            format!(
                "{name} · j/k {action}{switch} · B sidebar · / ? find · Ctrl+G outline · Ctrl+O open · F1 help "
            ),
            format!("{name} · j/k {action}{switch} · F1 help "),
            format!("{name}{switch} · F1 help "),
            format!("{name} · F1 help "),
            format!("{name} "),
        ];
        hints
            .into_iter()
            .find(|hint| hint.width() <= width)
            .unwrap_or_default()
    }

    pub(super) fn reading_key(&mut self, key: KeyEvent) -> Option<UpdateOutcome> {
        self.normalize_reading_focus();
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
            && (key.modifiers - KeyModifiers::SHIFT).is_empty()
        {
            self.focus = match self.focus {
                ReaderFocus::Content
                    if self.outline_is_available() && self.geometry.navigation.width > 0 =>
                {
                    ReaderFocus::Outline
                }
                ReaderFocus::Outline | ReaderFocus::Content => ReaderFocus::Content,
            };
            return Some(UpdateOutcome::Redraw);
        }
        if key.modifiers == KeyModifiers::CONTROL {
            match key.code {
                KeyCode::Char('d' | 'u') => {
                    self.scroll_page(key.code == KeyCode::Char('u'), true);
                    return Some(UpdateOutcome::Redraw);
                }
                _ => return None,
            }
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return None;
        }
        match key.code {
            KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.focused_edge(true);
            }
            KeyCode::Char('B') => self.toggle_sidebar(),
            KeyCode::Char('d') => self.scroll_page(false, true),
            KeyCode::Char('u') => self.scroll_page(true, true),
            KeyCode::PageDown => self.scroll_page(false, false),
            KeyCode::PageUp => self.scroll_page(true, false),
            KeyCode::Char(' ' | 'f') if self.focus == ReaderFocus::Content => {
                self.scroll_page(false, false);
            }
            KeyCode::Char('b') if self.focus == ReaderFocus::Content => {
                self.scroll_page(true, false);
            }
            KeyCode::Home | KeyCode::Char('g') => self.focused_edge(false),
            KeyCode::End | KeyCode::Char('G') => self.focused_edge(true),
            KeyCode::Char('j') | KeyCode::Down if self.focus == ReaderFocus::Content => {
                self.scroll_content(1);
            }
            KeyCode::Char('k') | KeyCode::Up if self.focus == ReaderFocus::Content => {
                self.scroll_content(-1);
            }
            KeyCode::Char('h' | 'l') | KeyCode::Left | KeyCode::Right | KeyCode::Enter
                if self.focus == ReaderFocus::Content =>
            {
                return Some(UpdateOutcome::Unchanged);
            }
            _ => return None,
        }
        Some(UpdateOutcome::Redraw)
    }

    fn focused_edge(&mut self, end: bool) {
        if self.focus == ReaderFocus::Content {
            self.jump_content(end);
        } else {
            let visible = self.visible_navigation_indices();
            if let Some(index) = if end { visible.last() } else { visible.first() } {
                self.set_selected_index(*index);
                self.scroll_to_selected();
            }
        }
    }

    fn scroll_page(&mut self, up: bool, half: bool) {
        let height = usize::from(self.geometry.content.height).max(1);
        let rows = if half {
            (height / 2).max(1)
        } else {
            height.saturating_sub(1).max(1)
        };
        let delta = isize::try_from(rows).unwrap_or(isize::MAX);
        self.scroll_content(if up { -delta } else { delta });
    }

    pub(super) fn toggle_sidebar(&mut self) {
        self.show_sidebar = !self.show_sidebar;
        self.normalize_reading_focus();
    }
}
