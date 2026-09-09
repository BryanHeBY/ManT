//! Explicit target selection and source reveal for owner-associated references.
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use mant_render::cells::{after_graphemes, graphemes};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::{App, Overlay, UpdateOutcome, fit_to_width};
use crate::{CopyRequest, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReferencePurpose {
    Open,
    Copy,
}

#[derive(Clone, Copy)]
enum ReferenceCommand {
    Confirm,
    Reveal,
}

enum ReferenceAction {
    Open(String),
    Copy(String),
    Reveal(String),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ReferenceChooser {
    choices: Vec<(String, String)>,
    selected: usize,
    purpose: ReferencePurpose,
    area: Rect,
    first: usize,
    /// Extended-grapheme offset into the selected label, never a scalar offset.
    horizontal: usize,
}

impl ReferenceChooser {
    fn new(choices: Vec<(String, String)>, purpose: ReferencePurpose) -> Option<Self> {
        (!choices.is_empty()).then_some(Self {
            choices,
            selected: 0,
            purpose,
            area: Rect::default(),
            first: 0,
            horizontal: 0,
        })
    }

    fn finish(self, command: ReferenceCommand) -> Option<ReferenceAction> {
        let (id, _) = self.choices.into_iter().nth(self.selected)?;
        Some(match (command, self.purpose) {
            (ReferenceCommand::Reveal, _) => ReferenceAction::Reveal(id),
            (ReferenceCommand::Confirm, ReferencePurpose::Open) => ReferenceAction::Open(id),
            (ReferenceCommand::Confirm, ReferencePurpose::Copy) => ReferenceAction::Copy(id),
        })
    }

    #[cfg(test)]
    pub(super) fn choices(&self) -> &[(String, String)] {
        &self.choices
    }

    #[cfg(test)]
    pub(super) const fn selected(&self) -> usize {
        self.selected
    }

    #[cfg(test)]
    pub(super) const fn area(&self) -> Rect {
        self.area
    }

    #[cfg(test)]
    pub(super) fn set_label(&mut self, index: usize, label: String) {
        self.choices[index].1 = label;
    }

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

    pub(super) fn show_reference_chooser(&mut self, purpose: ReferencePurpose) {
        let Some(node) = self.session.document.navigation().get(self.selected) else {
            return;
        };
        let mut choices = self.session.document.associated_reference_choices(&node.id);
        if choices.is_empty()
            && let Some(text) = self.session.document.reference_text(&node.id)
        {
            choices.push((node.id.clone(), text));
        }
        let Some(chooser) = ReferenceChooser::new(choices, purpose) else {
            self.report_notice("Select a linked heading, entry, or document reference".into());
            return;
        };
        self.overlay = Overlay::References(chooser);
    }

    pub(super) fn handle_reference_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
            }
            KeyCode::Enter => self.choose_reference(ReferenceCommand::Confirm),
            KeyCode::Char('r') => self.choose_reference(ReferenceCommand::Reveal),
            KeyCode::Down | KeyCode::Char('j') => self.move_reference_choice(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_reference_choice(-1),
            KeyCode::Left | KeyCode::Right => {
                if let Some(chooser) = self.overlay.references_mut() {
                    let label =
                        crate::text::sanitize_terminal_text(&chooser.choices[chooser.selected].1);
                    let limit = graphemes(&label).count().saturating_sub(1);
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
        if let Some(chooser) = self.overlay.references_mut() {
            chooser.selected = chooser
                .selected
                .saturating_add_signed(delta)
                .min(chooser.choices.len().saturating_sub(1));
            chooser.horizontal = 0;
        }
    }

    fn choose_reference(&mut self, command: ReferenceCommand) {
        let Some(action) = self
            .overlay
            .take_references()
            .and_then(|chooser| chooser.finish(command))
        else {
            return;
        };
        match action {
            ReferenceAction::Reveal(id) => {
                // This is a private, already scanned source coordinate, not a
                // public fragment selector requiring another identity lookup.
                let owner = self.selected;
                if self.reveal_anchor(&id) {
                    // Associated occurrences intentionally have no permanent tree
                    // row. Revealing their source must not jump selection outward
                    // to the containing section merely because that row is absent.
                    self.set_selected_index(owner);
                }
            }
            ReferenceAction::Copy(id) => self.queue_reference_copy(&id),
            ReferenceAction::Open(id) => {
                if let Some(target) = self.session.document.reference_target(&id) {
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
        }
    }

    pub(super) fn handle_reference_mouse(&mut self, mouse: MouseEvent) -> UpdateOutcome {
        if mouse.kind == MouseEventKind::ScrollDown {
            self.move_reference_choice(1);
        }
        if mouse.kind == MouseEventKind::ScrollUp {
            self.move_reference_choice(-1);
        }
        let Some(chooser) = self.overlay.references_mut() else {
            return UpdateOutcome::Unchanged;
        };
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            if !chooser.area.contains((mouse.column, mouse.row).into()) {
                // Dismiss the modal but consume this click. It must not also
                // activate a link, tab or menu behind the chooser.
                self.overlay = Overlay::None;
                return UpdateOutcome::Redraw;
            }
            let row = mouse.row.saturating_sub(chooser.area.y);
            if row > 0 && row < chooser.area.height.saturating_sub(2) {
                chooser.selected =
                    (chooser.first + usize::from(row - 1)).min(chooser.choices.len() - 1);
                chooser.horizontal = 0;
            } else if row == chooser.area.height.saturating_sub(2) {
                let command = if mouse.column >= chooser.area.x + chooser.area.width / 2 {
                    ReferenceCommand::Reveal
                } else {
                    ReferenceCommand::Confirm
                };
                self.choose_reference(command);
            }
        }
        UpdateOutcome::Redraw
    }

    pub(super) fn draw_reference_chooser(&mut self, frame: &mut Frame<'_>) {
        let Some(chooser) = self.overlay.references_mut() else {
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
            let action = match chooser.purpose {
                ReferencePurpose::Copy => "Enter: Copy",
                ReferencePurpose::Open => "Enter: Open",
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
    for grapheme in graphemes(after_graphemes(&label, horizontal)) {
        let columns = grapheme.columns();
        if used + columns > width {
            break;
        }
        text.push_str(grapheme.text());
        used += columns;
    }
    text.push_str(&" ".repeat(width.saturating_sub(used)));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_overlay_requires_choices_and_reveal_is_independent_of_purpose() {
        for purpose in [ReferencePurpose::Open, ReferencePurpose::Copy] {
            assert!(ReferenceChooser::new(Vec::new(), purpose).is_none());
            for command in [ReferenceCommand::Confirm, ReferenceCommand::Reveal] {
                let mut chooser = ReferenceChooser::new(
                    vec![
                        ("first".into(), "Same target".into()),
                        ("second".into(), "Same target".into()),
                    ],
                    purpose,
                )
                .unwrap();
                assert_eq!(
                    chooser.choices.len(),
                    2,
                    "occurrences are not target deduplication"
                );
                chooser.selected = 1;
                match (command, purpose, chooser.finish(command).unwrap()) {
                    (ReferenceCommand::Reveal, _, ReferenceAction::Reveal(id))
                    | (
                        ReferenceCommand::Confirm,
                        ReferencePurpose::Copy,
                        ReferenceAction::Copy(id),
                    )
                    | (
                        ReferenceCommand::Confirm,
                        ReferencePurpose::Open,
                        ReferenceAction::Open(id),
                    ) => assert_eq!(id, "second"),
                    _ => panic!(
                        "purpose/command must yield the corresponding selected occurrence action"
                    ),
                }
            }
        }
    }
}
