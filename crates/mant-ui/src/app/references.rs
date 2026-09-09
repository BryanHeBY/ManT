//! Explicit target selection and source reveal for owner-associated references.
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::{App, Overlay, UpdateOutcome, fit_to_width};
use crate::{CopyRequest, theme};

pub(super) struct ReferenceChooser {
    pub(super) choices: Vec<(String, String)>,
    pub(super) selected: usize,
    copy: bool,
    pub(super) area: Rect,
    first: usize,
    /// Extended-grapheme offset into the selected label, never a scalar offset.
    horizontal: usize,
}

impl ReferenceChooser {
    fn layout(&mut self, size: Rect) -> Rect {
        let width = size.width.saturating_sub(2).min(110);
        let height = size
            .height
            .saturating_sub(2)
            .min(u16::try_from(self.choices.len() + 3).unwrap_or(u16::MAX));
        self.area = Rect::new(
            size.x + (size.width - width) / 2,
            size.y + (size.height - height) / 2,
            width,
            height,
        );
        let rows = usize::from(height.saturating_sub(3));
        self.first = self.first.min(self.selected);
        if self.selected >= self.first + rows {
            self.first = self.selected.saturating_sub(rows.saturating_sub(1));
        }
        self.area
    }
}

impl App {
    pub(super) fn queue_reference_copy(&mut self, id: &str) {
        let Some(text) = self.session.document.reference_uri(id) else {
            self.report_notice("Reference target has no reusable link address".into());
            return;
        };
        if text.len() > crate::MAX_COPY_BYTES {
            self.report_notice("The reference exceeds the 4 MiB clipboard limit".into());
            return;
        }
        // URI conversion has validated/encoded every component. Terminal
        // display sanitization must never rewrite a copied destination.
        self.pending_copy = Some(CopyRequest::Reference { text });
    }

    pub(super) fn show_reference_chooser(&mut self, copy: bool) {
        let Some(node) = self.session.document.navigation().get(self.selected) else {
            return;
        };
        let mut choices = self.session.document.associated_reference_choices(&node.id);
        if choices.is_empty()
            && let Some(text) = self.session.document.reference_text(&node.id)
        {
            choices.push((node.id.clone(), text));
        }
        if choices.is_empty() {
            self.report_notice("Select a linked heading, entry, or document reference".into());
            return;
        }
        self.reference_chooser = Some(ReferenceChooser {
            choices,
            selected: 0,
            copy,
            area: Rect::default(),
            first: 0,
            horizontal: 0,
        });
        self.overlay = Overlay::References;
    }

