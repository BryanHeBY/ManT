//! Status and search bars, including query coverage and error feedback.
use crate::app::{App, fit_to_width};
use crate::{text::sanitize_terminal_text, theme};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use unicode_width::UnicodeWidthStr;
struct SearchFeedback {
    text: String,
    pending: bool,
    limited: bool,
    missing: bool,
}

impl App {
    pub(super) fn draw_status(&self, frame: &mut Frame<'_>, area: Rect) {
        let current = self
            .session
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
        let position = format!(" {}/{} · {current}", selected_position, visible.len());
        let ordinary_budget = usize::from(area.width)
            .saturating_sub(position.width().min(usize::from(area.width) / 3) + 1);
        let (suffix, important) = if let Some(notice) = &self.notice {
            (
                format!(
                    "{} ",
                    sanitize_terminal_text(notice.lines().next().unwrap_or_default())
                ),
                true,
            )
        } else if !self.session.document.content_complete() {
            ("Warning: source content is incomplete ".into(), true)
        } else if self.search.job.is_some() {
            (
                format!("Searching · {} found ", self.search.scope_matches.len()),
                true,
            )
        } else if !self.search.query.is_empty() && !self.search.scope_matches.is_empty() {
            (
                format!(
                    "{} · Find “{}” · {}{} matches ",
                    self.focus_label(),
                    self.search.query,
                    self.search.scope_matches.len(),
                    if self.search.limited {
                        "+ (partial)"
                    } else {
                        ""
                    },
                ),
                true,
            )
        } else if self
            .session
            .document
            .navigation()
            .get(self.selected)
            .is_some_and(|node| {
                self.session
                    .document
                    .reference_badges()
                    .contains_key(&node.id)
            })
        {
            (
                format!(
                    "{} · O: Open Reference · Y: Copy Target ",
                    self.focus_label()
                ),
                true,
            )
        } else {
            (self.reading_hint(ordinary_budget), false)
        };
        let suffix_width = suffix.width().min(usize::from(area.width));
        let left_width = usize::from(area.width).saturating_sub(suffix_width + 1);
        frame.render_widget(
            Paragraph::new(fit_to_width(&position, left_width)).style(style.fg(theme::TEXT)),
            Rect::new(
                area.x,
                area.y,
                u16::try_from(left_width).unwrap_or_default(),
                area.height,
            ),
        );
        frame.render_widget(
            Paragraph::new(suffix)
                .alignment(Alignment::Right)
                .style(if important {
                    style.fg(theme::PEACH)
                } else if !self.reading_focus_visible() {
                    style.patch(theme::style(theme::StyleRole::Metadata))
                } else {
                    style.patch(theme::style(theme::StyleRole::PaneFocus))
                }),
            Rect::new(
                area.x
                    + area
                        .width
                        .saturating_sub(u16::try_from(suffix_width).unwrap_or_default()),
                area.y,
                u16::try_from(suffix_width).unwrap_or_default(),
                area.height,
            ),
        );
    }

    pub(super) fn draw_search(&self, frame: &mut Frame<'_>, area: Rect) {
        let input = self.active_search_input();
        let style = Style::default().bg(theme::MENU);
        frame.render_widget(Block::default().style(style), area);
        let (before_cursor, after_cursor) = input.draft.split_at(input.cursor);
        let cursor_character = after_cursor.chars().next();
        let cursor_bytes = cursor_character.map_or(0, char::len_utf8);
        let after_cursor = &after_cursor[cursor_bytes..];
        let prefix = self.search_prompt_for_width(area.width);
        let prefix_width = prefix.width();
        let prompt = format!("{prefix}{}", input.draft);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(prefix, style.fg(theme::YELLOW)),
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
        let feedback = self.search_feedback();
        let suffix_style = if feedback.missing {
            style
                .fg(theme::BASE)
                .bg(theme::PEACH)
                .add_modifier(Modifier::BOLD)
        } else {
            style.fg(theme::SUBTEXT)
        };
        if prompt.width() + feedback.text.width() < usize::from(area.width) {
            frame.render_widget(
                Paragraph::new(Span::styled(feedback.text, suffix_style))
                    .alignment(Alignment::Right),
                area,
            );
        } else if feedback.limited
            || feedback.pending
            || input.input_limited
            || input.error.is_some()
        {
            // Coverage and input bounds remain visible behind a long query.
            let mut label = if input.error.is_some() {
                " invalid regex "
            } else if feedback.limited {
                " partial "
            } else if input.input_limited {
                " query limit "
            } else {
                " searching "
            };
            let available = usize::from(area.width).saturating_sub(prefix_width + 1);
            if label.width() > available {
                label = if input.error.is_some() {
                    " ERR "
                } else {
                    " + "
                };
            }
            if label.width() > available {
                return;
            }
            let width = u16::try_from(label.width()).unwrap_or_default();
            frame.render_widget(
                Paragraph::new(label).style(suffix_style),
                Rect::new(
                    area.right().saturating_sub(width),
                    area.y,
                    width,
                    area.height,
                ),
            );
        }
    }

    fn search_feedback(&self) -> SearchFeedback {
        let input = self.active_search_input();
        let outline = self.search_target == crate::app::search::SearchTarget::Outline;
        let (count, active) = if outline {
            (
                self.outline_search.matches.len(),
                self.outline_search.active_match,
            )
        } else {
            (self.search.scope_matches.len(), self.search.active_match)
        };
        let pending = if outline {
            self.outline_search.job.is_some()
        } else {
            self.search.job.is_some()
        };
        let limited = if outline {
            self.outline_search.limited
        } else {
            self.search.limited
        };
        let effective_query = if input.is_editing() {
            &input.draft
        } else {
            &input.query
        };
        let mut suffix = if let Some(error) = &input.error {
            format!(
                " Invalid regex: {} · Edit query ",
                error.lines().last().unwrap_or("invalid pattern")
            )
        } else if input.input_limited {
            " Query limit: 4096 bytes · Edit query · Esc close ".to_owned()
        } else if pending {
            format!(" Searching · {count} found · Enter select · Esc close ")
        } else if !effective_query.is_empty() {
            if count == 0 {
                if outline && self.outline_search.limited {
                    " No indexed matches · Esc close ".to_owned()
                } else {
                    " No matches · Edit query · Esc close ".to_owned()
                }
            } else {
                format!(
                    " {}/{}{} · {} · Esc close ",
                    active + 1,
                    count,
                    if outline { " nodes" } else { "" },
                    "Enter select",
                )
            }
        } else {
            " Type to search · Esc close ".to_owned()
        };
        if limited {
            suffix.push_str("· partial ");
        }
        SearchFeedback {
            text: suffix,
            pending,
            limited,
            missing: input.error.is_none() && count == 0 && !pending && !effective_query.is_empty(),
        }
    }
}
