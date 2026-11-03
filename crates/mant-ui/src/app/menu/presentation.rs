//! Menu and help painting with the dimensions also used by mouse hit routing.
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::super::{App, Overlay, fit_to_width};
use super::{MenuAction, MenuId, menu_entries};
use crate::theme;

pub(super) fn menu_overlay_width(id: MenuId) -> u16 {
    const PREFIX_WIDTH: usize = 4;
    const HORIZONTAL_MARGIN: usize = 2;
    let content_width = menu_entries(id)
        .iter()
        .map(|entry| {
            let shortcut_gap = usize::from(!entry.shortcut.is_empty());
            PREFIX_WIDTH + entry.label.width() + shortcut_gap + entry.shortcut.width()
        })
        .max()
        .unwrap_or_default();
    u16::try_from(content_width + HORIZONTAL_MARGIN)
        .unwrap_or(u16::MAX)
        .max(30)
}

pub(super) fn menu_overlay_left(id: MenuId, terminal_width: u16) -> u16 {
    id.left()
        .min(terminal_width.saturating_sub(menu_overlay_width(id)))
}

impl App {
    pub(in crate::app) fn draw_menu(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let style = Style::default().bg(theme::MENU);
        frame.render_widget(Block::default().style(style), area);
        let open_menu = match self.overlay {
            Overlay::Menu { id, .. } => Some(id),
            Overlay::None | Overlay::DocumentFinder | Overlay::Help | Overlay::References(_) => {
                None
            }
        };
        let spans = MenuId::ALL
            .into_iter()
            .flat_map(|id| {
                let active = open_menu == Some(id);
                let style = if active {
                    style.fg(theme::SELECTED_TEXT).bg(theme::SELECTED)
                } else {
                    style.fg(theme::SUBTEXT_BRIGHT)
                };
                // Labels have ASCII initials. Styling, unlike inserting a
                // marker, keeps the established tab and mouse-hit geometry.
                let (initial, rest) = id.label().split_at(1);
                [
                    Span::styled(" ", style),
                    Span::styled(initial, style.add_modifier(Modifier::UNDERLINED)),
                    Span::styled(format!("{rest} "), style),
                ]
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(Line::from(spans)).style(style), area);
        self.draw_document_tabs(frame, area, style);
    }

    pub(in crate::app) fn draw_menu_hint(frame: &mut Frame<'_>, area: Rect) {
        let full = " ←/→ menus · ↑/↓ items · Enter/Space run · Esc/F10 return";
        let hint = if full.width() <= usize::from(area.width) {
            full
        } else {
            " ←→ ↑↓ Enter Esc"
        };
        let style = Style::default().fg(theme::SUBTEXT_BRIGHT).bg(theme::MENU);
        frame.render_widget(Block::default().style(style), area);
        frame.render_widget(Paragraph::new(hint).style(style), area);
    }

    pub(in crate::app) fn draw_overlay(&mut self, frame: &mut Frame<'_>) {
        if self.overlay != Overlay::DocumentFinder {
            self.clear_finder_geometry();
        }
        match self.overlay {
            Overlay::None => {}
            Overlay::Menu { id, cursor } => self.draw_menu_overlay(frame, id, cursor),
            Overlay::DocumentFinder => self.draw_document_finder(frame),
            Overlay::Help => self.draw_help(frame),
            Overlay::References(_) => self.draw_reference_chooser(frame),
        }
    }

    fn draw_menu_overlay(&self, frame: &mut Frame<'_>, id: MenuId, cursor: usize) {
        let entries = menu_entries(id);
        let height = u16::try_from(entries.len()).unwrap_or_default();
        let area = Rect::new(
            menu_overlay_left(id, frame.area().width),
            1,
            menu_overlay_width(id).min(frame.area().width),
            height.min(frame.area().height.saturating_sub(1)),
        );
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::BASE)),
            area,
        );
        for (index, entry) in entries.iter().enumerate().take(usize::from(area.height)) {
            let row = Rect::new(
                area.x,
                area.y + u16::try_from(index).unwrap_or_default(),
                area.width,
                1,
            );
            let content = row.inner(Margin {
                horizontal: 1,
                vertical: 0,
            });
            let active = index == cursor;
            let prefix = match entry.action {
                MenuAction::ToggleSidebar if self.show_sidebar => "[x] ",
                MenuAction::ToggleFullOutlineLabels if self.full_outline_labels => "[x] ",
                MenuAction::ToggleSidebar | MenuAction::ToggleFullOutlineLabels => "[ ] ",
                _ => "    ",
            };
            let label = format!("{prefix}{}", entry.label);
            let gap = usize::from(content.width)
                .saturating_sub(label.width())
                .saturating_sub(entry.shortcut.width());
            let value = fit_to_width(
                &format!(
                    "{label}{}{shortcut}",
                    " ".repeat(gap),
                    shortcut = entry.shortcut
                ),
                usize::from(content.width),
            );
            let style = if active {
                Style::default()
                    .fg(theme::SELECTED_TEXT)
                    .bg(theme::SELECTED)
            } else {
                Style::default().fg(theme::TEXT).bg(theme::BASE)
            };
            frame.render_widget(Block::default().style(style), row);
            frame.render_widget(Paragraph::new(Span::styled(value, style)), content);
        }
    }

    fn draw_help(&mut self, frame: &mut Frame<'_>) {
        let lines = vec![
            Line::styled(
                "Keyboard Shortcuts",
                Style::default()
                    .fg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::raw("q            quit"),
            Line::raw("F10 / Alt+M/E/V/N/S/H  open a menu"),
            Line::raw("M/E/V/N/S/H  switch menus while a menu is open"),
            Line::raw("←/→ menus · ↑/↓ items · Enter/Space run"),
            Line::raw("Esc / F10    close menu and return"),
            Line::raw("Tab / Shift+Tab  switch Outline / Content focus"),
            Line::raw("↑/↓ or j/k  select node / scroll focused Content"),
            Line::raw("←/→ or h/l  move through the focused Outline"),
            Line::raw("Enter (Outline)  open reference / fold node"),
            Line::raw("Space (Outline)  fold or unfold selected node"),
            Line::raw("Ctrl+O       find and open a document"),
            Line::raw("top tabs     switch opened documents"),
            Line::raw("Alt+←/→      back / forward"),
            Line::raw("Ctrl+F / / · ?  find forward / backward in page"),
            Line::raw("Ctrl+G       find outline titles, entries and links"),
            Line::raw("n / N        repeat search direction / reverse"),
            Line::raw("search       Regex + case-sensitive by default"),
            Line::raw("Ctrl+R (Search)  toggle Regex / Literal"),
            Line::raw("Alt+C (Search)  Sensitive / Ignore / Smart case"),
            Line::raw("Alt+W (Search)  toggle wrap (default off)"),
            Line::raw("drag / Shift+click  select+copy / extend"),
            Line::raw("right-click   copy selected plain text"),
            Line::raw("y / Ctrl+Shift+C  copy selected plain text"),
            Line::raw("Shift+Y      copy selected reference target"),
            Line::raw("O            choose associated reference to open"),
            Line::raw("r (chooser)  reveal occurrence source; Esc returns"),
            Line::raw("g/G · Home/End  first/last in focused region"),
            Line::raw("d/u · Ctrl+d/u  scroll Content by half a viewport"),
            Line::raw("Space/f/b (Content) · PgDn/PgUp  page down/up"),
            Line::raw("search       live highlights; Enter selects and returns"),
            Line::raw("F1           help · Esc cancels / clears highlights"),
            Line::raw("B (uppercase)  toggle Outline sidebar"),
        ];
        let width = 76.min(frame.area().width.saturating_sub(2));
        let height = u16::try_from(lines.len())
            .unwrap_or(u16::MAX)
            .saturating_add(5)
            .min(frame.area().height);
        if width < 4 || height < 3 {
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
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::BLUE))
            .style(Style::default().bg(theme::BASE));
        let inner = block.inner(area).inner(Margin {
            horizontal: 1,
            vertical: 1,
        });
        frame.render_widget(block, area);
        let [body, footer] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
        self.help_scroll = self
            .help_scroll
            .min(lines.len().saturating_sub(usize::from(body.height)));
        frame.render_widget(
            Paragraph::new(lines)
                .scroll((u16::try_from(self.help_scroll).unwrap_or_default(), 0))
                .style(Style::default().fg(theme::TEXT).bg(theme::BASE)),
            body,
        );
        frame.render_widget(
            Paragraph::new(if footer.width >= 55 {
                "↑/↓ scroll · Esc or ? closes this window"
            } else {
                "Esc or ? closes this window"
            })
            .style(Style::default().fg(theme::SUBTEXT).bg(theme::BASE)),
            footer,
        );
    }
}
