//! Real reader input and service actions shared by integration tests.

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mant_ui::{App, CopyRequest, ReaderServices};
use ratatui::buffer::Buffer;
use unicode_width::UnicodeWidthStr;

pub(super) fn pointer(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

/// Locate labels by terminal cells, skipping wide grapheme continuation cells.
/// The caller chooses its own body scope and match. UTF-8 byte length is never
/// used as a terminal-cell or selection width.
pub(super) fn positions(buffer: &Buffer, label: &str) -> Vec<(u16, u16)> {
    let width = u16::try_from(label.width()).unwrap();
    let mut found = Vec::new();
    if width == 0 || width > buffer.area.width {
        return found;
    }
    for row in 1..buffer.area.height.saturating_sub(1) {
        for column in 0..=buffer.area.width.saturating_sub(width) {
            let mut text = String::new();
            let mut cursor = column;
            while cursor < column + width {
                let symbol = buffer[(cursor, row)].symbol();
                text.push_str(symbol);
                cursor += u16::try_from(symbol.width().max(1)).unwrap();
            }
            if text == label {
                found.push((column, row));
            }
        }
    }
    found
}

pub(super) fn click(app: &mut App, column: u16, row: u16) {
    pointer(app, MouseEventKind::Down(MouseButton::Left), column, row);
    pointer(app, MouseEventKind::Up(MouseButton::Left), column, row);
}

/// Perform the existing down/drag/up selection. A caller that needs to enter
/// drag mode for one cell supplies its original intermediate coordinate.
pub(super) fn select_span(
    app: &mut App,
    column: u16,
    row: u16,
    last: u16,
    initial_drag: Option<u16>,
) {
    pointer(app, MouseEventKind::Down(MouseButton::Left), column, row);
    if let Some(initial) = initial_drag {
        pointer(app, MouseEventKind::Drag(MouseButton::Left), initial, row);
    }
    pointer(app, MouseEventKind::Drag(MouseButton::Left), last, row);
    pointer(app, MouseEventKind::Up(MouseButton::Left), last, row);
}

pub(super) fn opened_targets(app: &mut App) -> Vec<String> {
    let mut activated = Vec::new();
    let mut open = |uri: &mant_ui::ExternalUri| {
        activated.push(uri.as_str().to_owned());
        Ok(())
    };
    app.service_pending(&mut ReaderServices {
        open_external: Some(&mut open),
        ..ReaderServices::default()
    });
    activated
}

pub(super) fn copied_selections(
    app: &mut App,
    mut selection_text: impl FnMut(CopyRequest) -> String,
) -> Vec<String> {
    let mut copied = Vec::new();
    let mut copy = |request| {
        copied.push(selection_text(request));
        Ok(())
    };
    app.service_pending(&mut ReaderServices {
        copy_to_clipboard: Some(&mut copy),
        ..ReaderServices::default()
    });
    copied
}