    pub(super) fn handle_reference_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                self.reference_chooser = None;
            }
            KeyCode::Enter => self.choose_reference(false),
            KeyCode::Char('r') => self.choose_reference(true),
            KeyCode::Down | KeyCode::Char('j') => self.move_reference_choice(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_reference_choice(-1),
            KeyCode::Left | KeyCode::Right => {
                if let Some(chooser) = &mut self.reference_chooser {
                    let label =
                        crate::text::sanitize_terminal_text(&chooser.choices[chooser.selected].1);
                    let span = Span::raw(label);
                    let limit = span
                        .styled_graphemes(Style::default())
                        .count()
                        .saturating_sub(1);
                    chooser.horizontal = if key.code == KeyCode::Left {
                        chooser.horizontal.saturating_sub(12)
                    } else {
                        chooser.horizontal.saturating_add(12).min(limit)
                    };
                }
            }
            _ => {}
        }
    }

    fn move_reference_choice(&mut self, delta: isize) {
        if let Some(chooser) = &mut self.reference_chooser {
            chooser.selected = chooser
                .selected
                .saturating_add_signed(delta)
                .min(chooser.choices.len().saturating_sub(1));
            chooser.horizontal = 0;
        }
    }

    fn choose_reference(&mut self, reveal: bool) {
        let Some(chooser) = self.reference_chooser.take() else {
            return;
        };
        let Some((id, _)) = chooser.choices.get(chooser.selected) else {
            return;
        };
        self.overlay = Overlay::None;
        if reveal {
            // This is a private, already scanned source coordinate, not a
            // public fragment selector requiring another identity lookup.
            let owner = self.selected;
            if self.reveal_anchor(id) {
                // Associated occurrences intentionally have no permanent tree
                // row. Revealing their source must not jump selection outward
                // to the containing section merely because that row is absent.
                self.set_selected_index(owner);
            }
        } else if chooser.copy {
            self.queue_reference_copy(id);
        } else if let Some(target) = self.session.document.reference_target(id) {
            if let Some(target) = self.session.document.activation_target(target) {
                self.activate_link_target(target);
            } else {
                self.report_notice(
                    "This reference has no registered document context; its target was not opened"
                        .into(),
                );
            }
        }
    }

    pub(super) fn handle_reference_mouse(&mut self, mouse: MouseEvent) -> UpdateOutcome {
        if mouse.kind == MouseEventKind::ScrollDown {
            self.move_reference_choice(1);
        }
        if mouse.kind == MouseEventKind::ScrollUp {
            self.move_reference_choice(-1);
        }
        let Some(chooser) = &mut self.reference_chooser else {
            return UpdateOutcome::Unchanged;
        };
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && chooser.area.contains((mouse.column, mouse.row).into())
        {
            let row = mouse.row.saturating_sub(chooser.area.y);
            if row > 0 && row < chooser.area.height.saturating_sub(2) {
                chooser.selected =
                    (chooser.first + usize::from(row - 1)).min(chooser.choices.len() - 1);
                chooser.horizontal = 0;
            } else if row == chooser.area.height.saturating_sub(2) {
                let reveal = mouse.column >= chooser.area.x + chooser.area.width / 2;
                self.choose_reference(reveal);
            }
        }
        UpdateOutcome::Redraw
    }

    pub(super) fn draw_reference_chooser(&mut self, frame: &mut Frame<'_>) {
        let Some(chooser) = &mut self.reference_chooser else {
            return;
        };
        let area = chooser.layout(frame.area());
        let Rect { width, height, .. } = area;
        let rows = usize::from(height.saturating_sub(3));
        let style = Style::default().bg(theme::MENU).fg(theme::LINK);
        let title = if self.session.document.references_limited() {
            "References · known sources only; inventory limited"
        } else {
            "References · ←/→ inspect · choose target or reveal source"
        };
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(style),
            area,
        );
        let content_width = usize::from(width.saturating_sub(2));
        let lines = chooser
            .choices
            .iter()
            .enumerate()
            .skip(chooser.first)
            .take(rows)
            .map(|(index, (_, label))| {
                Line::styled(
                    reference_choice_text(
                        label,
                        index == chooser.selected,
                        if index == chooser.selected {
                            chooser.horizontal
                        } else {
                            0
                        },
                        content_width,
                    ),
                    if index == chooser.selected {
                        style.bg(theme::SELECTED)
                    } else {
                        style
                    },
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines),
            Rect::new(
                area.x.saturating_add(1),
                area.y.saturating_add(1),
                width.saturating_sub(2),
                height.saturating_sub(3),
            ),
        );
        if height >= 3 {
            let action = if chooser.copy {
                "Enter: Copy"
            } else {
                "Enter: Open"
            };
            let half = content_width / 2;
            let footer = format!(
                "{}{}",
                fit_to_width(action, half),
                fit_to_width("r: Reveal · Esc: Back", content_width - half)
            );
            frame.render_widget(
                Paragraph::new(footer).style(style),
                Rect::new(
                    area.x.saturating_add(1),
                    area.y + height - 2,
                    width.saturating_sub(2),
                    1,
                ),
            );
        }
    }
}

fn reference_choice_text(label: &str, selected: bool, horizontal: usize, width: usize) -> String {
    let mut text = String::new();
    if width > 0 {
        text.push(if selected { '›' } else { ' ' });
    }
    if width > 1 {
        text.push(' ');
    }
    let mut used = width.min(2);
    let label = crate::text::sanitize_terminal_text(label);
    let span = Span::raw(label);
    for grapheme in span.styled_graphemes(Style::default()).skip(horizontal) {
        let columns = grapheme.symbol.width();
        if used + columns > width {
            break;
        }
        text.push_str(grapheme.symbol);
        used += columns;
    }
    text.push_str(&" ".repeat(width.saturating_sub(used)));
    text
}
